use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Instant;

/// Scheduling classification for a thread inside the managed Wine/game process tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadClass {
    /// Background shader compiler threads (demoted to `SCHED_BATCH` + `nice +10`
    /// so they never starve the game's RenderThread/GameThread).
    ShaderCompiler,
    /// Latency-sensitive threads (never demoted).
    LatencyCritical,
    /// Untouched normal threads.
    Normal,
}

/// Classifies a Linux thread name (`/proc/<pid>/task/<tid>/comm`, max 15 chars).
///
/// NOTE: `dxvk-cs`, `dxvk-submit`, and `dxvk-queue` are latency-critical render
/// submission threads and MUST NOT be classified as shader compilers.
pub fn classify_thread_comm(comm: &str) -> ThreadClass {
    let lower = comm.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return ThreadClass::Normal;
    }

    if lower.starts_with("dxvk-shader")
        || lower.starts_with("vkd3d_shader")
        || lower.starts_with("vkd3d_pipeline")
        || lower.starts_with("vkd3d_worker")
        || lower.starts_with("shadercompile")
    {
        return ThreadClass::ShaderCompiler;
    }

    if lower.starts_with("dxvk-cs")
        || lower.starts_with("dxvk-submit")
        || lower.starts_with("dxvk-queue")
        || lower.starts_with("vkd3d_queue")
        || lower == "wineserver"
        || lower.starts_with("renderthread")
        || lower.starts_with("rhithread")
        || lower.starts_with("winepulse")
        || lower.starts_with("winepipewire")
    {
        return ThreadClass::LatencyCritical;
    }

    ThreadClass::Normal
}

/// Applies fail-soft scheduling policy to `tid`.
///
/// Only demotes `ShaderCompiler` threads (`SCHED_BATCH` + `nice +10`), which
/// always succeeds for unprivileged users on Linux and eliminates compilation
/// stutter without risking system starvation.
pub fn apply_thread_policy(tid: i32, class: ThreadClass) -> bool {
    match class {
        ThreadClass::ShaderCompiler => unsafe {
            let param = libc::sched_param { sched_priority: 0 };
            let sched_ok = libc::sched_setscheduler(tid, libc::SCHED_BATCH, &param) == 0;
            let nice_ok = libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, 10) == 0;
            sched_ok || nice_ok
        },
        ThreadClass::LatencyCritical | ThreadClass::Normal => false,
    }
}

/// Checks whether the host kernel supports `pidfd_open(2)`.
pub fn is_pidfd_supported() -> bool {
    let fd = open_pidfd(std::process::id() as i32);
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
        true
    } else {
        false
    }
}

fn open_pidfd(pid: i32) -> RawFd {
    unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::c_int, 0 as libc::c_uint) as RawFd }
}

/// Parses parent PID (`ppid`, 4th field) from a `/proc/<pid>/stat` line safely,
/// handling process names containing spaces or parentheses.
pub fn parse_proc_stat_ppid(stat_content: &str) -> Option<i32> {
    let rparen = stat_content.rfind(')')?;
    let after = stat_content.get(rparen + 2..)?;
    let mut fields = after.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse::<i32>().ok()
}

/// Collects `root_pid` and all descendant PIDs currently visible in `/proc`.
fn collect_descendant_pids(root_pid: i32) -> Vec<i32> {
    let mut children_map: HashMap<i32, Vec<i32>> = HashMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return vec![root_pid];
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<i32>() else {
            continue;
        };
        let stat_path = entry.path().join("stat");
        let Ok(stat) = fs::read_to_string(stat_path) else {
            continue;
        };
        if let Some(ppid) = parse_proc_stat_ppid(&stat) {
            children_map.entry(ppid).or_default().push(pid);
        }
    }

    let mut result = Vec::new();
    let mut queue = vec![root_pid];
    let mut seen = HashSet::new();
    seen.insert(root_pid);

    while let Some(pid) = queue.pop() {
        result.push(pid);
        if let Some(kids) = children_map.get(&pid) {
            for &kid in kids {
                if seen.insert(kid) {
                    queue.push(kid);
                }
            }
        }
    }

    result
}

/// Scans `/proc/<pid>/task/*/comm` across `root_pid`'s process tree and applies
/// scheduling policies to any unseen `ShaderCompiler` threads.
pub fn scan_and_tune_process_tree(root_pid: i32, tuned_tids: &mut HashSet<i32>) -> usize {
    let mut newly_tuned = 0usize;
    for pid in collect_descendant_pids(root_pid) {
        let task_dir = Path::new("/proc").join(pid.to_string()).join("task");
        let Ok(tasks) = fs::read_dir(&task_dir) else {
            continue;
        };
        for task in tasks.flatten() {
            let Ok(tid) = task.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            if tuned_tids.contains(&tid) {
                continue;
            }
            let Ok(comm) = fs::read_to_string(task.path().join("comm")) else {
                continue;
            };
            let class = classify_thread_comm(&comm);
            if class == ThreadClass::ShaderCompiler {
                if apply_thread_policy(tid, class) {
                    newly_tuned += 1;
                }
                tuned_tids.insert(tid);
            }
        }
    }
    newly_tuned
}

/// Waits for `child` to exit using event-driven `pidfd_open` + `poll` while
/// periodically demoting newly spawned DXVK/vkd3d shader compiler threads.
///
/// Falls back transparently to `child.wait()` when disabled or if `pidfd_open`
/// is unavailable on the host kernel.
pub fn wait_with_supervisor(child: &mut Child, enable_supervisor: bool) -> Result<ExitStatus, String> {
    if !enable_supervisor {
        return child
            .wait()
            .map_err(|err| format!("failed to wait for child process: {err}"));
    }

    let pid = child.id() as i32;
    let pidfd = open_pidfd(pid);
    if pidfd < 0 {
        return child
            .wait()
            .map_err(|err| format!("failed to wait for child process: {err}"));
    }

    struct PidFdGuard(RawFd);
    impl Drop for PidFdGuard {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.0);
            }
        }
    }
    let _guard = PidFdGuard(pidfd);

    let start = Instant::now();
    let mut tuned_tids: HashSet<i32> = HashSet::new();
    let mut pfd = libc::pollfd {
        fd: pidfd,
        events: libc::POLLIN,
        revents: 0,
    };

    loop {
        // Fast scan during the first 15s of startup while DXVK/vkd3d spawn worker
        // threads, then relax to a 2s steady-state interval (~0% CPU).
        let timeout_ms: libc::c_int = if start.elapsed().as_secs() < 15 {
            250
        } else {
            2_000
        };

        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if rc > 0 {
            break;
        }
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        // Timeout expired and process is still alive: tune any newly spawned shader threads.
        let _ = scan_and_tune_process_tree(pid, &mut tuned_tids);
    }

    child
        .wait()
        .map_err(|err| format!("failed to reap child process: {err}"))
}
