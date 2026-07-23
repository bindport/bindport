// SPDX-License-Identifier: MIT

use crate::support::*;

fn scoped_identity(root: &Path, project: &str, service: &str) -> ServiceIdentity {
    let root = fs::canonicalize(root).expect("canonical test root");
    resolve_identity(IdentitySources {
        cwd: &root,
        command: &[],
        cli_project: Some(project),
        cli_service: Some(service),
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    })
}

fn reserve_sibling(
    registry_path: &Path,
    identity: &ServiceIdentity,
    port: u16,
    hostname: Option<&str>,
    route_url: Option<&str>,
    health_url: Option<&str>,
) {
    Registry::open(registry_path)
        .expect("registry")
        .record_reserved_lease(&ReserveLease {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port,
            hostname: hostname.map(str::to_string),
            route_url: route_url.map(str::to_string),
            health_url: health_url.map(str::to_string),
        })
        .expect("reserve sibling");
}

fn record_active_sibling(
    registry_path: &Path,
    root: &Path,
    identity: &ServiceIdentity,
    port: u16,
    hostname: &str,
    route_url: &str,
    health_url: &str,
) {
    Registry::open(registry_path)
        .expect("registry")
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port,
            hostname: Some(hostname.to_string()),
            route_url: Some(route_url.to_string()),
            health_url: Some(health_url.to_string()),
            pid: std::process::id(),
            command: current_process_command(),
            cwd: fs::canonicalize(root).expect("canonical root"),
        })
        .expect("record active sibling");
}

fn guarded_loopback_ports(count: usize) -> (Vec<TcpListener>, Vec<u16>) {
    let listeners = (0..count)
        .map(|_| TcpListener::bind(("127.0.0.1", 0)).expect("bind test port"))
        .collect::<Vec<_>>();
    let ports = listeners
        .iter()
        .map(|listener| listener.local_addr().expect("local address").port())
        .collect();
    (listeners, ports)
}

fn free_loopback_ports(count: usize) -> Vec<u16> {
    guarded_loopback_ports(count).1
}

#[test]
fn configured_templates_expand_every_active_and_reserved_sibling_field() {
    let registry_path = temp_registry_path("sibling-fields-registry");
    let root = temp_test_dir("sibling-fields-root")
        .canonicalize()
        .expect("canonical root");
    let (mut port_guards, ports) = guarded_loopback_ports(3);
    let api = scoped_identity(&root, "sibling-fields", "api");
    let database = scoped_identity(&root, "sibling-fields", "database");
    record_active_sibling(
        &registry_path,
        &root,
        &api,
        ports[0],
        "api.localhost",
        "https://api.localhost",
        "https://api.localhost/health",
    );
    reserve_sibling(
        &registry_path,
        &database,
        ports[1],
        Some("database.localhost"),
        Some("https://database.localhost"),
        Some("https://database.localhost/health"),
    );
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "sibling-fields"
default_range = "{consumer_port}-{consumer_port}"
skip_ports = []

[[services]]
name = "consumer"
command = ["sh", "-c", "printf '%s\n%s\n%s|%s|%s' \"$ACTIVE_FIELDS\" \"$RESERVED_FIELDS\" \"$1\" \"$2\" \"$3\"", "sh"]
args = ["{{services.api.port}}", "{{services.database.url}}", "{{services.api.port}}"]
env.ACTIVE_FIELDS = "{{services.api.port}}|{{services.api.host}}|{{services.api.url}}|{{services.api.hostname}}|{{services.api.route_url}}|{{services.api.health_url}}"
env.RESERVED_FIELDS = "{{services.database.port}}|{{services.database.host}}|{{services.database.url}}|{{services.database.hostname}}|{{services.database.route_url}}|{{services.database.health_url}}"
"#,
            consumer_port = ports[2]
        ),
    )
    .expect("write config");

    drop(port_guards.pop().expect("consumer port guard"));
    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "consumer"])
        .output()
        .expect("run consumer");

    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!(
            "{}|127.0.0.1|http://127.0.0.1:{}|api.localhost|https://api.localhost|https://api.localhost/health\n{}|127.0.0.1|http://127.0.0.1:{}|database.localhost|https://database.localhost|https://database.localhost/health\n{}|http://127.0.0.1:{}|{}",
            ports[0], ports[0], ports[1], ports[1], ports[0], ports[1], ports[0]
        )
    );
}

