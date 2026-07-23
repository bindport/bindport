// SPDX-License-Identifier: MIT

#![cfg(unix)]

use crate::support::*;
use rusqlite::Connection;

fn reserve_all_port(registry_path: &Path, root: &Path) -> u16 {
    let reserve = bindport_with_registry(registry_path)
        .current_dir(root)
        .args(["reserve", "--all"])
        .output()
        .expect("reserve all");
    assert!(
        reserve.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&reserve.stderr)
    );

    lookup_port(registry_path, root)
}

fn lookup_port(registry_path: &Path, root: &Path) -> u16 {
    let output = bindport_with_registry(registry_path)
        .current_dir(root)
        .args(["port", "web"])
        .output()
        .expect("port lookup");
    assert!(
        output.status.success(),
        "port lookup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .expect("port stdout")
        .trim()
        .parse()
        .expect("decimal port")
}

fn free_loopback_range() -> (u16, u16) {
    loop {
        let first = TcpListener::bind(("127.0.0.1", 0)).expect("bind first range port");
        let start = first.local_addr().expect("first address").port();
        let Some(end) = start.checked_add(1) else {
            continue;
        };
        if let Ok(second) = TcpListener::bind(("127.0.0.1", end)) {
            drop(second);
            drop(first);
            return (start, end);
        }
    }
}

fn status_json(registry_path: &Path, root: &Path) -> Value {
    let output = bindport_with_registry(registry_path)
        .current_dir(root)
        .args(["status", "--json"])
        .output()
        .expect("status");
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).expect("status json")
}

#[test]
fn reserved_run_promotes_same_lease_and_records_route_command_and_cwd() {
    let registry_path = temp_registry_path("reserved-run-success-registry");
    let root = temp_test_dir("reserved-run-success-root")
        .canonicalize()
        .expect("canonical root");
    let service_root = root.join("apps").join("web");
    fs::create_dir_all(&service_root).expect("service root");
    let service_root = service_root.canonicalize().expect("canonical service root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "reserved-run"
default_range = "{port}-{port}"
skip_ports = []

[[services]]
name = "web"
path = "apps/web"
hostname = "web.reserved.localhost"
route_url = "http://{{hostname}}:{{port}}"
health_url = "{{route_url}}/health"
command = ["sh", "-c", "printf '%s|%s' \"$PORT\" \"$(pwd -P)\""]
"#
        ),
    )
    .expect("write config");

    let reserved_port = reserve_all_port(&registry_path, &root);
    let before = Registry::open(&registry_path)
        .expect("registry")
        .export_snapshot()
        .expect("export");
    let lease_id = before.leases[0].id;

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web"])
        .output()
        .expect("run reserved service");
    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!("{reserved_port}|{}", service_root.display())
    );

    let mut registry = Registry::open(&registry_path).expect("registry");
    let after = registry.export_snapshot().expect("export");
    assert_eq!(after.leases.len(), 1);
    assert_eq!(after.leases[0].id, lease_id);
    assert_eq!(after.leases[0].port, reserved_port);
    assert_eq!(
        after.leases[0].hostname.as_deref(),
        Some("web.reserved.localhost")
    );
    assert_eq!(
        after.leases[0].route_url.as_deref(),
        Some(format!("http://web.reserved.localhost:{reserved_port}").as_str())
    );
    assert_eq!(after.runs.len(), 1);
    assert_eq!(after.runs[0].lease_id, lease_id);
    assert_eq!(after.runs[0].cwd, service_root.display().to_string());
    assert!(after.runs[0].command.contains("printf '%s|%s'"));

    let status = registry.status_snapshot().expect("status");
    assert_eq!(status.services[0].state, "stopped");
    assert_eq!(status.services[0].port, reserved_port);
    assert_eq!(status.services[0].cwd, service_root.display().to_string());
    assert_eq!(status.services[0].exit_code, Some(0));
}

#[test]
fn reserved_run_spawn_failure_keeps_reservation() {
    let registry_path = temp_registry_path("reserved-spawn-failure-registry");
    let root = temp_test_dir("reserved-spawn-failure-root")
        .canonicalize()
        .expect("canonical root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"reserved-spawn-failure\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\ncommand = [\"bindport-command-that-does-not-exist\"]\n"
        ),
    )
    .expect("write config");
    let reserved_port = reserve_all_port(&registry_path, &root);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web"])
        .output()
        .expect("run reserved service");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to spawn"));
    assert_eq!(lookup_port(&registry_path, &root), reserved_port);
    let status = status_json(&registry_path, &root);
    assert_eq!(status["services"][0]["state"], "reserved");
    assert!(status["runs"].as_array().expect("runs").is_empty());
}

#[test]
fn occupied_reserved_port_fails_before_spawn_and_remains_queryable() {
    let registry_path = temp_registry_path("reserved-occupied-registry");
    let root = temp_test_dir("reserved-occupied-root")
        .canonicalize()
        .expect("canonical root");
    let marker = root.join("spawned");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"reserved-occupied\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\n"
        ),
    )
    .expect("write config");
    let reserved_port = reserve_all_port(&registry_path, &root);
    let listener = TcpListener::bind(("127.0.0.1", reserved_port)).expect("occupy reserved port");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf spawned > \"$1\"",
            "sh",
            marker.to_str().expect("marker path"),
        ])
        .output()
        .expect("run reserved service");

    assert!(!output.status.success());
    assert!(!marker.exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("reserved port {reserved_port} is occupied")));
    assert!(stderr.contains("no child was spawned"));
    assert_eq!(lookup_port(&registry_path, &root), reserved_port);
    assert_eq!(
        status_json(&registry_path, &root)["services"][0]["state"],
        "reserved"
    );

    drop(listener);
}

