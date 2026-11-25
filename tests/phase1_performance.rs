use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;

use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::runtime::build_runtime_env;
use hyperx_proton::shader_warm::{
    collect_shader_cache_dirs, prewarm_shader_cache_dirs, spawn_shader_cache_prewarm,
};
use hyperx_proton::supervisor::{
    ThreadClass, classify_thread_comm, is_pidfd_supported, parse_proc_stat_ppid,
    scan_and_tune_process_tree, wait_with_supervisor,
};
use hyperx_proton::sys_tune::{
    DEFAULT_TIMERSLACK_NS, apply_process_tuning, reflink_or_copy,
};
use hyperx_proton::topology::{
    compute_shader_compiler_threads, count_unique_physical_cores, detect_physical_cores,
    detect_physical_cores_from_sysfs,
};

#[test]
fn topology_counts_physical_cores_and_computes_shader_budget() {
    // 4 physical cores with 2-way SMT (8 logical threads)
    let smt_pairs = vec![
        (0, 0),
        (0, 0),
        (0, 1),
        (0, 1),
        (0, 2),
        (0, 2),
        (0, 3),
        (0, 3),
    ];
    assert_eq!(count_unique_physical_cores(&smt_pairs), 4);

    // Fake sysfs tree with 6 physical cores across 12 logical CPUs
    let tmp = tempfile::tempdir().unwrap();
    for cpu in 0..12u32 {
        let topo = tmp.path().join(format!("cpu{cpu}/topology"));
        fs::create_dir_all(&topo).unwrap();
        fs::write(topo.join("physical_package_id"), "0\n").unwrap();
        fs::write(topo.join("core_id"), format!("{}\n", cpu % 6)).unwrap();
    }
    fs::create_dir_all(tmp.path().join("cpufreq")).unwrap();
    assert_eq!(detect_physical_cores_from_sysfs(tmp.path()), Some(6));
    assert!(detect_physical_cores() >= 1);

    assert_eq!(compute_shader_compiler_threads(1), 1);
    assert_eq!(compute_shader_compiler_threads(2), 1);
    assert_eq!(compute_shader_compiler_threads(4), 2);
    assert_eq!(compute_shader_compiler_threads(6), 4);
    assert_eq!(compute_shader_compiler_threads(8), 6);
    assert_eq!(compute_shader_compiler_threads(16), 8);
}

#[test]
fn runtime_env_sets_shader_threads_and_respects_user_overrides() {
    let runner = Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(Path::new("/tmp/SHARED")), None).unwrap();

    let host_env = BTreeMap::new();
    let rt = build_runtime_env(runner, &layout, &host_env, true);
    let expected = compute_shader_compiler_threads(detect_physical_cores()).to_string();
    assert_eq!(
        rt.env.get("DXVK_NUM_COMPILER_THREADS").map(String::as_str),
        Some(expected.as_str())
    );
    assert_eq!(
        rt.env.get("VKD3D_WORKER_THREADS").map(String::as_str),
        Some(expected.as_str())
    );
    assert_eq!(
        rt.env.get("DXVK_STATE_CACHE").map(String::as_str),
        Some("1")
    );

    // Explicit per-component override wins over HYPERX_PROTON_SHADER_THREADS
    let mut custom = BTreeMap::new();
    custom.insert("HYPERX_PROTON_SHADER_THREADS".to_string(), "3".to_string());
    custom.insert("DXVK_NUM_COMPILER_THREADS".to_string(), "5".to_string());
    let rt_custom = build_runtime_env(runner, &layout, &custom, true);
    assert_eq!(
        rt_custom
            .env
            .get("DXVK_NUM_COMPILER_THREADS")
            .map(String::as_str),
        Some("5")
    );
    assert_eq!(
        rt_custom.env.get("VKD3D_WORKER_THREADS").map(String::as_str),
        Some("3")
    );
}