#[test]
fn repeated_sibling_references_remain_one_startup_snapshot_while_child_runs() {
    let registry_path = temp_registry_path("sibling-snapshot-registry");
    let root = temp_test_dir("sibling-snapshot-root")
        .canonicalize()
        .expect("root");
    let ready = root.join("ready");
    let continue_file = root.join("continue");
    let (mut port_guards, ports) = guarded_loopback_ports(3);
    let api = scoped_identity(&root, "sibling-snapshot", "api");
    reserve_sibling(&registry_path, &api, ports[0], None, None, None);
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "sibling-snapshot"
default_range = "{consumer_port}-{consumer_port}"
skip_ports = []

[[services]]
name = "consumer"
command = ["sh", "-c", "printf '%s\n' \"$API_VALUES\"; touch \"$1\"; while [ ! -f \"$2\" ]; do sleep 0.01; done; printf '%s\n' \"$API_VALUES\"", "sh", "{ready}", "{continue_file}"]
env.API_VALUES = "{{services.api.port}}|{{services.api.port}}|{{services.api.url}}"
"#,
            consumer_port = ports[2],
            ready = ready.display(),
            continue_file = continue_file.display(),
        ),
    )
    .expect("write config");

    drop(port_guards.pop().expect("consumer port guard"));
    let mut child = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "consumer"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn consumer");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.exists() {
        if child.try_wait().expect("poll consumer").is_some() {
            let output = child.wait_with_output().expect("collect consumer output");
            panic!(
                "consumer exited before snapshot barrier with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if Instant::now() >= deadline {
            let kill_error = child.kill().err();
            let output = child.wait_with_output().expect("reap timed-out consumer");
            panic!(
                "consumer did not reach snapshot barrier before timeout; status {}; kill error: {kill_error:?}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    Registry::open(&registry_path)
        .expect("registry")
        .release_reserved_identity(&api.identity_key)
        .expect("release old sibling")
        .expect("old sibling reservation");
    reserve_sibling(&registry_path, &api, ports[1], None, None, None);
    fs::write(&continue_file, "continue").expect("release consumer");
    let output = child.wait_with_output().expect("wait for consumer");

    assert!(
        output.status.success(),
        "consumer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = format!(
        "{0}|{0}|http://127.0.0.1:{0}\n{0}|{0}|http://127.0.0.1:{0}\n",
        ports[0]
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
}

#[test]
fn sibling_references_are_isolated_by_project_and_exact_worktree_scope() {
    let registry_path = temp_registry_path("sibling-scope-registry");
    let first_root = temp_test_dir("sibling-scope-first")
        .canonicalize()
        .expect("first root");
    let second_root = temp_test_dir("sibling-scope-second")
        .canonicalize()
        .expect("second root");
    let (mut port_guards, ports) = guarded_loopback_ports(5);
    reserve_sibling(
        &registry_path,
        &scoped_identity(&first_root, "scope-project", "api"),
        ports[0],
        None,
        None,
        None,
    );
    reserve_sibling(
        &registry_path,
        &scoped_identity(&second_root, "scope-project", "api"),
        ports[1],
        None,
        None,
        None,
    );
    reserve_sibling(
        &registry_path,
        &scoped_identity(&first_root, "other-project", "api"),
        ports[2],
        None,
        None,
        None,
    );

    let consumer_guards = port_guards.split_off(3);
    for ((root, consumer_port, expected), consumer_guard) in [
        (&first_root, ports[3], ports[0]),
        (&second_root, ports[4], ports[1]),
    ]
    .into_iter()
    .zip(consumer_guards)
    {
        fs::write(
            root.join(".bindport.toml"),
            format!(
                "project = \"scope-project\"\ndefault_range = \"{consumer_port}-{consumer_port}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\ncommand = [\"sh\", \"-c\", \"printf '%s' \\\"$API_PORT\\\"\"]\nenv.API_PORT = \"{{services.api.port}}\"\n"
            ),
        )
        .expect("write config");
        drop(consumer_guard);
        let output = bindport_with_registry(&registry_path)
            .current_dir(root)
            .args(["run", "consumer"])
            .output()
            .expect("run scoped consumer");
        assert!(
            output.status.success(),
            "run failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected.to_string().as_bytes());
        assert_ne!(output.stdout, ports[2].to_string().as_bytes());
    }
}

#[test]
fn sibling_reference_never_matches_another_project() {
    let registry_path = temp_registry_path("sibling-project-registry");
    let root = temp_test_dir("sibling-project-root")
        .canonicalize()
        .expect("root");
    let ports = free_loopback_ports(2);
    reserve_sibling(
        &registry_path,
        &scoped_identity(&root, "other-project", "api"),
        ports[0],
        None,
        None,
        None,
    );
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"current-project\"\ndefault_range = \"{0}-{0}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.API_PORT = \"{{services.api.port}}\"\n",
            ports[1]
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "consumer", "--", "sh", "-c", "true"])
        .output()
        .expect("run consumer");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no active or reserved service"));
}

#[test]
fn missing_stopped_and_ambiguous_sibling_references_fail_before_spawn() {
    for case in ["missing", "stopped", "ambiguous"] {
        let registry_path = temp_registry_path(&format!("sibling-{case}-registry"));
        let root = temp_test_dir(&format!("sibling-{case}-root"))
            .canonicalize()
            .expect("root");
        let ports = free_loopback_ports(3);
        let api = scoped_identity(&root, "sibling-errors", "api");
        if case != "missing" {
            reserve_sibling(&registry_path, &api, ports[0], None, None, None);
        }
        if case == "stopped" {
            Registry::open(&registry_path)
                .expect("registry")
                .release_reserved_identity(&api.identity_key)
                .expect("release")
                .expect("released");
        } else if case == "ambiguous" {
            reserve_sibling(&registry_path, &api, ports[1], None, None, None);
        }
        fs::write(
            root.join(".bindport.toml"),
            format!(
                "project = \"sibling-errors\"\ndefault_range = \"{0}-{0}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.API_PORT = \"{{services.api.port}}\"\n",
                ports[2]
            ),
        )
        .expect("write config");
        let marker = root.join("spawned");
        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args([
                "run",
                "consumer",
                "--",
                "sh",
                "-c",
                "printf spawned > \"$1\"",
                "sh",
                marker.to_str().expect("marker"),
            ])
            .output()
            .expect("run consumer");

        assert!(!output.status.success(), "{case} reference succeeded");
        assert!(!marker.exists(), "child spawned for {case} reference");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let expected = if case == "ambiguous" {
            "multiple active or reserved services"
        } else {
            "no active or reserved service"
        };
        assert!(
            stderr.contains(expected),
            "unexpected {case} error: {stderr}"
        );
    }
}

#[cfg(unix)]
#[test]
fn stale_sibling_reference_fails_before_spawn() {
    let registry_path = temp_registry_path("sibling-stale-registry");
    let root = temp_test_dir("sibling-stale-root")
        .canonicalize()
        .expect("root");
    let ports = free_loopback_ports(2);
    let api = scoped_identity(&root, "sibling-stale", "api");
    Registry::open(&registry_path)
        .expect("registry")
        .record_run_started(&RunStart {
            project: api.project.clone(),
            service: api.service.clone(),
            identity: Some(api),
            host: String::from("127.0.0.1"),
            port: ports[0],
            hostname: None,
            route_url: None,
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: root.clone(),
        })
        .expect("stale sibling");
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"sibling-stale\"\ndefault_range = \"{0}-{0}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.API_PORT = \"{{services.api.port}}\"\n",
            ports[1]
        ),
    )
    .expect("write config");
    let marker = root.join("spawned");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "consumer",
            "--",
            "sh",
            "-c",
            "touch \"$1\"",
            "sh",
            marker.to_str().expect("marker"),
        ])
        .output()
        .expect("run consumer");

    assert!(!output.status.success());
    assert!(!marker.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no active or reserved service"));
}

