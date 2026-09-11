// SPDX-License-Identifier: MIT

use super::service_support::*;
use crate::support::*;

fn hook_fixture(name: &str) -> ServiceFixture {
    let fixture = ServiceFixture::new(name);
    fs::write(
        fixture.root.join(".bindport.toml"),
        "project = \"dashboard-hook-output\"\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"cleanup\"\nevents = [\"routes_removed\"]\ncommand = [\"sh\", \"-ec\", \"printf 'hook-stdout\\n'; printf 'hook-stderr\\n' >&2; printf finished > hook-finished.txt\"]\n",
    ).expect("hook config");
    let trust = bindport_with_registry(&fixture.registry)
        .current_dir(&fixture.root)
        .env("XDG_STATE_HOME", &fixture.state_home)
        .args(["hooks", "trust", "cleanup"])
        .output()
        .expect("trust hook");
    assert_success(&trust);
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("fixture port");
    let mut registry = Registry::open(&fixture.registry).expect("registry");
    let started = registry
        .record_run_started(&RunStart {
            project: "dashboard-hook-output".into(),
            service: "web".into(),
            identity: None,
            host: "127.0.0.1".into(),
            port: listener.local_addr().expect("fixture address").port(),
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: fixture.root.clone(),
        })
        .expect("fixture run");
    registry
        .record_run_finished(started, Some(0))
        .expect("finished fixture");
    fixture
}

fn assert_hook_finished(fixture: &ServiceFixture) {
    assert_eq!(
        fs::read_to_string(fixture.root.join("hook-finished.txt")).expect("hook finished marker"),
        "finished"
    );
}

#[test]
fn background_dashboard_hook_stdout_goes_to_log_and_hook_finishes() {
    let fixture = hook_fixture("dashboard-background-hook-output");
    let started = fixture.run("start");
    assert_success(&started);
    let (_, port) = endpoint(&String::from_utf8_lossy(&started.stdout));
    let response = http_post_clean(port, "/api/clean/stopped", None);
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert_hook_finished(&fixture);
    let log = fs::read_to_string(fixture.state_home.join(SERVICE_NAME).join("dashboard.log"))
        .expect("dashboard log");
    assert!(log.contains("hook-stdout"), "{log}");
    assert!(log.contains("hook-stderr"), "{log}");
    assert!(!log.contains("hook `cleanup` failed"), "{log}");
}

#[test]
fn foreground_dashboard_hook_keeps_redirected_stdout() {
    let fixture = hook_fixture("dashboard-foreground-hook-output");
    let stdout = fixture.root.join("foreground.stdout");
    let dashboard = start_foreground(fixture.command("serve"), &stdout);
    let response = http_post_clean(dashboard.port, "/api/clean/stopped", None);
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert_hook_finished(&fixture);
    let output = fs::read_to_string(&stdout).expect("foreground output");
    assert!(output.contains("hook-stdout"), "{output}");
    assert!(!output.contains("hook-stderr"), "{output}");
    let stderr = fs::read_to_string(stdout.with_extension("stderr")).expect("foreground stderr");
    assert!(stderr.contains("hook-stderr"), "{stderr}");
}

#[test]
fn cli_cleanup_hook_keeps_captured_stdout() {
    let fixture = hook_fixture("dashboard-cli-hook-output-control");
    let output = bindport_with_registry(&fixture.registry)
        .current_dir(&fixture.root)
        .env("XDG_STATE_HOME", &fixture.state_home)
        .args(["clean", "--stopped"])
        .output()
        .expect("CLI cleanup");
    assert_success(&output);
    assert_hook_finished(&fixture);
    assert!(String::from_utf8_lossy(&output.stdout).contains("hook-stdout"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("hook-stderr"));
}
