use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Instant;

/// Scheduling classification for a thread inside the managed Wine/game process tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadClass {
    /// Heavy shader compiler threads (`SCHED_BATCH` + `nice +10`).
    ShaderCompiler,
    /// Background disk cache, asset streaming I/O, and GC workers (`SCHED_BATCH` + `nice +6`)
    /// so open-world streaming yields cleanly to GameThread/RenderThread/RHIThread.
    BackgroundWorker,
    /// Latency-sensitive threads (never demoted).
    LatencyCritical,
    /// Untouched normal threads.
    Normal,
}

/// Classifies a Linux thread name (`/proc/<pid>/task/<tid>/comm`, max 15 chars).
///
/// NOTE: `dxvk-cs`, `dxvk-submit`, `dxvk-queue`, `vkd3d_queue`, `RenderThread`,
/// `RHIThread`, and `TaskGraphThread` are frame-critical and MUST NOT be demoted.
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

    if lower.starts_with("vkd3d-disk")
        || lower.starts_with("fasyncloadingth")
        || lower.starts_with("fasyncpurge")
        || lower.starts_with("iothreadpool")
        || lower.starts_with("clvasyncbuildpo")
        || lower.starts_with("sgen worker")
    {
        return ThreadClass::BackgroundWorker;
    }

    if lower.starts_with("dxvk-cs")
        || lower.starts_with("dxvk-submit")
        || lower.starts_with("dxvk-queue")
        || lower.starts_with("vkd3d_queue")
        || lower.starts_with("vkd3d_fence")
        || lower.starts_with("vkd3d-swapchain")
        || lower == "wineserver"
        || lower.starts_with("renderthread")
        || lower.starts_with("rhithread")
        || lower.starts_with("audiothread")
        || lower.starts_with("audiomixer")
        || lower.starts_with("faudio_")
        || lower.starts_with("winepulse")
        || lower.starts_with("winepipewire")
    {
        return ThreadClass::LatencyCritical;
    }

    ThreadClass::Normal
}

/// Applies fail-soft scheduling policy to `tid`.
///
/// Only demotes `ShaderCompiler` (`SCHED_BATCH` + `nice +10`) and `BackgroundWorker`
/// (`SCHED_BATCH` + `nice +6`), which always succeeds for unprivileged Linux users
/// and reduces RenderThread preemption without risking system starvation.
pub fn apply_thread_policy(tid: i32, class: ThreadClass) -> bool {
    let nice_val = match class {
        ThreadClass::ShaderCompiler => 10,
        ThreadClass::BackgroundWorker => 6,
        ThreadClass::LatencyCritical | ThreadClass::Normal => return false,
    };

    unsafe {
        let param = libc::sched_param { sched_priority: 0 };
        let sched_ok = libc::sched_setscheduler(tid, libc::SCHED_BATCH, &param) == 0;
        let nice_ok = libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, nice_val) == 0;
        sched_ok || nice_ok
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

/// Collects `root_pid`, all descendant PIDs, and (optionally) any Wine processes
/// sharing `wineprefix` (catching processes reparented away by Wine's loader).
fn collect_managed_pids(root_pid: i32, wineprefix: Option<&Path>) -> Vec<i32> {
    let mut children_map: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut prefix_matches: Vec<i32> = Vec::new();
    let needle = wineprefix.map(|p| format!("WINEPREFIX={}", p.display()).into_bytes());
    let self_pid = std::process::id() as i32;

    let Ok(entries) = fs::read_dir("/proc") else {
        return vec![root_pid];
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<i32>() else {
            continue;
        };
        let stat_path = entry.path().join("stat");
        if let Ok(stat) = fs::read_to_string(stat_path)
            && let Some(ppid) = parse_proc_stat_ppid(&stat)
        {
            children_map.entry(ppid).or_default().push(pid);
        }

        if pid != self_pid
            && let Some(ref prefix_needle) = needle
            && let Ok(env_bytes) = fs::read(entry.path().join("environ"))
            && env_bytes.split(|&b| b == 0).any(|entry| entry == prefix_needle.as_slice())
        {
            prefix_matches.push(pid);
        }
    }

    let mut result = Vec::new();
    let mut queue = vec![root_pid];
    let mut seen = HashSet::new();
    seen.insert(root_pid);

    for pid in prefix_matches {
        if seen.insert(pid) {
            queue.push(pid);
        }
    }

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

/// Scans `/proc/<pid>/task/*/comm` across `root_pid`'s process tree (and optional
/// matching `WINEPREFIX` processes) and applies scheduling policies to unseen
/// `ShaderCompiler` and `BackgroundWorker` threads.
pub fn scan_and_tune_process_tree(
    root_pid: i32,
    wineprefix: Option<&Path>,
    tuned_tids: &mut HashSet<i32>,
) -> usize {
    let mut newly_tuned = 0usize;
    for pid in collect_managed_pids(root_pid, wineprefix) {
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
            if matches!(
                class,
                ThreadClass::ShaderCompiler | ThreadClass::BackgroundWorker
            ) {
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
/// periodically demoting newly spawned shader compiler & background streaming threads.
///
/// Falls back transparently to `child.wait()` when disabled or if `pidfd_open`
/// is unavailable on the host kernel.
pub fn wait_with_supervisor(
    child: &mut Child,
    wineprefix: Option<&Path>,
    enable_supervisor: bool,
) -> Result<ExitStatus, String> {
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
        // Fast scan during the first 15s of startup while DXVK/vkd3d/game spawn worker
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

        // Timeout expired and process is still alive: tune any newly spawned background threads.
        let _ = scan_and_tune_process_tree(pid, wineprefix, &mut tuned_tids);
    }

    child
        .wait()
        .map_err(|err| format!("failed to reap child process: {err}"))
}