#[test]
fn sibling_reference_failure_precedes_blocking_output_preflight() {
    let registry_path = temp_registry_path("sibling-preflight-registry");
    let root = temp_test_dir("sibling-preflight-root")
        .canonicalize()
        .expect("root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"sibling-preflight\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.API_PORT = \"{{services.api.port}}\"\n[[outputs]]\nname = \"broken\"\ntemplate = \"missing-output-template\"\ntarget = \"broken.txt\"\non_failure = \"block\"\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "consumer", "--", "sh", "-c", "true"])
        .output()
        .expect("run consumer");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("failed to resolve sibling service reference"));
    assert!(!stderr.contains("missing-output-template"));
}

#[test]
fn sibling_syntax_is_narrow_and_brace_escaping_remains_compatible() {
    for (case, template) in [
        ("malformed", "{services.api}"),
        ("unknown-field", "{services.api.password}"),
    ] {
        let registry_path = temp_registry_path(&format!("sibling-syntax-{case}-registry"));
        let root = temp_test_dir(&format!("sibling-syntax-{case}-root"));
        let port = free_loopback_port();
        fs::write(
            root.join(".bindport.toml"),
            format!(
                "project = \"sibling-syntax\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.VALUE = \"{template}\"\n"
            ),
        )
        .expect("write config");
        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args(["run", "consumer", "--", "sh", "-c", "true"])
            .output()
            .expect("run consumer");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("unknown or unavailable template placeholder")
        );
    }

    let registry_path = temp_registry_path("sibling-escaped-registry");
    let root = temp_test_dir("sibling-escaped-root");
    let (port_guards, ports) = guarded_loopback_ports(1);
    let port = ports[0];
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"sibling-escaped\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.VALUE = \"{{{{services.api.port}}}}|{{service}}\"\n"
        ),
    )
    .expect("write config");
    drop(port_guards);
    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "consumer",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$VALUE\"",
        ])
        .output()
        .expect("run consumer");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{services.api.port}|consumer");
}