#[test]
fn sys_tune_applies_timerslack_nofile_and_reflink_or_copy() {
    let report = apply_process_tuning(DEFAULT_TIMERSLACK_NS);
    assert!(report.timerslack_applied);
    assert_eq!(report.timerslack_ns, DEFAULT_TIMERSLACK_NS);
    assert!(report.nofile_soft.unwrap_or(0) >= 1024);

    let actual_slack = unsafe { libc::prctl(libc::PR_GET_TIMERSLACK, 0, 0, 0, 0) };
    assert_eq!(actual_slack as u64, DEFAULT_TIMERSLACK_NS);

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src.dll");
    let dst = tmp.path().join("dst.dll");
    fs::write(&src, b"HYPERX_TEST_PAYLOAD").unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();

    let _method = reflink_or_copy(&src, &dst).expect("reflink_or_copy should succeed");
    assert_eq!(fs::read(&dst).unwrap(), b"HYPERX_TEST_PAYLOAD");
    assert_eq!(
        fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn shader_prewarm_collects_dirs_and_advises_cache_files_only() {
    let tmp = tempfile::tempdir().unwrap();
    let compat = tmp.path().join("SHARED");
    let dxvk_dir = compat.join("pfx/drive_c/users/steamuser/AppData/Local/dxvk");
    let gl_dir = tmp.path().join("glcache");
    fs::create_dir_all(&dxvk_dir).unwrap();
    fs::create_dir_all(&gl_dir).unwrap();

    fs::write(dxvk_dir.join("game.dxvk-cache"), vec![1u8; 4096]).unwrap();
    fs::write(gl_dir.join("shader.bin"), vec![2u8; 8192]).unwrap();
    fs::write(gl_dir.join("vkd3d-proton.cache.write"), vec![3u8; 2048]).unwrap();
    fs::write(gl_dir.join("ignored.txt"), b"not a shader cache").unwrap();

    let layout = resolve_prefix_layout(Some(&compat), None).unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "__GL_SHADER_DISK_CACHE_PATH".to_string(),
        gl_dir.display().to_string(),
    );

    let dirs = collect_shader_cache_dirs(&layout, &env);
    assert_eq!(dirs.len(), 2);

    let stats = prewarm_shader_cache_dirs(&dirs);
    assert_eq!(stats.files_advised, 3);
    assert_eq!(stats.bytes_advised, 4096 + 8192 + 2048);

    let async_stats = spawn_shader_cache_prewarm(dirs).unwrap().join().unwrap();
    assert_eq!(async_stats.files_advised, 3);
}

#[test]
fn supervisor_classifies_threads_safely_parses_stat_and_tunes_real_thread() {
    // Shader compilers MUST be demoted
    assert_eq!(
        classify_thread_comm("dxvk-shader"),
        ThreadClass::ShaderCompiler
    );
    assert_eq!(
        classify_thread_comm("vkd3d_worker"),
        ThreadClass::ShaderCompiler
    );
    assert_eq!(
        classify_thread_comm("vkd3d_pipeline"),
        ThreadClass::ShaderCompiler
    );
    assert_eq!(
        classify_thread_comm("ShaderCompile"),
        ThreadClass::ShaderCompiler
    );

    // Render / submission / audio threads MUST NEVER be demoted
    assert_eq!(classify_thread_comm("dxvk-cs"), ThreadClass::LatencyCritical);
    assert_eq!(
        classify_thread_comm("dxvk-submit"),
        ThreadClass::LatencyCritical
    );
    assert_eq!(
        classify_thread_comm("dxvk-queue"),
        ThreadClass::LatencyCritical
    );
    assert_eq!(
        classify_thread_comm("RenderThread"),
        ThreadClass::LatencyCritical
    );
    assert_eq!(
        classify_thread_comm("RHIThread"),
        ThreadClass::LatencyCritical
    );
    assert_eq!(
        classify_thread_comm("wineserver"),
        ThreadClass::LatencyCritical
    );
    assert_eq!(classify_thread_comm("random-worker"), ThreadClass::Normal);

    // /proc/<pid>/stat parsing with spaces and parentheses in comm
    assert_eq!(
        parse_proc_stat_ppid("1234 (Client (Win64) Shipping) S 987 1234 1234 0 -1"),
        Some(987)
    );

    // Real thread demotion test inside current process
    let start_barrier = Arc::new(Barrier::new(2));
    let done_barrier = Arc::new(Barrier::new(2));
    let b_start = Arc::clone(&start_barrier);
    let b_done = Arc::clone(&done_barrier);

    let handle = thread::Builder::new()
        .name("dxvk-shader".to_string())
        .spawn(move || {
            b_start.wait();
            b_done.wait();
            unsafe { libc::sched_getscheduler(0) }
        })
        .unwrap();

    start_barrier.wait();
    let mut tuned = HashSet::new();
    let count = scan_and_tune_process_tree(std::process::id() as i32, &mut tuned);
    done_barrier.wait();
    let policy = handle.join().unwrap();

    assert!(count >= 1, "expected dxvk-shader thread to be tuned");
    assert_eq!(policy, libc::SCHED_BATCH);

    // pidfd event-driven child exit test
    assert!(is_pidfd_supported());
    let mut child = Command::new("bash")
        .args(["-c", "exit 42"])
        .spawn()
        .expect("spawn test child");
    let status = wait_with_supervisor(&mut child, true).expect("wait_with_supervisor");
    assert_eq!(status.code(), Some(42));
}
