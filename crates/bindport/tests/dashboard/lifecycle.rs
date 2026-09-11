// SPDX-License-Identifier: MIT

use std::{os::unix::fs::MetadataExt, sync::Barrier};

use super::service_support::*;
use crate::support::*;

#[test]
fn concurrent_dashboard_starts_share_one_server_and_state() {
    let fixture = ServiceFixture::new("dashboard-concurrent-start");
    let barrier = Barrier::new(6);
    let outputs = thread::scope(|scope| {
        let handles = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    fixture.run("start")
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("starter thread"))
            .collect::<Vec<_>>()
    });
    for output in &outputs {
        assert_success(output);
    }
    let endpoints = outputs
        .iter()
        .map(|output| endpoint(&String::from_utf8_lossy(&output.stdout)))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        endpoints.len(),
        1,
        "concurrent starters created multiple dashboards"
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.stdout.starts_with(b"dashboard started:"))
            .count(),
        1
    );
    let &(pid, port) = endpoints.first().expect("dashboard endpoint");
    assert!(http_get(port, "/healthz").starts_with("HTTP/1.1 200 OK"));
    let state_path = fixture
        .state_home
        .join(SERVICE_NAME)
        .join("dashboard.state");
    let state = fs::read_to_string(&state_path).expect("dashboard state");
    assert!(state.lines().any(|line| line == format!("pid={pid}")));
    assert!(
        state
            .lines()
            .any(|line| line == format!("url=http://127.0.0.1:{port}"))
    );
    let status = fixture.run("status");
    assert_success(&status);
    assert_eq!(
        endpoint(&String::from_utf8_lossy(&status.stdout)),
        (pid, port)
    );
    assert_success(&fixture.run("stop"));
    assert!(!state_path.exists());
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect(("127.0.0.1", port)).is_ok() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "stopped dashboard still listens"
    );
}

#[test]
fn dashboard_lifecycle_commands_wait_for_the_same_lock() {
    let fixture = ServiceFixture::new("dashboard-lock-wait");
    let mut blocked = Vec::new();
    for operation in ["status", "stop", "start"] {
        let lock = open_lock(&fixture.lock_path());
        lock.lock().expect("hold lifecycle lock");
        let mut child = fixture.command(operation).spawn().expect("spawn command");
        let waited = wait_for_child(&mut child, Duration::from_millis(250)).is_none();
        drop(lock);
        let output = fixture.finish(child);
        assert_success(&output);
        blocked.push((operation, waited));
    }
    assert!(
        blocked.iter().all(|(_, waited)| *waited),
        "commands bypassed lock: {blocked:?}"
    );
}

#[test]
fn dashboard_lock_file_survives_holder_crash_without_blocking_start() {
    const LOCK_ENV: &str = "BINDPORT_TEST_DASHBOARD_LOCK_PATH";
    if let Some(path) = std::env::var_os(LOCK_ENV) {
        let path = PathBuf::from(path);
        let lock = open_lock(&path);
        lock.lock().expect("worker lock");
        fs::write(path.with_extension("ready"), "ready").expect("worker ready");
        loop {
            thread::park();
        }
    }

    let fixture = ServiceFixture::new("dashboard-lock-crash");
    let lock_path = fixture.lock_path();
    let ready = lock_path.with_extension("ready");
    let mut worker = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "lifecycle::dashboard_lock_file_survives_holder_crash_without_blocking_start",
        ])
        .env(LOCK_ENV, &lock_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lock worker");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let ready = ready.exists();
    let _ = worker.kill();
    worker.wait().expect("reap lock worker");
    assert!(ready, "lock worker did not acquire lock");
    let inode = fs::metadata(&lock_path)
        .expect("persistent lock file")
        .ino();
    assert_success(&fixture.run("start"));
    let tracked = fixture.servers.lock().expect("server list").len();
    assert_eq!(tracked, 1);
    assert_success(&fixture.run("status"));
    assert_success(&fixture.run("stop"));
    assert!(fixture.servers.lock().expect("server list").is_empty());
    assert_eq!(
        fs::metadata(&lock_path).expect("lock after stop").ino(),
        inode
    );
    assert_success(&fixture.run("start"));
    assert_eq!(
        fs::metadata(&lock_path).expect("lock after restart").ino(),
        inode
    );
}

#[test]
fn dashboard_start_failure_releases_the_lifecycle_lock() {
    let fixture = ServiceFixture::new("dashboard-lock-start-error");
    let failed = fixture.finish(
        fixture
            .command("start")
            .args([
                "--auth",
                "required",
                "--token-env",
                "BINDPORT_TEST_MISSING_DASHBOARD_TOKEN",
            ])
            .env_remove("BINDPORT_TEST_MISSING_DASHBOARD_TOKEN")
            .spawn()
            .expect("spawn failing start"),
    );
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stderr)
            .contains("BINDPORT_TEST_MISSING_DASHBOARD_TOKEN is required")
    );
    assert_success(&fixture.run("start"));
    assert_success(&fixture.run("status"));
}

#[test]
fn dashboard_lock_errors_fail_before_state_or_log_mutation() {
    let fixture = ServiceFixture::new("dashboard-lock-open-error");
    fs::create_dir(fixture.lock_path()).expect("directory at lock path");
    for operation in ["start", "status", "stop"] {
        let output = fixture.run(operation);
        assert!(!output.status.success(), "{operation} ignored lock error");
        assert!(String::from_utf8_lossy(&output.stderr).contains("dashboard service unavailable"));
    }
    let state_dir = fixture.state_home.join(SERVICE_NAME);
    assert!(!state_dir.join("dashboard.state").exists());
    assert!(!state_dir.join("dashboard.log").exists());
}