#[test]
fn sibling_references_in_route_metadata_report_the_supported_locations() {
    for (field, value) in [
        ("hostname", "{services.api.port}.localhost"),
        ("route_url", "http://127.0.0.1:{services.api.port}"),
        ("health_url", "http://127.0.0.1:{services.api.port}/health"),
    ] {
        let registry_path = temp_registry_path(&format!("sibling-{field}-registry"));
        let root = temp_test_dir(&format!("sibling-{field}-root"));
        let port = free_loopback_port();
        fs::write(
            root.join(".bindport.toml"),
            format!(
                "project = \"sibling-narrow\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\n{field} = \"{value}\"\n"
            ),
        )
        .expect("write config");
        let marker = root.join("spawned");

        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args([
                "run",
                "consumer",
                "--",
                "sh",
                "-c",
                "touch \"$1\"",
                "sh",
                marker.to_str().expect("marker"),
            ])
            .output()
            .expect("run consumer");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success());
        assert!(!marker.exists());
        assert!(stderr.contains(&format!("is not supported in {field}")));
        assert!(stderr.contains(
            "sibling references are only supported in configured service command, args, and env"
        ));
    }
}

#[test]
fn sibling_references_do_not_expand_in_cli_env_templates() {
    let registry_path = temp_registry_path("sibling-cli-env-registry");
    let root = temp_test_dir("sibling-cli-env-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"sibling-narrow\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "consumer",
            "--env",
            "VALUE={services.api.port}",
            "--",
            "sh",
            "-c",
            "true",
        ])
        .output()
        .expect("run consumer");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unknown or unavailable template placeholder")
    );
}