#[test]
fn promotion_failure_terminates_child_and_leaves_no_false_active_run() {
    let registry_path = temp_registry_path("reserved-promotion-failure-registry");
    let root = temp_test_dir("reserved-promotion-failure-root")
        .canonicalize()
        .expect("canonical root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"reserved-promotion-failure\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\n"
        ),
    )
    .expect("write config");
    let reserved_port = reserve_all_port(&registry_path, &root);
    let before = Registry::open(&registry_path)
        .expect("registry")
        .export_snapshot()
        .expect("export");
    let lease_id = before.leases[0].id;
    Connection::open(&registry_path)
        .expect("sqlite connection")
        .execute_batch(
            "CREATE TRIGGER fail_reserved_promotion
             BEFORE UPDATE OF state ON leases
             WHEN OLD.state = 'reserved' AND NEW.state = 'active'
             BEGIN
                 SELECT RAISE(ABORT, 'forced promotion failure');
             END;",
        )
        .expect("promotion failure trigger");

    let started_at = Instant::now();
    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sleep", "10"])
        .output()
        .expect("run reserved service");

    assert!(!output.status.success());
    assert!(started_at.elapsed() < Duration::from_secs(5));
    assert!(String::from_utf8_lossy(&output.stderr).contains("child was terminated"));
    assert_eq!(lookup_port(&registry_path, &root), reserved_port);
    let mut registry = Registry::open(&registry_path).expect("registry");
    let after = registry.export_snapshot().expect("export");
    assert_eq!(after.leases.len(), 1);
    assert_eq!(after.leases[0].id, lease_id);
    assert_eq!(after.leases[0].port, reserved_port);
    assert!(after.runs.is_empty());
    let status = registry.status_snapshot().expect("status");
    assert_eq!(status.services[0].state, "reserved");
}

#[test]
fn reserved_startup_port_race_keeps_reservation_without_retrying() {
    let registry_path = temp_registry_path("reserved-race-registry");
    let root = temp_test_dir("reserved-race-root")
        .canonicalize()
        .expect("canonical root");
    let marker_path = root.join("race-port");
    let pid_path = root.join("listener-pid");
    let (range_start, range_end) = free_loopback_range();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"reserved-race\"\ndefault_range = \"{range_start}-{range_end}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\n"
        ),
    )
    .expect("write config");
    let reserved_port = reserve_all_port(&registry_path, &root);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            concat!(
                "if [ ! -f \"$1\" ]; then ",
                "python3 -c 'import os,socket,sys,time; from pathlib import Path; ",
                "s=socket.socket(); ",
                "s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); ",
                "s.bind((\"127.0.0.1\", int(sys.argv[1]))); ",
                "s.listen(); ",
                "Path(sys.argv[2]).write_text(str(os.getpid())); ",
                "Path(sys.argv[3]).write_text(sys.argv[1]); ",
                "time.sleep(5)' \"$PORT\" \"$2\" \"$1\" & ",
                "i=0; ",
                "while [ ! -f \"$1\" ] && [ \"$i\" -lt 100 ]; do ",
                "i=$((i + 1)); sleep 0.02; ",
                "done; ",
                "[ -f \"$1\" ] || exit 99; ",
                "exit 98; ",
                "fi; ",
                "printf '%s' \"$PORT\"",
            ),
            "sh",
            marker_path.to_str().expect("marker path"),
            pid_path.to_str().expect("pid path"),
        ])
        .output()
        .expect("run reserved service");

    terminate_process_from_file(&pid_path);

    assert_eq!(output.status.code(), Some(98));
    assert!(output.stdout.is_empty(), "reserved run silently retried");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("reservation was kept"));
    assert!(stderr.contains("no alternate port was assigned"));
    assert!(!stderr.contains("retrying with another port"));
    assert_eq!(lookup_port(&registry_path, &root), reserved_port);
    let status = status_json(&registry_path, &root);
    assert_eq!(status["services"][0]["state"], "reserved");
    assert_eq!(status["runs"].as_array().expect("runs").len(), 1);
    assert_eq!(status["runs"][0]["exit_code"], 98);
}

#[test]
fn reserved_runs_are_isolated_to_the_current_project_worktree() {
    let registry_path = temp_registry_path("reserved-isolation-registry");
    let first_root = temp_test_dir("reserved-isolation-first")
        .canonicalize()
        .expect("canonical first root");
    let second_root = temp_test_dir("reserved-isolation-second")
        .canonicalize()
        .expect("canonical second root");
    let (range_start, range_end) = free_loopback_range();
    let config = format!(
        "project = \"reserved-isolation\"\ndefault_range = \"{range_start}-{range_end}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\n"
    );
    fs::write(first_root.join(".bindport.toml"), &config).expect("first config");
    fs::write(second_root.join(".bindport.toml"), config).expect("second config");
    let reserved_first = reserve_all_port(&registry_path, &first_root);
    let reserved_second = reserve_all_port(&registry_path, &second_root);
    assert_ne!(reserved_first, reserved_second);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&first_root)
        .args(["run", "web", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run first reserved service");

    assert!(output.status.success());
    assert_eq!(output.stdout, reserved_first.to_string().as_bytes());
    assert_eq!(lookup_port(&registry_path, &second_root), reserved_second);
    let status = status_json(&registry_path, &second_root);
    assert!(
        status["services"]
            .as_array()
            .expect("services")
            .iter()
            .any(|service| service["port"] == reserved_second && service["state"] == "reserved")
    );
}
