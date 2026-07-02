// SPDX-License-Identifier: MIT

mod support;

use support::*;

#[test]
fn dashboard_serves_status_api() {
    let registry_path = temp_registry_path("dashboard-api-registry");
    let mut command = bindport_with_registry(&registry_path);
    let output = command
        .env(BINDPORT_PROJECT_ENV, "dashboard-fixture")
        .args(["run", "web", "--", "sh", "-c", "printf dashboard-fixture"])
        .output()
        .expect("run bindport fixture");

    assert!(output.status.success());

    let dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let response = http_get(dashboard.port, "/api/status");

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let body = http_body(&response);
    let status = serde_json::from_str::<Value>(body).expect("status json");

    assert_eq!(status["schema_version"], "0.4");
    assert!(status["outputs"].as_array().expect("outputs").is_empty());
    assert_eq!(status["services"][0]["project"], "dashboard-fixture");
    assert_eq!(status["services"][0]["service"], "web");
    assert_eq!(
        status["services"][0]["command"],
        "sh -c printf dashboard-fixture"
    );
}
#[test]
fn dashboard_status_api_matches_cli_status_json() {
    let registry_path = temp_registry_path("dashboard-cli-parity-registry");
    let output = bindport_with_registry(&registry_path)
        .env(BINDPORT_PROJECT_ENV, "dashboard-parity-fixture")
        .args([
            "run",
            "web",
            "--hostname",
            "{project}-{service}.localhost",
            "--route-url",
            "https://{hostname}",
            "--",
            "sh",
            "-c",
            "printf dashboard-parity",
        ])
        .output()
        .expect("run bindport fixture");

    assert!(output.status.success());

    let cli_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(
        cli_output.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&cli_output.stderr)
    );

    let cli_status = serde_json::from_slice::<Value>(&cli_output.stdout).expect("status json");
    let dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let response = http_get(dashboard.port, "/api/status");

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let dashboard_status =
        serde_json::from_str::<Value>(http_body(&response)).expect("dashboard status json");

    assert_eq!(
        dashboard_status["schema_version"],
        cli_status["schema_version"]
    );
    assert_eq!(dashboard_status["outputs"], cli_status["outputs"]);
    assert_eq!(dashboard_status["services"], cli_status["services"]);
    assert_eq!(dashboard_status["runs"], cli_status["runs"]);
    assert_eq!(
        cli_status["services"][0]["hostname"],
        "dashboard-parity-fixture-web.localhost"
    );
    assert_eq!(
        cli_status["services"][0]["route_url"],
        "https://dashboard-parity-fixture-web.localhost"
    );
    assert_eq!(cli_status["services"][0]["health"], "unknown");
    assert_eq!(cli_status["services"][0]["proxy"], Value::Null);
    assert!(
        dashboard_status["generated_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        cli_status["generated_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}
#[test]
fn dashboard_cleans_stopped_entries() {
    let registry_path = temp_registry_path("dashboard-clean-registry");
    let output = bindport_with_registry(&registry_path)
        .env(BINDPORT_PROJECT_ENV, "dashboard-clean-fixture")
        .args(["run", "web", "--", "sh", "-c", "printf dashboard-clean"])
        .output()
        .expect("run bindport fixture");

    assert!(output.status.success());

    let dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let clean_response = http_post_clean(dashboard.port, "/api/clean/stopped", None);

    assert!(clean_response.starts_with("HTTP/1.1 200 OK"));

    let report = serde_json::from_str::<Value>(http_body(&clean_response)).expect("clean json");
    assert_eq!(report["leases"], 1);
    assert_eq!(report["runs"], 1);
    assert_eq!(report["states"]["stopped"], 1);

    let status_response = http_get(dashboard.port, "/api/status");
    let status = serde_json::from_str::<Value>(http_body(&status_response)).expect("status json");

    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
}
#[test]
fn dashboard_clean_removes_owned_output_files_for_removed_routes() {
    let registry_path = temp_registry_path("dashboard-clean-output-registry");
    let root = temp_test_dir("dashboard-clean-output-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"dashboard-clean-output\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"dashboard-clean.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
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

    let mut command = bindport_with_registry(&registry_path);
    command.current_dir(&root);
    let dashboard = start_dashboard(command);
    let clean_response = http_post_clean(dashboard.port, "/api/clean/stopped", None);

    assert!(clean_response.starts_with("HTTP/1.1 200 OK"));
    assert!(!rendered_path.exists());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status after dashboard clean");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["outputs"][0]["name"], "traefik");
    assert_eq!(status["outputs"][0]["removed"], 1);
}
#[test]
fn dashboard_can_register_itself_as_a_service() {
    let registry_path = temp_registry_path("dashboard-register-service-registry");
    let port = free_loopback_port();
    let dashboard = start_dashboard_with_args(
        bindport_with_registry(&registry_path),
        &[
            "dashboard",
            "serve",
            "--port",
            &port.to_string(),
            "--register-service",
        ],
    );
    let response = http_get(dashboard.port, "/api/status");

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let body = http_body(&response);
    let status = serde_json::from_str::<Value>(body).expect("status json");
    let services = status["services"].as_array().expect("services");
    let dashboard_service = services
        .iter()
        .find(|service| service["project"] == SERVICE_NAME && service["service"] == "dashboard")
        .expect("dashboard service registration");

    assert_eq!(dashboard_service["state"], "active");
    assert_eq!(dashboard_service["host"], "127.0.0.1");
    assert_eq!(dashboard_service["port"], u64::from(port));
    assert_eq!(
        dashboard_service["route_url"],
        format!("http://127.0.0.1:{port}")
    );
    assert_eq!(dashboard_service["health"], "unknown");
    assert_eq!(dashboard_service["proxy"], Value::Null);
}
#[test]
fn dashboard_registers_service_from_config() {
    let registry_path = temp_registry_path("dashboard-register-config-registry");
    let root = temp_test_dir("dashboard-register-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "[dashboard]\nregister_service = true\n",
    )
    .expect("write dashboard config");

    let port = free_loopback_port();
    let mut command = bindport_with_registry(&registry_path);
    command.current_dir(&root);
    let dashboard = start_dashboard_with_args(
        command,
        &["dashboard", "serve", "--port", &port.to_string()],
    );
    let response = http_get(dashboard.port, "/api/status");

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let status = serde_json::from_str::<Value>(http_body(&response)).expect("status json");
    let services = status["services"].as_array().expect("services");

    assert!(
        services
            .iter()
            .any(|service| service["project"] == SERVICE_NAME
                && service["service"] == "dashboard"
                && service["state"] == "active")
    );
}
#[test]
fn dashboard_no_register_service_overrides_config_registration() {
    let registry_path = temp_registry_path("dashboard-register-override-registry");
    let root = temp_test_dir("dashboard-register-override-root");
    fs::write(
        root.join(".bindport.toml"),
        "[dashboard]\nregister_service = true\n",
    )
    .expect("write dashboard config");

    let port = free_loopback_port();
    let mut command = bindport_with_registry(&registry_path);
    command.current_dir(&root);
    let dashboard = start_dashboard_with_args(
        command,
        &[
            "dashboard",
            "serve",
            "--port",
            &port.to_string(),
            "--no-register-service",
        ],
    );
    let response = http_get(dashboard.port, "/api/status");

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let status = serde_json::from_str::<Value>(http_body(&response)).expect("status json");

    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
}
#[test]
fn dashboard_registration_redacts_literal_token_from_command() {
    let registry_path = temp_registry_path("dashboard-register-token-registry");
    let port = free_loopback_port();
    let secret = "secret-in-registry";
    let dashboard = start_dashboard_with_args(
        bindport_with_registry(&registry_path),
        &[
            "dashboard",
            "serve",
            "--port",
            &port.to_string(),
            "--register-service",
            "--auth",
            "required",
            "--token",
            secret,
        ],
    );
    let response = http_get_with_auth(dashboard.port, "/api/status", &format!("Bearer {secret}"));

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let body = http_body(&response);
    let status = serde_json::from_str::<Value>(body).expect("status json");
    let services = status["services"].as_array().expect("services");
    let dashboard_service = services
        .iter()
        .find(|service| service["project"] == SERVICE_NAME && service["service"] == "dashboard")
        .expect("dashboard service registration");
    let command = dashboard_service["command"].as_str().expect("command");

    assert!(
        command.contains("--token ***"),
        "unexpected command: {command}"
    );
    assert!(
        !command.contains(secret),
        "dashboard token leaked: {command}"
    );
}
#[test]
fn dashboard_status_api_handles_100_services() {
    let registry_path = temp_registry_path("dashboard-100-services-registry");
    let mut registry = Registry::open(&registry_path).expect("registry");
    for index in 0..100 {
        let service = format!("service-{index:03}");
        let identity = ServiceIdentity {
            project: String::from("bulk-project"),
            service: service.clone(),
            git: None,
            identity_key: format!("v1:bulk-{index:03}"),
        };
        registry
            .record_run_started(&RunStart {
                project: identity.project.clone(),
                service,
                identity: Some(identity),
                host: String::from("127.0.0.1"),
                port: 29_100 + index,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("bulk fixture"),
                cwd: PathBuf::from("/tmp/bindport-bulk-fixture"),
            })
            .expect("record bulk service");
    }
    drop(registry);

    let port = free_loopback_port();
    let dashboard = start_dashboard_with_args(
        bindport_with_registry(&registry_path),
        &["dashboard", "serve", "--port", &port.to_string()],
    );
    let response = http_get(dashboard.port, "/api/status");

    assert!(response.starts_with("HTTP/1.1 200 OK"));

    let status = serde_json::from_str::<Value>(http_body(&response)).expect("status json");
    assert_eq!(status["services"].as_array().expect("services").len(), 100);
}
#[test]
fn dashboard_uses_cli_port_option() {
    let registry_path = temp_registry_path("dashboard-cli-port-registry");
    let port = free_loopback_port();
    let dashboard = start_dashboard_with_args(
        bindport_with_registry(&registry_path),
        &["dashboard", "serve", "--port", &port.to_string()],
    );
    let response = http_get(dashboard.port, "/healthz");

    assert_eq!(dashboard.port, port);
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}
#[test]
fn dashboard_rejects_non_loopback_host_without_auth() {
    let registry_path = temp_registry_path("dashboard-host-auth-registry");
    let output = bindport_with_registry(&registry_path)
        .args(["dashboard", "serve", "--host", "0.0.0.0"])
        .output()
        .expect("run dashboard serve");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires auth"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn dashboard_requires_bearer_token_when_auth_is_enabled() {
    let registry_path = temp_registry_path("dashboard-token-registry");
    let port = free_loopback_port();
    let dashboard = start_dashboard_with_args(
        bindport_with_registry(&registry_path),
        &[
            "dashboard",
            "serve",
            "--port",
            &port.to_string(),
            "--auth",
            "required",
            "--token",
            "secret",
        ],
    );
    let rejected = http_get(dashboard.port, "/api/status");
    let accepted = http_get_with_auth(dashboard.port, "/api/status", "Bearer secret");
    let clean_rejected = http_post_clean(dashboard.port, "/api/clean/stopped", None);
    let clean_accepted =
        http_post_clean(dashboard.port, "/api/clean/stopped", Some("Bearer secret"));

    assert!(rejected.starts_with("HTTP/1.1 401 Unauthorized"));
    assert!(accepted.starts_with("HTTP/1.1 200 OK"));
    assert!(clean_rejected.starts_with("HTTP/1.1 401 Unauthorized"));
    assert!(clean_accepted.starts_with("HTTP/1.1 200 OK"));
}
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
#[test]
fn dashboard_rejects_untrusted_host_header() {
    let registry_path = temp_registry_path("dashboard-host-rejection-registry");
    let dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let response = http_get_with_host(dashboard.port, "/api/status", "example.test");

    assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
    assert_eq!(http_body(&response), "forbidden\n");
}
#[test]
fn dashboard_returns_not_found_for_unknown_route() {
    let registry_path = temp_registry_path("dashboard-not-found-registry");
    let dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let response = http_get(dashboard.port, "/missing");

    assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    assert_eq!(http_body(&response), "not found\n");
}
#[test]
fn dashboard_falls_back_when_preferred_port_is_busy() {
    let busy_preferred = TcpListener::bind(("127.0.0.1", 0)).expect("bind busy dashboard port");
    let preferred_port = busy_preferred
        .local_addr()
        .expect("busy dashboard port")
        .port();
    let fallback_port = free_loopback_port();
    let registry_path = temp_registry_path("dashboard-fallback-registry");
    let root = temp_test_dir("dashboard-fallback-root");
    fs::write(
        root.join(".bindport.toml"),
        format!("default_range = \"{fallback_port}-{fallback_port}\"\nskip_ports = []\n"),
    )
    .expect("write dashboard fallback config");

    let mut command = bindport_with_registry(&registry_path);
    command.current_dir(&root);
    let preferred_port_arg = preferred_port.to_string();
    let dashboard = start_dashboard_with_args(
        command,
        &["dashboard", "serve", "--port", &preferred_port_arg],
    );

    assert_eq!(dashboard.port, fallback_port);
    assert_ne!(dashboard.port, preferred_port);

    drop(busy_preferred);
}
#[test]
fn dashboard_survives_dropped_connection() {
    let registry_path = temp_registry_path("dashboard-dropped-connection-registry");
    let mut dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let stream = TcpStream::connect(("127.0.0.1", dashboard.port)).expect("connect dashboard");
    drop(stream);
    thread::sleep(Duration::from_millis(50));

    assert!(
        dashboard
            .child
            .try_wait()
            .expect("poll dashboard")
            .is_none(),
        "dashboard exited after a dropped connection"
    );

    let response = http_get(dashboard.port, "/healthz");

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(http_body(&response), "ok\n");
}