#[test]
fn absent_optional_sibling_metadata_fails_clearly_before_spawn() {
    let registry_path = temp_registry_path("sibling-optional-registry");
    let root = temp_test_dir("sibling-optional-root");
    let ports = free_loopback_ports(2);
    reserve_sibling(
        &registry_path,
        &scoped_identity(&root, "sibling-optional", "api"),
        ports[0],
        None,
        None,
        None,
    );
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"sibling-optional\"\ndefault_range = \"{0}-{0}\"\nskip_ports = []\n[[services]]\nname = \"consumer\"\nenv.API_HOSTNAME = \"{{services.api.hostname}}\"\n",
            ports[1]
        ),
    )
    .expect("write config");
    let marker = root.join("spawned");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "consumer",
            "--",
            "sh",
            "-c",
            "touch \"$1\"",
            "sh",
            marker.to_str().expect("marker"),
        ])
        .output()
        .expect("run consumer");

    assert!(!output.status.success());
    assert!(!marker.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("has no configured `hostname` value in the startup registry snapshot")
    );
}

fn guarded_two_port_range() -> (Vec<TcpListener>, u16, u16) {
    loop {
        let first = TcpListener::bind(("127.0.0.1", 0)).expect("first port");
        let start = first.local_addr().expect("first address").port();
        let Some(end) = start.checked_add(1) else {
            continue;
        };
        if let Ok(second) = TcpListener::bind(("127.0.0.1", end)) {
            return (vec![first, second], start, end);
        }
    }
}

#[test]
fn reserve_all_supports_out_of_declaration_order_sibling_startup() {
    let registry_path = temp_registry_path("sibling-reserve-all-registry");
    let root = temp_test_dir("sibling-reserve-all-root")
        .canonicalize()
        .expect("root");
    let (range_guards, range_start, range_end) = guarded_two_port_range();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "sibling-reserve-all"
default_range = "{range_start}-{range_end}"
skip_ports = []

[[services]]
name = "api"
command = ["sh", "-c", "printf '%s' \"$PORT\""]

[[services]]
name = "web"
command = ["sh", "-c", "printf '%s' \"$API_PORT\""]
env.API_PORT = "{{services.api.port}}"
"#
        ),
    )
    .expect("write config");

    drop(range_guards);
    let reserve = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve", "--all"])
        .output()
        .expect("reserve all");
    assert!(
        reserve.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&reserve.stderr)
    );
    let status = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["status", "--json"])
        .output()
        .expect("status");
    let status: Value = serde_json::from_slice(&status.stdout).expect("status json");
    assert!(
        status["services"]
            .as_array()
            .expect("services")
            .iter()
            .all(|service| service["state"] == "reserved")
    );
    let api_port_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["port", "api"])
        .output()
        .expect("api port");
    let api_port = String::from_utf8(api_port_output.stdout)
        .expect("port stdout")
        .trim()
        .parse::<u16>()
        .expect("api port");

    let web = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web"])
        .output()
        .expect("run web first");
    assert!(
        web.status.success(),
        "web failed: {}",
        String::from_utf8_lossy(&web.stderr)
    );
    assert_eq!(web.stdout, api_port.to_string().as_bytes());

    let api = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "api"])
        .output()
        .expect("run api second");
    assert!(
        api.status.success(),
        "api failed: {}",
        String::from_utf8_lossy(&api.stderr)
    );
    assert_eq!(api.stdout, api_port.to_string().as_bytes());
}
