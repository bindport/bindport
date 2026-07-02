// SPDX-License-Identifier: MIT

mod support;

use support::*;

#[test]
fn status_schema_document_matches_current_contract() {
    let schema = serde_json::from_str::<Value>(include_str!("../../../docs/status.schema.json"))
        .expect("status schema json");

    assert_eq!(schema["properties"]["schema_version"]["const"], "0.4");
    assert_eq!(schema["additionalProperties"].as_bool(), Some(false));

    let top_level_required = schema["required"]
        .as_array()
        .expect("top-level required fields")
        .iter()
        .map(|field| field.as_str().expect("required field"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        top_level_required,
        BTreeSet::from([
            "generated_at",
            "hooks",
            "outputs",
            "runs",
            "schema_version",
            "services",
        ])
    );

    let service_required = schema["$defs"]["service"]["required"]
        .as_array()
        .expect("service required fields")
        .iter()
        .map(|field| field.as_str().expect("service required field"))
        .collect::<BTreeSet<_>>();
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
        .is_subset(&service_required)
    );
}
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
fn open_prints_best_url_for_active_service() {
    let registry_path = temp_registry_path("open-service-url-registry");
    let root = temp_test_dir("open-service-url-root");
    let marker_path = temp_path("open-service-url-marker");
    let marker_arg = marker_path.display().to_string();
    fs::write(
        root.join(".bindport.toml"),
        "project = \"open-project\"\ndefault_range = \"29480-29481\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"web.localhost\"\nroute_url = \"https://{hostname}\"\n",
    )
    .expect("write open config");

    let mut child = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf ready > \"$1\"; sleep 2",
            "sh",
            &marker_arg,
        ])
        .spawn()
        .expect("spawn bindport service");

    wait_for_file_contains(&marker_path, "ready", Duration::from_secs(5));
    let stdout = wait_for_open_url(
        &registry_path,
        &["open", "web", "--print"],
        Duration::from_secs(5),
    );

    assert_eq!(stdout.trim(), "https://web.localhost");

    let status = wait_for_child(&mut child, Duration::from_secs(3)).expect("service exits");
    assert!(status.success());
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
#[test]
fn clean_dry_run_reports_without_removing_stopped_entries() {
    let registry_path = temp_registry_path("clean-dry-run");
    let run_output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf clean"])
        .output()
        .expect("run bindport");

    assert!(run_output.status.success());

    let dry_run = bindport_with_registry(&registry_path)
        .args(["clean", "--dry-run", "--json"])
        .output()
        .expect("run bindport clean dry-run");

    assert!(
        dry_run.status.success(),
        "clean dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );

    let report = serde_json::from_slice::<Value>(&dry_run.stdout).expect("clean json");
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["leases"], 1);
    assert_eq!(report["runs"], 1);
    assert_eq!(report["states"]["stopped"], 1);
    assert_eq!(report["states"]["stale"], 0);

    let status_after_dry_run = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status =
        serde_json::from_slice::<Value>(&status_after_dry_run.stdout).expect("status json");
    assert_eq!(status["services"].as_array().expect("services").len(), 1);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 1);

    let clean = bindport_with_registry(&registry_path)
        .args(["clean", "--json"])
        .output()
        .expect("run bindport clean");

    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let report = serde_json::from_slice::<Value>(&clean.stdout).expect("clean json");
    assert_eq!(report["dry_run"], false);
    assert_eq!(report["leases"], 1);
    assert_eq!(report["runs"], 1);

    let status_after_clean = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_after_clean.stdout).expect("status json");
    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
}
#[cfg(unix)]
#[test]
fn clean_requires_yes_for_noninteractive_stale_entries() {
    let registry_path = temp_registry_path("clean-stale-confirmation");
    let mut registry = Registry::open(&registry_path).expect("registry");
    registry
        .record_run_started(&RunStart {
            project: String::from("stale-clean"),
            service: String::from("web"),
            identity: None,
            host: String::from("127.0.0.1"),
            port: 29_501,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: PathBuf::from("/tmp/bindport-stale-clean-fixture"),
        })
        .expect("record stale candidate");

    let rejected = bindport_with_registry(&registry_path)
        .args(["clean", "--stale", "--json"])
        .output()
        .expect("run bindport clean");

    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("stale cleanup requires confirmation; rerun with --yes")
    );

    let accepted = bindport_with_registry(&registry_path)
        .args(["clean", "--stale", "--json", "--yes"])
        .output()
        .expect("run bindport clean with confirmation");

    assert!(
        accepted.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );

    let report = serde_json::from_slice::<Value>(&accepted.stdout).expect("clean json");
    assert_eq!(report["leases"], 1);
    assert_eq!(report["states"]["stale"], 1);
}
#[test]
fn clean_keeps_active_entries() {
    let registry_path = temp_registry_path("clean-keeps-active");
    let run_output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf clean"])
        .output()
        .expect("run bindport");

    assert!(run_output.status.success());
    reserve_registry_port(&registry_path, 29_501);

    let clean = bindport_with_registry(&registry_path)
        .args(["clean", "--json"])
        .output()
        .expect("run bindport clean");

    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let report = serde_json::from_slice::<Value>(&clean.stdout).expect("clean json");
    assert_eq!(report["leases"], 1);
    assert_eq!(report["states"]["stopped"], 1);

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(services.len(), 1);
    assert_eq!(runs.len(), 1);
    assert_eq!(services[0]["state"], "active");
    assert_eq!(services[0]["port"], 29_501);
}
#[test]
fn status_json_reports_git_identity() {
    let registry_path = temp_registry_path("git-identity-registry");
    let root = temp_test_dir("git-identity-root");
    init_git_repo(&root, "feature/tree");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(
        service["project"],
        root.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(service["branch"], "feature/tree");
    assert_eq!(service["branch_label"], "feature-tree");
    assert_eq!(
        service["worktree_path"],
        root.canonicalize().unwrap().display().to_string()
    );
    assert!(service["commit"].as_str().expect("commit").len() >= 7);
    assert!(
        service["identity_key"]
            .as_str()
            .expect("identity key")
            .starts_with("v1:")
    );
}
#[test]
fn same_service_in_distinct_worktrees_keeps_distinct_identities() {
    let registry_path = temp_registry_path("worktree-collision-registry");
    let first_root = temp_test_dir("worktree-collision-first");
    let second_root = temp_test_dir("worktree-collision-second");
    let config = "project = \"monorepo\"\ndefault_range = \"29440-29449\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n";

    for root in [&first_root, &second_root] {
        fs::create_dir_all(root.join("apps").join("web")).expect("service dir");
        fs::write(root.join(".bindport.toml"), config).expect("write config");
        init_git_repo(root, "feature/tree");
    }

    let first_marker = temp_path("worktree-collision-first-port");
    let first_marker_arg = first_marker.display().to_string();
    let mut first = bindport_with_registry(&registry_path)
        .current_dir(first_root.join("apps").join("web"))
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PORT\" > \"$1\"; sleep 2",
            "sh",
            &first_marker_arg,
        ])
        .spawn()
        .expect("start first service");
    let first_port = wait_for_file_contains(&first_marker, "", Duration::from_secs(5))
        .parse::<u16>()
        .expect("first port");

    let second_output = bindport_with_registry(&registry_path)
        .current_dir(second_root.join("apps").join("web"))
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run second service");
    assert!(
        second_output.status.success(),
        "second service failed: {}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    let second_port = String::from_utf8(second_output.stdout)
        .expect("second stdout")
        .parse::<u16>()
        .expect("second port");

    assert_ne!(first_port, second_port);
    assert!((29440..=29449).contains(&first_port));
    assert!((29440..=29449).contains(&second_port));

    let first_status = wait_for_child(&mut first, Duration::from_secs(5)).expect("first exits");
    assert!(first_status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    assert!(status_output.status.success());
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    assert_eq!(services.len(), 2);

    let identity_keys = services
        .iter()
        .map(|service| service["identity_key"].as_str().expect("identity key"))
        .collect::<BTreeSet<_>>();
    let worktree_paths = services
        .iter()
        .map(|service| service["worktree_path"].as_str().expect("worktree path"))
        .collect::<BTreeSet<_>>();
    let ports = services
        .iter()
        .map(|service| service["port"].as_u64().expect("service port"))
        .collect::<BTreeSet<_>>();

    assert_eq!(identity_keys.len(), 2);
    assert_eq!(worktree_paths.len(), 2);
    assert_eq!(ports.len(), 2);
    for service in services {
        assert_eq!(service["project"], "monorepo");
        assert_eq!(service["service"], "web");
        assert_eq!(service["branch"], "feature/tree");
    }
}
#[test]
fn status_json_reports_package_metadata_identity() {
    let registry_path = temp_registry_path("package-identity-registry");
    let root = temp_test_dir("package-identity-root");
    fs::write(root.join("package.json"), r#"{"name":"@example/portal"}"#)
        .expect("write package json");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(service["project"], "portal");
    assert_eq!(service["service"], "portal");
}
#[test]
fn clean_removes_owned_output_files_for_removed_routes() {
    let registry_path = temp_registry_path("clean-output-removed-registry");
    let root = temp_test_dir("clean-output-removed-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"clean-output-removed\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"clean.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
        ),
    )
    .expect("write output config");
    let rendered_path = root.join(".bindport/generated/traefik/web.yml");

    let run_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        run_output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&run_output.stderr)
    );
    assert!(rendered_path.is_file());

    let clean_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["clean", "--stopped"])
        .output()
        .expect("clean stopped entries");

    assert!(
        clean_output.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );
    assert!(!rendered_path.exists());
}
