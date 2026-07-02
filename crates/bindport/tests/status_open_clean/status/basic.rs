// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn status_json_starts_empty() {
    let registry_path = temp_registry_path("empty-status");
    let output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(output.status.success());

    let status = serde_json::from_slice::<Value>(&output.stdout).expect("status json");
    assert_eq!(
        object_keys(&status),
        BTreeSet::from([
            "generated_at",
            "hooks",
            "outputs",
            "runs",
            "schema_version",
            "services",
        ])
    );
    assert_eq!(status["schema_version"], "0.4");
    assert!(
        status["generated_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(status["outputs"].as_array().expect("outputs").len(), 0);
    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
    assert_eq!(status["hooks"]["items"].as_array().expect("hooks").len(), 0);
}
#[test]
fn status_json_reports_finished_run() {
    let registry_path = temp_registry_path("finished-status");
    let run_output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(run_output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(services.len(), 1);
    assert_eq!(runs.len(), 1);
    assert!(
        BTreeSet::from([
            "project",
            "service",
            "state",
            "port",
            "host",
            "url",
            "hostname",
            "route_url",
            "health_url",
            "worktree_path",
            "worktree_hash",
            "git_common_dir",
            "branch",
            "branch_label",
            "commit",
            "identity_key",
            "pid",
            "command",
            "cwd",
            "started_at",
            "exited_at",
            "exit_code",
            "health",
            "outputs",
            "proxy",
        ])
        .is_subset(&object_keys(&services[0]))
    );
    assert!(
        BTreeSet::from([
            "id",
            "lease_id",
            "pid",
            "command",
            "cwd",
            "started_at",
            "exited_at",
            "exit_code",
        ])
        .is_subset(&object_keys(&runs[0]))
    );
    assert_eq!(services[0]["state"], "stopped");
    assert_eq!(services[0]["exit_code"], 0);
    assert!(services[0]["port"].as_u64().expect("port") >= DEFAULT_PORT_RANGE.start as u64);
    assert!(services[0]["port"].as_u64().expect("port") <= DEFAULT_PORT_RANGE.end as u64);
    assert_eq!(services[0]["hostname"], Value::Null);
    assert_eq!(services[0]["route_url"], Value::Null);
    assert_eq!(services[0]["outputs"].as_array().expect("outputs").len(), 0);
    assert_eq!(services[0]["proxy"], Value::Null);
    assert_eq!(runs[0]["exit_code"], 0);
}
#[test]
fn status_reports_latest_service_once_and_keeps_run_history() {
    let registry_path = temp_registry_path("deduped-status");
    let root = temp_test_dir("deduped-status-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"status-project\"\nservice = \"web\"\ndefault_range = \"29320-29321\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    let second_port = run_print_port(&registry_path, &root);

    assert_eq!(second_port, first_port);

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status json");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(services.len(), 1);
    assert_eq!(runs.len(), 2);
    assert_eq!(services[0]["project"], "status-project");
    assert_eq!(services[0]["service"], "web");
    assert_eq!(
        services[0]["port"].as_u64().expect("service port"),
        u64::from(second_port)
    );
    assert_eq!(services[0]["pid"], runs[0]["pid"]);
    assert_eq!(services[0]["started_at"], runs[0]["started_at"]);

    let plain_status = bindport_with_registry(&registry_path)
        .args(["status"])
        .output()
        .expect("run bindport status");

    assert!(plain_status.status.success());
    let stdout = String::from_utf8(plain_status.stdout).expect("plain status stdout");
    let lines = stdout.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains(&format!("stopped\tweb\t127.0.0.1:{second_port}")));
}
