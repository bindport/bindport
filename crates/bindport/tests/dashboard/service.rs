// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn dashboard_start_status_stop_controls_background_service() {
    let registry_path = temp_registry_path("dashboard-service-registry");
    let state_home = temp_test_dir("dashboard-service-state");
    let port = free_loopback_port();
    let port_arg = port.to_string();
    let start = bindport_with_registry(&registry_path)
        .env("XDG_STATE_HOME", &state_home)
        .args(["dashboard", "start", "--port", &port_arg])
        .output()
        .expect("start dashboard service");

    assert!(
        start.status.success(),
        "dashboard start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    assert!(String::from_utf8_lossy(&start.stdout).contains("dashboard started:"));
    assert!(http_get(port, "/healthz").starts_with("HTTP/1.1 200 OK"));

    let status = bindport_with_registry(&registry_path)
        .env("XDG_STATE_HOME", &state_home)
        .args(["dashboard", "status"])
        .output()
        .expect("dashboard service status");
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("dashboard running:"));

    let stop = bindport_with_registry(&registry_path)
        .env("XDG_STATE_HOME", &state_home)
        .args(["dashboard", "stop"])
        .output()
        .expect("stop dashboard service");
    assert!(
        stop.status.success(),
        "dashboard stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
}
#[test]
fn dashboard_start_reports_child_startup_error() {
    let registry_path = temp_registry_path("dashboard-service-start-error-registry");
    let state_home = temp_test_dir("dashboard-service-start-error-state");
    let output = bindport_with_registry(&registry_path)
        .env("XDG_STATE_HOME", &state_home)
        .args(["dashboard", "start", "--auth", "required"])
        .output()
        .expect("start dashboard service without token");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("dashboard did not start:"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("BINDPORT_DASHBOARD_TOKEN is required"),
        "unexpected stderr: {stderr}"
    );
}
#[test]
fn dashboard_start_passes_cli_token_outside_child_argv() {
    let registry_path = temp_registry_path("dashboard-service-token-registry");
    let state_home = temp_test_dir("dashboard-service-token-state");
    let port = free_loopback_port();
    let port_arg = port.to_string();
    let start = bindport_with_registry(&registry_path)
        .env("XDG_STATE_HOME", &state_home)
        .args([
            "dashboard",
            "start",
            "--port",
            &port_arg,
            "--auth",
            "required",
            "--token",
            "secret",
        ])
        .output()
        .expect("start dashboard service with token");

    assert!(
        start.status.success(),
        "dashboard start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    let stdout = String::from_utf8_lossy(&start.stdout);
    assert!(stdout.contains("dashboard started:"));

    #[cfg(target_os = "linux")]
    {
        let pid = stdout
            .split_whitespace()
            .last()
            .expect("dashboard pid")
            .parse::<u32>()
            .expect("dashboard pid is numeric");
        let cmdline =
            fs::read(Path::new("/proc").join(pid.to_string()).join("cmdline")).expect("cmdline");
        assert!(
            !String::from_utf8_lossy(&cmdline).contains("secret"),
            "dashboard token leaked into child argv"
        );
    }

    assert!(http_get(port, "/api/status").starts_with("HTTP/1.1 401 Unauthorized"));
    assert!(
        http_get_with_auth(port, "/api/status", "Bearer secret").starts_with("HTTP/1.1 200 OK")
    );

    let stop = bindport_with_registry(&registry_path)
        .env("XDG_STATE_HOME", &state_home)
        .args(["dashboard", "stop"])
        .output()
        .expect("stop dashboard service");
    assert!(
        stop.status.success(),
        "dashboard stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
}
#[test]
#[cfg(target_os = "linux")]
fn dashboard_stop_removes_mismatched_state_without_signal() {
    let state_home = temp_test_dir("dashboard-service-mismatch-state");
    let state_dir = state_home.join(SERVICE_NAME);
    let state_file = state_dir.join("dashboard.state");
    fs::create_dir_all(&state_dir).expect("dashboard state dir");
    fs::write(
        &state_file,
        format!(
            "pid={}\nurl=http://127.0.0.1:27080\nprocess_start_time=0\n",
            std::process::id()
        ),
    )
    .expect("dashboard state file");

    let stop = bindport()
        .env("XDG_STATE_HOME", &state_home)
        .args(["dashboard", "stop"])
        .output()
        .expect("stop dashboard service");

    assert!(
        stop.status.success(),
        "dashboard stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(
        String::from_utf8_lossy(&stop.stdout).contains("no longer matches dashboard"),
        "unexpected stdout: {}",
        String::from_utf8_lossy(&stop.stdout)
    );
    assert!(!state_file.exists());
}
