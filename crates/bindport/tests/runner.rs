// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bindport_core::{
    BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS,
    FALLBACK_CONFIG_FILE, SERVICE_NAME, ServiceIdentity,
};
use bindport_registry::{REGISTRY_PATH_ENV, Registry, RunStart};
use serde_json::Value;

fn bindport() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bindport"))
}

fn bindport_with_registry(registry_path: &Path) -> Command {
    let mut command = bindport();
    command.env(REGISTRY_PATH_ENV, registry_path);
    command.env("XDG_CONFIG_HOME", config_home_for_registry(registry_path));
    command.env("XDG_STATE_HOME", state_home_for_registry(registry_path));
    command.env_remove(BINDPORT_PROJECT_ENV);
    command.env_remove(BINDPORT_SERVICE_ENV);
    command
}

fn bindport_without_registry_path() -> Command {
    let mut command = bindport();
    command.env_remove(REGISTRY_PATH_ENV);
    command.env_remove("XDG_CONFIG_HOME");
    command.env_remove("XDG_STATE_HOME");
    command.env_remove("HOME");
    command.env_remove("APPDATA");
    command
}

fn config_home_for_registry(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("config-home")
}

fn state_home_for_registry(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("state-home")
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: libc::c_int) {
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
    assert_eq!(result, 0, "send signal to process {pid}");
}

#[cfg(unix)]
fn terminate_process_from_file(path: &Path) {
    let Ok(pid) = fs::read_to_string(path) else {
        return;
    };
    let Ok(pid) = pid.trim().parse::<libc::pid_t>() else {
        return;
    };

    let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents).expect("write executable fixture");
    let mut permissions = fs::metadata(path)
        .expect("executable fixture metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mark executable fixture");
}

#[cfg(unix)]
fn prepend_path(path: &Path) -> String {
    let existing_path = std::env::var_os("PATH").unwrap_or_default();

    format!("{}:{}", path.display(), existing_path.to_string_lossy())
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(status) = child.try_wait().expect("poll child status") {
            return Some(status);
        }

        if Instant::now() >= deadline {
            return None;
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_file_contains(path: &Path, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && contents.contains(needle)
        {
            return contents;
        }

        if Instant::now() >= deadline {
            panic!(
                "{} did not contain `{needle}` within {timeout:?}",
                path.display()
            );
        }

        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn dash_dash_runs_child_with_assigned_port() {
    let registry_path = temp_registry_path("dash-dash");
    let output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let port = stdout.parse::<u16>().expect("stdout is a port number");

    assert!(DEFAULT_PORT_RANGE.contains(port));
    assert!(!DEFAULT_SKIP_PORTS.contains(&port));
}

#[cfg(unix)]
#[test]
fn package_script_runs_bindport_next_dev_flow() {
    let registry_path = temp_registry_path("package-script-registry");
    let root = temp_test_dir("package-script-root");
    let bindport_bin_dir = root.join(".test-bin");
    let next_bin_dir = root.join("node_modules").join(".bin");

    fs::create_dir_all(&bindport_bin_dir).expect("bindport bin dir");
    fs::create_dir_all(&next_bin_dir).expect("next bin dir");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_bindport"),
        bindport_bin_dir.join("bindport"),
    )
    .expect("link bindport binary");
    write_executable(
        &next_bin_dir.join("next"),
        "#!/bin/sh\nif [ \"$1\" != \"dev\" ]; then echo \"unexpected next args: $*\" >&2; exit 64; fi\nprintf 'next-dev-port=%s\\n' \"$PORT\"\n",
    );
    fs::write(
        root.join("package.json"),
        r#"{"name":"bindport-package-script-fixture","private":true,"scripts":{"dev":"bindport -- next dev"}}"#,
    )
    .expect("write package json");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"package-script-fixture\"\nservice = \"web\"\ndefault_range = \"29420-29421\"\nskip_ports = []\n",
    )
    .expect("write config");

    let output = Command::new("npm")
        .current_dir(&root)
        .env(REGISTRY_PATH_ENV, &registry_path)
        .env_remove(BINDPORT_PROJECT_ENV)
        .env_remove(BINDPORT_SERVICE_ENV)
        .env("PATH", prepend_path(&bindport_bin_dir))
        .env("NO_UPDATE_NOTIFIER", "1")
        .env("NPM_CONFIG_AUDIT", "false")
        .env("NPM_CONFIG_FUND", "false")
        .args(["run", "--silent", "dev"])
        .output()
        .expect("run package script");

    assert!(
        output.status.success(),
        "package script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let port = stdout
        .trim()
        .strip_prefix("next-dev-port=")
        .expect("next dev port marker")
        .parse::<u16>()
        .expect("port");

    assert!(matches!(port, 29_420 | 29_421));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "package-script-fixture");
    assert_eq!(status["services"][0]["service"], "web");
    assert_eq!(status["services"][0]["command"], "next dev");
    assert_eq!(status["services"][0]["hostname"], Value::Null);
    assert_eq!(status["services"][0]["route_url"], Value::Null);
    assert_eq!(status["services"][0]["proxy"], Value::Null);
    assert_eq!(status["services"][0]["exit_code"], 0);
    assert_eq!(
        status["services"][0]["port"]
            .as_u64()
            .expect("service port"),
        u64::from(port)
    );
    assert_eq!(status["runs"][0]["exit_code"], 0);
}

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

#[test]
fn runner_preserves_child_exit_code() {
    let registry_path = temp_registry_path("exit-code");
    let status = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "exit 37"])
        .status()
        .expect("run bindport");

    assert_eq!(status.code(), Some(37));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["exit_code"], 37);
}

#[test]
fn run_subcommand_accepts_dash_dash_separator() {
    let registry_path = temp_registry_path("run-subcommand");
    let output = bindport_with_registry(&registry_path)
        .args(["run", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}

#[test]
fn configured_service_command_expands_port_arguments() {
    let registry_path = temp_registry_path("configured-command-registry");
    let root = temp_test_dir("configured-command-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "storybook-project"
default_range = "{port}-{port}"
skip_ports = []

[[services]]
name = "storybook"
command = ["sh", "-c", "printf '%s|%s|%s' \"$PORT\" \"$1\" \"$2\"", "sh"]
args = ["--port", "{{port}}"]
"#
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "storybook"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!("{port}|--port|{port}")
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["service"], "storybook");
    assert!(
        status["services"][0]["command"]
            .as_str()
            .expect("command")
            .ends_with(&format!("--port {port}"))
    );
}

#[test]
fn explicit_child_command_overrides_configured_service_command() {
    let registry_path = temp_registry_path("configured-command-override-registry");
    let root = temp_test_dir("configured-command-override-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"override-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\ncommand = [\"sh\", \"-c\", \"exit 99\"]\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, port.to_string().as_bytes());
}

#[test]
fn command_surface_reports_invalid_arguments() {
    let registry_path = temp_registry_path("invalid-command-surface-registry");
    let cases: &[(&[&str], &str)] = &[
        (&["unknown"], "unknown bindport command: unknown"),
        (&["run"], "no command provided after `--`"),
        (&["run", "--"], "no command provided after `--`"),
        (
            &["run", "web", "api", "--", "true"],
            "only one service name can be provided",
        ),
        (
            &["run", "--unknown", "--", "true"],
            "unknown run option `--unknown`",
        ),
        (
            &["run", "--env", "PORT", "--", "true"],
            "invalid env assignment `PORT`; expected NAME=VALUE",
        ),
        (
            &["run", "--env", "1PORT=3000", "--", "true"],
            "invalid env variable name `1PORT`",
        ),
        (
            &["run", "--hostname", "--", "true"],
            "--hostname requires a value",
        ),
        (
            &["run", "--route-url", "--", "true"],
            "--route-url requires a value",
        ),
        (
            &["config", "explain", "extra"],
            "config explain does not take arguments",
        ),
        (&["config", "missing"], "unknown config command `missing`"),
        (
            &["doctor", "outputs", "extra"],
            "doctor outputs does not take arguments",
        ),
        (&["doctor", "missing"], "unknown doctor command `missing`"),
    ];

    for (args, expected_error) in cases {
        let output = bindport_with_registry(&registry_path)
            .args(*args)
            .output()
            .expect("run bindport");

        assert!(
            !output.status.success(),
            "expected failure for args {args:?}"
        );
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        assert!(
            stderr.contains(expected_error),
            "stderr for args {args:?} did not contain `{expected_error}`:\n{stderr}"
        );
    }
}

#[test]
fn run_subcommand_service_argument_overrides_env_and_config() {
    let registry_path = temp_registry_path("identity-precedence");
    let root = temp_test_dir("identity-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"config-project\"\nservice = \"config-service\"\ndefault_range = \"29120-29120\"\n",
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(BINDPORT_PROJECT_ENV, "env-project")
        .env(BINDPORT_SERVICE_ENV, "env-service")
        .args([
            "run",
            "cli-service",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PORT\"",
        ])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29120");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "env-project");
    assert_eq!(status["services"][0]["service"], "cli-service");
}

#[test]
fn service_config_injects_env_templates_and_route_metadata() {
    let registry_path = temp_registry_path("service-env-registry");
    let root = temp_test_dir("service-env-root");
    let port = free_loopback_port();
    init_git_repo(&root, "feature/tree");
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hoststamp\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"{{branch}}.{{project}}.localhost\"\nhealth_url = \"{{route_url}}/health\"\nenv.BINDPORT_ASSIGNED_PORT = \"{{port}}\"\nenv.BINDPORT_ROUTE = \"{{route_url}}\"\nenv.BINDPORT_HEALTH = \"{{health_url}}\"\nenv.BINDPORT_DIRECT_URL = \"{{url}}\"\nenv.HOSTNAME = \"0.0.0.0\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s|%s' \"$BINDPORT_ASSIGNED_PORT\" \"$BINDPORT_ROUTE\" \"$BINDPORT_HEALTH\" \"$BINDPORT_DIRECT_URL\" \"$HOSTNAME\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!(
            "{port}|http://feature-tree.hoststamp.localhost|http://feature-tree.hoststamp.localhost/health|http://127.0.0.1:{port}|0.0.0.0"
        )
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(service["project"], "hoststamp");
    assert_eq!(service["service"], "web");
    assert_eq!(service["hostname"], "feature-tree.hoststamp.localhost");
    assert_eq!(
        service["route_url"],
        "http://feature-tree.hoststamp.localhost"
    );
    assert_eq!(
        service["health_url"],
        "http://feature-tree.hoststamp.localhost/health"
    );
    assert_eq!(service["port"], port);
}

#[test]
fn run_cli_templates_override_service_config() {
    let registry_path = temp_registry_path("cli-template-registry");
    let root = temp_test_dir("cli-template-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"template-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"config.{{project}}.localhost\"\nenv.NEXT_PUBLIC_BINDPORT_URL = \"config\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--hostname",
            "cli-{service}.localhost",
            "--env",
            "NEXT_PUBLIC_BINDPORT_URL={route_url}",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$NEXT_PUBLIC_BINDPORT_URL\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"http://cli-web.localhost");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["hostname"], "cli-web.localhost");
    assert_eq!(
        status["services"][0]["route_url"],
        "http://cli-web.localhost"
    );
}

#[test]
fn run_templates_reject_unknown_placeholders() {
    let registry_path = temp_registry_path("template-error-registry");
    let output = bindport_with_registry(&registry_path)
        .args([
            "run",
            "web",
            "--env",
            "NEXT_PUBLIC_BINDPORT_URL={missing}",
            "--",
            "sh",
            "-c",
            "true",
        ])
        .output()
        .expect("run bindport");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unknown or unavailable template placeholder `missing`"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_templates_escape_literal_braces() {
    let registry_path = temp_registry_path("template-escape-registry");
    let output = bindport_with_registry(&registry_path)
        .args([
            "run",
            "web",
            "--env",
            r#"APP_CONFIG={{"api":"{service}"}}"#,
            "--",
            "sh",
            "-c",
            "printf '%s' \"$APP_CONFIG\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, br#"{"api":"web"}"#);
}

#[test]
fn wrapped_command_flags_are_passed_to_child() {
    let registry_path = temp_registry_path("flags");
    let output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$1\"", "sh", "--version"])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"--version");
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
    assert_eq!(status["schema_version"], "0.4");
    assert_eq!(status["outputs"].as_array().expect("outputs").len(), 0);
    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
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
fn runner_reuses_previous_identity_port_when_available() {
    let registry_path = temp_registry_path("sticky-registry");
    let root = temp_test_dir("sticky-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29300-29301\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    let second_port = run_print_port(&registry_path, &root);

    assert_eq!(second_port, first_port);
}

#[test]
fn runner_falls_back_when_previous_identity_port_is_active() {
    let registry_path = temp_registry_path("sticky-occupied-registry");
    let root = temp_test_dir("sticky-occupied-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29310-29311\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    reserve_registry_port(&registry_path, first_port);
    let second_port = run_print_port(&registry_path, &root);

    assert_ne!(second_port, first_port);
    assert!(matches!(second_port, 29_310 | 29_311));
}

#[cfg(unix)]
#[test]
fn runner_retries_once_when_assigned_port_is_claimed_after_spawn() {
    let registry_path = temp_registry_path("allocation-retry-registry");
    let root = temp_test_dir("allocation-retry-root");
    let marker_path = temp_path("allocation-retry-marker");
    let pid_path = temp_path("allocation-retry-pid");
    let marker_arg = marker_path.display().to_string();
    let pid_arg = pid_path.display().to_string();

    fs::write(
        root.join(".bindport.toml"),
        "project = \"retry-project\"\nservice = \"web\"\ndefault_range = \"29400-29401\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
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
            &marker_arg,
            &pid_arg,
        ])
        .output()
        .expect("run bindport");

    terminate_process_from_file(&pid_path);

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let first_port = fs::read_to_string(&marker_path)
        .expect("first port marker")
        .parse::<u16>()
        .expect("first port");
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let second_port = stdout.parse::<u16>().expect("second port");
    let stderr = String::from_utf8(output.stderr).expect("stderr");

    assert_ne!(second_port, first_port);
    assert!(matches!(first_port, 29_400 | 29_401));
    assert!(matches!(second_port, 29_400 | 29_401));
    assert!(stderr.contains("retrying with another port"));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let runs = status["runs"].as_array().expect("runs");
    let mut exit_codes = runs
        .iter()
        .map(|run| run["exit_code"].as_i64().expect("exit code"))
        .collect::<Vec<_>>();
    exit_codes.sort_unstable();

    assert_eq!(runs.len(), 2);
    assert_eq!(exit_codes, [0, 98]);
    assert_eq!(status["services"][0]["exit_code"], 0);
    assert_eq!(
        status["services"][0]["port"]
            .as_u64()
            .expect("service port"),
        u64::from(second_port)
    );
}

#[test]
fn runner_continues_when_registry_path_is_unavailable() {
    let output = bindport_without_registry_path()
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let port = stdout.parse::<u16>().expect("stdout is a port number");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");

    assert!(DEFAULT_PORT_RANGE.contains(port));
    assert!(stderr.contains("running without registry recording"));
}

#[test]
fn parent_project_config_sets_port_range_and_project() {
    let registry_path = temp_registry_path("project-config-registry");
    let root = temp_test_dir("project-config-root");
    let nested = root.join("packages").join("web");
    fs::create_dir_all(&nested).expect("nested dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"configured-project\"\ndefault_range = \"29100-29101\"\nskip_ports = [29100]\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&nested)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29101");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "configured-project");
}

#[test]
fn service_path_config_infers_service_from_current_directory() {
    let registry_path = temp_registry_path("service-path-registry");
    let root = temp_test_dir("service-path-root");
    let api_src = root.join("apps").join("api").join("src");
    fs::create_dir_all(&api_src).expect("api source dir");
    fs::create_dir_all(root.join("apps").join("web")).expect("web dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"monorepo\"\ndefault_range = \"29104-29104\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\nhostname = \"web.{project}.localhost\"\nenv.BINDPORT_SELECTED_SERVICE = \"{service}\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\nhostname = \"{service}.{project}.localhost\"\nenv.BINDPORT_SELECTED_SERVICE = \"{service}\"\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&api_src)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s' \"$PORT\" \"$BINDPORT_SELECTED_SERVICE\"",
        ])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29104|api");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "monorepo");
    assert_eq!(status["services"][0]["service"], "api");
    assert_eq!(status["services"][0]["hostname"], "api.monorepo.localhost");
    assert_eq!(
        status["services"][0]["route_url"],
        "http://api.monorepo.localhost"
    );
}

#[test]
fn config_explain_reports_field_and_identity_sources() {
    let registry_path = temp_registry_path("config-explain-registry");
    let root = temp_test_dir("config-explain-root");
    let api_src = root.join("apps").join("api").join("src");
    fs::create_dir_all(&api_src).expect("api source dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"base-project\"\ndefault_range = \"29105-29106\"\nskip_ports = [29105]\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
    )
    .expect("write base config");
    fs::write(
        root.join(".bindport.local.toml"),
        "project = \"local-project\"\ndefault_range = \"29107-29107\"\n",
    )
    .expect("write local config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&api_src)
        .args(["config", "explain"])
        .output()
        .expect("run bindport config explain");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("config explain stdout");

    assert!(stdout.contains("BindPort config explain"));
    assert!(stdout.contains("config:"));
    assert!(stdout.contains(".bindport.toml"));
    assert!(stdout.contains("config local override:"));
    assert!(stdout.contains(".bindport.local.toml"));
    assert!(stdout.contains("project: local-project (local override config)"));
    assert!(stdout.contains("default_range: 29107-29107 (local override config)"));
    assert!(stdout.contains("skip_ports: 1 ports (project config)"));
    assert!(stdout.contains("services: 2 entries (project config)"));
    assert!(stdout.contains("project: local-project (local override config `project`)"));
    assert!(stdout.contains("service: api (project config `[[services]].path`)"));
}

#[test]
fn config_validate_reports_ok_for_valid_config() {
    let registry_path = temp_registry_path("config-validate-ok-registry");
    let root = temp_test_dir("config-validate-ok-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"valid-project\"\ndefault_range = \"29108-29109\"\nskip_ports = [29108]\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.service }}.yml\"\n",
    )
    .expect("write valid config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("run bindport config validate");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("config validate stdout");

    assert!(stdout.contains("BindPort config validate"));
    assert!(stdout.contains("validation: ok"));
}

#[test]
fn config_validate_reports_actionable_errors() {
    let registry_path = temp_registry_path("config-validate-error-registry");
    let root = temp_test_dir("config-validate-error-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"invalid-project\"\n[[services]]\npath = \"../api\"\n[[services]]\nname = \"web\"\npath = \"/tmp/web\"\n[[services]]\nname = \"web\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\n[[outputs]]\nname = \"debug\"\ntarget = \"debug/{{ route.service }}.txt\"\n",
    )
    .expect("write invalid config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("run bindport config validate");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("config validate stdout");

    assert!(stdout.contains("BindPort config validate"));
    assert!(stdout.contains("validation: 6 errors"));
    assert!(stdout.contains("error: services[0].name: service name is required"));
    assert!(stdout.contains("error: services[0].path: service path must be relative"));
    assert!(stdout.contains("error: services[1].path: service path must be relative"));
    assert!(stdout.contains("error: services[2].name: duplicate service name `web`"));
    assert!(
        stdout.contains("error: outputs[0].target: output `traefik` is missing required `target`")
    );
    assert!(
        stdout
            .contains("error: outputs[1].template: output `debug` is missing required `template`")
    );
}

#[test]
fn checked_in_monorepo_example_resolves_services() {
    let registry_path = temp_registry_path("monorepo-example-registry");
    let root = workspace_root().join("examples").join("monorepo");

    let validate = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("validate monorepo example");
    assert!(
        validate.status.success(),
        "config validate failed: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
    let validate_stdout = String::from_utf8(validate.stdout).expect("validate stdout");
    assert!(validate_stdout.contains("validation: ok"));

    for (path, service) in [("apps/web", "web"), ("apps/api", "api")] {
        let explain = bindport_with_registry(&registry_path)
            .current_dir(root.join(path))
            .args(["config", "explain"])
            .output()
            .expect("explain monorepo service");
        assert!(
            explain.status.success(),
            "config explain failed for {path}: {}",
            String::from_utf8_lossy(&explain.stderr)
        );
        let stdout = String::from_utf8(explain.stdout).expect("explain stdout");
        assert!(stdout.contains("project: orderful (project config `project`)"));
        assert!(stdout.contains(&format!(
            "service: {service} (project config `[[services]].path`)"
        )));
    }
}

#[test]
fn local_project_config_overrides_base_project_config() {
    let registry_path = temp_registry_path("local-project-config-registry");
    let root = temp_test_dir("local-project-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"base-project\"\ndefault_range = \"29102-29102\"\nskip_ports = []\n",
    )
    .expect("write base config");
    fs::write(
        root.join(".bindport.local.toml"),
        "project = \"local-project\"\ndefault_range = \"29103-29103\"\n",
    )
    .expect("write local config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29103");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "local-project");

    let doctor_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");
    let stdout = String::from_utf8(doctor_output.stdout).expect("doctor stdout");

    assert!(stdout.contains("config local override:"));
    assert!(stdout.contains(".bindport.local.toml"));
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
    fs::write(root.join("package.json"), r#"{"name":"@stutz/hoststamp"}"#)
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

    assert_eq!(service["project"], "hoststamp");
    assert_eq!(service["service"], "hoststamp");
}

#[test]
fn toml_config_wins_over_json_in_same_directory() {
    let registry_path = temp_registry_path("config-precedence-registry");
    let root = temp_test_dir("config-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "default_range = \"29110-29110\"\n",
    )
    .expect("write toml config");
    fs::write(
        root.join(".bindport.json"),
        r#"{"default_range":"29111-29111"}"#,
    )
    .expect("write json config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29110");
}

#[test]
fn fallback_config_from_config_home_is_used_when_no_project_config_exists() {
    let state_dir = temp_test_dir("fallback-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = config_home_for_registry(&registry_path)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);
    let cwd = temp_test_dir("fallback-config-cwd");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("config dir");
    fs::write(&config_path, "default_range = \"29200-29200\"\n").expect("write fallback config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29200");
}

#[test]
fn templates_export_builtin_traefik_template() {
    let registry_path = temp_registry_path("templates-builtin-registry");
    let output = bindport_with_registry(&registry_path)
        .args(["templates", "export", "bindport-traefik"])
        .output()
        .expect("export built-in template");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("Generated by BindPort"));
    assert!(stdout.contains("Host(`{{ route.hostname }}`)"));
}

#[test]
fn project_template_shadows_builtin_template() {
    let registry_path = temp_registry_path("templates-project-registry");
    let root = temp_test_dir("templates-project-root");
    let template_dir = root.join(".bindport").join("templates");
    fs::create_dir_all(&template_dir).expect("template dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"template-project\"\n",
    )
    .expect("project config");
    fs::write(
        template_dir.join("bindport-traefik.yaml.j2"),
        "project template\n",
    )
    .expect("project template");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["templates", "export", "bindport-traefik"])
        .output()
        .expect("export project template");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"project template\n");
}

#[test]
fn templates_source_option_bypasses_project_shadow() {
    let registry_path = temp_registry_path("templates-source-registry");
    let root = temp_test_dir("templates-source-root");
    let template_dir = root.join(".bindport").join("templates");
    fs::create_dir_all(&template_dir).expect("template dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"template-project\"\n",
    )
    .expect("project config");
    fs::write(template_dir.join("bindport-traefik"), "project template\n")
        .expect("project template");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "templates",
            "show",
            "--source",
            "built-in",
            "bindport-traefik",
        ])
        .output()
        .expect("show built-in template");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("source: built-in"));
    assert!(stdout.contains("Generated by BindPort"));
    assert!(!stdout.contains("project template"));
}

#[test]
fn render_command_writes_config_files_and_records_ownership() {
    let registry_path = temp_registry_path("render-output-registry");
    let root = temp_test_dir("render-output-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"render-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"render.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n"
        ),
    )
    .expect("write render config");

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

    let rendered_path = root
        .join(".bindport")
        .join("generated")
        .join("traefik")
        .join("web.yml");
    let dry_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "traefik", "--dry-run"])
        .output()
        .expect("dry-run render");

    assert!(
        dry_run.status.success(),
        "render dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let dry_run_stdout = String::from_utf8(dry_run.stdout).expect("dry-run stdout");
    assert!(dry_run_stdout.contains("would render traefik: 1 files"));
    assert!(dry_run_stdout.contains("traefik/web.yml"));
    assert!(!rendered_path.exists());

    let render = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "traefik"])
        .output()
        .expect("render output");

    assert!(
        render.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&render.stderr)
    );
    assert!(
        String::from_utf8(render.stdout)
            .expect("render stdout")
            .contains("rendered traefik: 1 files")
    );

    let contents = fs::read_to_string(&rendered_path).expect("rendered file");
    assert!(contents.contains("Generated by BindPort"));
    assert!(contents.contains("is stopped"));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    assert!(
        status_output.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let rendered_path_string = rendered_path
        .canonicalize()
        .expect("canonical rendered path")
        .display()
        .to_string();

    assert_eq!(status["schema_version"], "0.4");
    assert_eq!(status["outputs"][0]["name"], "traefik");
    assert_eq!(status["outputs"][0]["pending"], 0);
    assert_eq!(status["outputs"][0]["rendered"], 1);
    assert_eq!(status["outputs"][0]["removed"], 0);
    assert_eq!(status["outputs"][0]["error"], 0);
    assert_eq!(status["services"][0]["outputs"][0]["name"], "traefik");
    assert_eq!(status["services"][0]["outputs"][0]["status"], "rendered");
    assert_eq!(
        status["services"][0]["outputs"][0]["path"],
        rendered_path_string
    );
    assert_eq!(status["services"][0]["proxy"]["adapter"], "traefik");
    assert_eq!(status["services"][0]["proxy"]["rendered"], true);
    assert_eq!(
        status["services"][0]["proxy"]["target"],
        rendered_path_string
    );

    let rerender = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "traefik"])
        .output()
        .expect("rerender output");

    assert!(
        rerender.status.success(),
        "rerender failed: {}",
        String::from_utf8_lossy(&rerender.stderr)
    );
}

#[test]
fn render_all_writes_every_enabled_output() {
    let registry_path = temp_registry_path("render-all-registry");
    let root = temp_test_dir("render-all-root");
    let template_dir = root.join(".bindport").join("templates");
    fs::create_dir_all(&template_dir).expect("template dir");
    fs::write(
        template_dir.join("debug-route.txt.j2"),
        "debug {{ output.name }} {{ route.service }} {{ vars.mode }} {{ route.target_url }}\n",
    )
    .expect("write debug template");

    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"render-all\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"render-all.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n[[outputs]]\nname = \"debug\"\ntemplate = \"debug-route\"\nroot = \".bindport/generated\"\ntarget = \"debug/{{{{ route.service }}}}.txt\"\nauto_render = false\n[outputs.vars]\nmode = \"dev\"\n"
        ),
    )
    .expect("write render config");

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

    let render = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "--all"])
        .output()
        .expect("render all outputs");

    assert!(
        render.status.success(),
        "render all failed: {}",
        String::from_utf8_lossy(&render.stderr)
    );
    let stdout = String::from_utf8(render.stdout).expect("render stdout");
    assert!(stdout.contains("rendered traefik: 1 files"));
    assert!(stdout.contains("rendered debug: 1 files"));

    assert!(root.join(".bindport/generated/traefik/web.yml").is_file());
    assert_eq!(
        fs::read_to_string(root.join(".bindport/generated/debug/web.txt"))
            .expect("debug rendered file"),
        format!("debug debug web dev http://127.0.0.1:{port}")
    );
}

#[test]
fn render_env_local_output_writes_opt_in_dotenv_file() {
    let registry_path = temp_registry_path("render-env-local-registry");
    let root = temp_test_dir("render-env-local-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"env-local-project\"\ndefault_range = \"29430-29439\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"env-local.localhost\"\n[[outputs]]\nname = \"env-local\"\ntemplate = \"bindport-env-local\"\ntarget = \"apps/{{ route.service }}/.env.local\"\nauto_render = false\n",
    )
    .expect("write env-local config");

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

    let render = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "env-local"])
        .output()
        .expect("render env-local output");

    assert!(
        render.status.success(),
        "render env-local failed: {}",
        String::from_utf8_lossy(&render.stderr)
    );
    let stdout = String::from_utf8(render.stdout).expect("render stdout");
    assert!(stdout.contains("rendered env-local: 1 files"));

    let env_file = root.join("apps/web/.env.local");
    let contents = fs::read_to_string(&env_file).expect("env-local file");
    let rendered_port = dotenv_value(&contents, "PORT")
        .expect("rendered port")
        .parse::<u16>()
        .expect("rendered port is numeric");
    assert!((29430..=29439).contains(&rendered_port));
    assert!(contents.contains("BINDPORT_PROJECT=env-local-project"));
    assert!(contents.contains("BINDPORT_SERVICE=web"));
    assert!(contents.contains("BINDPORT_STATE=stopped"));
    assert!(contents.contains(&format!(
        "BINDPORT_TARGET_URL=http://127.0.0.1:{rendered_port}"
    )));
    assert!(contents.contains("BINDPORT_HOSTNAME=env-local.localhost"));
    assert!(contents.contains("BINDPORT_ROUTE_URL=http://env-local.localhost"));
}

#[test]
fn render_repair_records_externally_modified_owned_files() {
    let registry_path = temp_registry_path("render-repair-modified-registry");
    let root = temp_test_dir("render-repair-modified-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"render-repair\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"repair.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n"
        ),
    )
    .expect("write render config");
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

    let render = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "traefik"])
        .output()
        .expect("render output");
    assert!(
        render.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&render.stderr)
    );
    fs::write(&rendered_path, "external change").expect("modify rendered file");

    let repair = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "--repair", "traefik"])
        .output()
        .expect("repair output");
    assert!(
        repair.status.success(),
        "repair failed: {}",
        String::from_utf8_lossy(&repair.stderr)
    );
    let repair_stdout = String::from_utf8(repair.stdout).expect("repair stdout");
    assert!(repair_stdout.contains("repaired traefik: 0 files"));
    assert!(repair_stdout.contains("preserved traefik: 1 externally modified files"));
    assert_eq!(
        fs::read_to_string(&rendered_path).expect("preserved file"),
        "external change"
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["outputs"][0]["rendered"], 0);
    assert_eq!(status["outputs"][0]["error"], 1);
    assert_eq!(status["services"][0]["outputs"][0]["status"], "error");
    assert_eq!(
        status["services"][0]["outputs"][0]["reason"],
        "external_modified"
    );
}

#[test]
fn runner_blocks_start_when_required_output_preflight_fails() {
    let registry_path = temp_registry_path("render-block-preflight-registry");
    let root = temp_test_dir("render-block-preflight-root");
    let port = free_loopback_port();
    let marker_path = root.join("child-ran");
    let marker_arg = marker_path.display().to_string();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"render-block\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"block.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"../blocked.yml\"\non_failure = \"block\"\n"
        ),
    )
    .expect("write render config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf ran > \"$1\"",
            "sh",
            &marker_arg,
        ])
        .output()
        .expect("run bindport");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsafe output target"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker_path.exists());
}

#[test]
fn runner_auto_renders_outputs_on_start_and_exit() {
    let registry_path = temp_registry_path("auto-render-registry");
    let root = temp_test_dir("auto-render-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"auto-render-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"auto.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
        ),
    )
    .expect("write render config");

    let rendered_path = root
        .join(".bindport")
        .join("generated")
        .join("traefik")
        .join("web.yml");
    let mut bindport = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "sleep 2"])
        .spawn()
        .expect("spawn bindport");

    let active_contents =
        wait_for_file_contains(&rendered_path, "routers:", Duration::from_secs(5));
    assert!(active_contents.contains("Host(`auto.localhost`)"));
    assert!(active_contents.contains(&format!("url: \"http://127.0.0.1:{port}\"")));

    let status = wait_for_child(&mut bindport, Duration::from_secs(5)).expect("bindport exits");
    assert!(status.success());

    let stopped_contents =
        wait_for_file_contains(&rendered_path, "is stopped", Duration::from_secs(5));
    assert!(!stopped_contents.contains("routers:"));
}

#[test]
fn runner_skips_outputs_when_auto_render_is_disabled() {
    let registry_path = temp_registry_path("auto-render-disabled-registry");
    let root = temp_test_dir("auto-render-disabled-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"auto-render-disabled\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"disabled.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n"
        ),
    )
    .expect("write render config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.join(".bindport/generated/traefik/web.yml").exists());
}

#[test]
fn project_config_hooks_require_cli_trust() {
    let registry_path = temp_registry_path("hooks-untrusted-registry");
    let root = temp_test_dir("hooks-untrusted-root");
    let hook_log = root.join("hook.log");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-untrusted\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"route-log\"\nevents = [\"route_started\", \"route_finished\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("printf hook >> \"$1\""),
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");
    fs::write(
        root.join(".bindport.local.toml"),
        "[hooks]\ntrusted = true\n",
    )
    .expect("write ignored local trust config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!hook_log.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("hook `route-log` not run (pending approval)"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn trusted_local_hooks_run_for_route_events() {
    let registry_path = temp_registry_path("hooks-trusted-registry");
    let root = temp_test_dir("hooks-trusted-root");
    let hook_log = root.join("hook.log");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-trusted\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"route-log\"\nevents = [\"route_started\", \"route_finished\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("printf '%s|%s\\n' \"$BINDPORT_HOOK_EVENTS\" \"$BINDPORT_HOOK_SOURCES\" >> \"$1\""),
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");
    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "route-log"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );
    assert!(String::from_utf8_lossy(&trust.stdout).contains("approved 1 hook(s)"));

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let hook_output = fs::read_to_string(&hook_log).expect("hook output");

    assert!(hook_output.contains("route_started|cli_runner"));
    assert!(hook_output.contains("route_finished|cli_runner"));
}

#[cfg(unix)]
#[test]
fn trusted_hook_invalidates_when_local_target_changes() {
    let registry_path = temp_registry_path("hooks-target-change-registry");
    let root = temp_test_dir("hooks-target-change-root");
    let hook_log = root.join("hook.log");
    let hook_script = root.join("reload-hook");
    let port = free_loopback_port();
    write_executable(
        &hook_script,
        "#!/bin/sh\nprintf 'v1:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> \"$1\"\n",
    );
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-target-change\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./reload-hook\", {}]\n",
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "reload"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let first_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");
    assert!(
        first_run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&first_run.stderr)
    );
    assert!(
        fs::read_to_string(&hook_log)
            .expect("hook log")
            .contains("v1:route_started")
    );

    write_executable(
        &hook_script,
        "#!/bin/sh\nprintf 'v2:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> \"$1\"\n",
    );
    let status = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "status"])
        .output()
        .expect("hook status");
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("changed\treload"));

    let second_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");
    assert!(
        second_run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&second_run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second_run.stderr)
            .contains("hook `reload` not run (changed since the last trust decision)"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&second_run.stderr)
    );
    assert!(
        !fs::read_to_string(&hook_log)
            .expect("hook log")
            .contains("v2:route_started")
    );
}

#[cfg(unix)]
#[test]
fn hook_timeout_kills_spawned_process_group() {
    let registry_path = temp_registry_path("hooks-timeout-group-registry");
    let root = temp_test_dir("hooks-timeout-group-root");
    let leaked_marker = root.join("leaked-child");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-timeout-group\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 100\n[[hooks.commands]]\nname = \"leaky\"\nevents = [\"route_started\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("sh -c 'sleep 0.5; printf leaked > \"$1\"' sh \"$1\" & sleep 10"),
            toml_string(&leaked_marker.display().to_string()),
        ),
    )
    .expect("write hook config");

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "leaky"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");
    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("hook `sh -c"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("timed out after 100ms"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    thread::sleep(Duration::from_millis(800));
    assert!(
        !leaked_marker.exists(),
        "timed-out hook left a background child running"
    );
}

#[cfg(unix)]
#[test]
fn repo_scoped_hook_trust_applies_to_matching_worktree() {
    let registry_path = temp_registry_path("hooks-repo-scope-registry");
    let repo = temp_test_dir("hooks-repo-scope-root");
    let port = free_loopback_port();
    init_git_repo(&repo, "main");
    write_executable(
        &repo.join("reload-hook"),
        "#!/bin/sh\nprintf 'repo:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> hook.log\n",
    );
    fs::write(
        repo.join(".bindport.toml"),
        format!("project = \"hooks-repo-scope\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./reload-hook\"]\n"),
    )
    .expect("write hook config");
    run_git(&repo, ["add", ".bindport.toml", "reload-hook"]);
    run_git(&repo, ["commit", "-m", "add hook fixture"]);

    let worktree = temp_test_dir("hooks-repo-scope-worktree");
    fs::remove_dir(&worktree).expect("remove empty temp dir before worktree add");
    let worktree_arg = worktree.display().to_string();
    run_git(
        &repo,
        ["worktree", "add", &worktree_arg, "-b", "feature/reuse"],
    );

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&repo)
        .args(["hooks", "trust", "--scope", "repo", "reload"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let run = bindport_with_registry(&registry_path)
        .current_dir(&worktree)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport from second worktree");
    assert!(
        run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        fs::read_to_string(worktree.join("hook.log"))
            .expect("worktree hook log")
            .contains("repo:route_started")
    );
}

#[test]
fn render_dry_run_reports_hooks_without_running_them() {
    let registry_path = temp_registry_path("hooks-dry-run-registry");
    let root = temp_test_dir("hooks-dry-run-root");
    let hook_log = root.join("hook.log");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-dry-run\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"dry-run.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"render-log\"\nevents = [\"render_requested\", \"output_rendered\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("printf hook >> \"$1\""),
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");
    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "render-log"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

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

    let dry_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "--dry-run"])
        .output()
        .expect("dry-run render");

    assert!(
        dry_run.status.success(),
        "render dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let stdout = String::from_utf8(dry_run.stdout).expect("dry-run stdout");

    assert!(stdout.contains("would render traefik: 1 files"));
    assert!(stdout.contains("would run hook render-log"));
    assert!(stdout.contains("BINDPORT_HOOK_CONTEXT=<redacted>"));
    assert!(!hook_log.exists());
}

#[test]
#[cfg(unix)]
fn runner_auto_renders_stale_routes_reconciled_during_route_event() {
    let registry_path = temp_registry_path("auto-render-stale-registry");
    let root = temp_test_dir("auto-render-stale-root");
    let stale_port = free_loopback_port();
    let active_port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"auto-render-stale\"\ndefault_range = \"{active_port}-{active_port}\"\nskip_ports = []\n[[services]]\nname = \"api\"\nhostname = \"api.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
        ),
    )
    .expect("write render config");

    let mut registry = Registry::open(&registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("auto-render-stale"),
        service: String::from("web"),
        git: None,
        identity_key: String::from("v1:auto-render-stale:web"),
    };
    registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port: stale_port,
            hostname: Some(String::from("web.localhost")),
            route_url: Some(String::from("http://web.localhost")),
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: root.clone(),
        })
        .expect("record stale fixture");
    drop(registry);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "api", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stale_contents = fs::read_to_string(root.join(".bindport/generated/traefik/web.yml"))
        .expect("stale route render");
    assert!(stale_contents.contains(" is stale, so no live router was rendered."));
    assert!(!stale_contents.contains("routers:"));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    assert!(status_output.status.success());
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let stale_service = services
        .iter()
        .find(|service| service["service"] == "web")
        .expect("stale service");

    assert_eq!(stale_service["state"], "stale");
    assert_eq!(stale_service["outputs"][0]["status"], "rendered");
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

#[test]
fn runner_delete_on_stopped_removes_owned_output_file_after_exit() {
    let registry_path = temp_registry_path("delete-on-stopped-registry");
    let root = temp_test_dir("delete-on-stopped-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"delete-on-stopped\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"stopped.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\ndelete_on = [\"stopped\", \"removed\"]\n"
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
    assert!(!rendered_path.exists());
}

#[test]
fn doctor_outputs_reports_configured_output() {
    let registry_path = temp_registry_path("doctor-output-registry");
    let root = temp_test_dir("doctor-output-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{ route.service }}.yml\"\n",
    )
    .expect("write output config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("BindPort output doctor"));
    assert!(stdout.contains("routes: 0"));
    assert!(stdout.contains("output traefik:"));
    assert!(stdout.contains("template: bindport-traefik (built-in)"));
    assert!(stdout.contains("planned files: 0"));
    assert!(!root.join(".bindport/generated").exists());
}

#[test]
fn doctor_outputs_reports_hook_trust_status_without_outputs() {
    let registry_path = temp_registry_path("doctor-hook-registry");
    let root = temp_test_dir("doctor-hook-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-hook-project\"\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"route-log\"\nevents = [\"route_started\"]\ncommand = [\"true\"]\n",
    )
    .expect("write hook config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("hooks: 1 configured"));
    assert!(stdout.contains("hook route-log:"));
    assert!(stdout.contains("trust: pending"));
    assert!(stdout.contains("events: route_started"));
    assert!(stdout.contains("target: true (opaque)"));
    assert!(stdout.contains("hook hash:"));
    assert!(stdout.contains("target hash:"));
    assert!(stdout.contains("BINDPORT_HOOK_EVENTS=<redacted>"));
    assert!(stdout.contains("outputs: none configured"));
}

#[test]
fn doctor_outputs_reports_wildcard_template_warning() {
    let registry_path = temp_registry_path("doctor-output-wildcard-registry");
    let root = temp_test_dir("doctor-output-wildcard-root");
    let template_dir = root.join(".bindport").join("templates");
    fs::create_dir_all(&template_dir).expect("template dir");
    fs::write(template_dir.join("debug.10.txt.j2"), "first\n").expect("write first template");
    fs::write(template_dir.join("debug.20.txt.j2"), "second\n").expect("write second template");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"debug\"\ntemplate = \"debug\"\nroot = \".bindport/generated\"\ntarget = \"debug/{{ route.service }}.txt\"\n",
    )
    .expect("write output config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("output debug:"));
    assert!(stdout.contains("template: debug (project)"));
    assert!(stdout.contains("template warning: multiple wildcard matches"));
    assert!(stdout.contains("debug.10.txt.j2"));
}

#[test]
fn doctor_outputs_reports_render_plan_errors() {
    let registry_path = temp_registry_path("doctor-output-collision-registry");
    let root = temp_test_dir("doctor-output-collision-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/same.yml\"\n",
    )
    .expect("write output config");
    record_registry_service(&registry_path, "web", 29_601);
    record_registry_service(&registry_path, "api", 29_602);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("routes: 2"));
    assert!(stdout.contains("output traefik:"));
    assert!(stdout.contains("plan: invalid"));
    assert!(stdout.contains("multiple routes render to target `traefik/same.yml`"));
    assert!(!root.join(".bindport/generated/traefik/same.yml").exists());
}

#[test]
fn doctor_reports_unknown_config_keys() {
    let registry_path = temp_registry_path("doctor-unknown-config-registry");
    let root = temp_test_dir("doctor-unknown-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "defaultrange = \"29100-29199\"\n[proxy.traefik]\nenabled = true\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("ignored unknown top-level keys: defaultrange, proxy"));
    assert!(stdout.contains(
        "config applied keys: project, service, default_range, skip_ports, services, dashboard, output_defaults, outputs, hooks"
    ));
}

#[test]
fn doctor_reports_identity_registry_and_next_candidate() {
    let registry_path = temp_registry_path("doctor-diagnostics-registry");
    let root = temp_test_dir("doctor-diagnostics-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-project\"\nservice = \"web\"\ndefault_range = \"29340-29349\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let candidate = doctor_candidate_port(&stdout);

    assert!(stdout.contains(&format!("registry: {} (ok)", registry_path.display())));
    assert!(stdout.contains("effective identity: project=doctor-project service=web"));
    assert!(stdout.contains("identity key: v1:"));
    assert!(stdout.contains("registry active ports in range: none"));
    assert!(stdout.contains("previous identity port: none"));
    assert!(stdout.contains("os listener conflicts in range: "));
    assert!(stdout.contains("allocation scan start: "));
    assert!((29_340..=29_349).contains(&candidate));
}

#[test]
fn doctor_reports_active_registry_port_conflict() {
    let registry_path = temp_registry_path("doctor-active-conflict-registry");
    let root = temp_test_dir("doctor-active-conflict-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-project\"\nservice = \"web\"\ndefault_range = \"29350-29355\"\nskip_ports = []\n",
    )
    .expect("write project config");
    reserve_registry_port(&registry_path, 29_350);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let candidate = doctor_candidate_port(&stdout);

    assert!(stdout.contains("registry active ports in range: 29350"));
    assert_ne!(candidate, 29_350);
    assert!((29_350..=29_355).contains(&candidate));
}

#[test]
fn doctor_caps_os_listener_conflict_scan_for_wide_ranges() {
    let registry_path = temp_registry_path("doctor-wide-range-registry");
    let root = temp_test_dir("doctor-wide-range-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-project\"\nservice = \"web\"\ndefault_range = \"28500-65535\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("scanned first 1024 of 37036 ports"));
}

#[test]
fn init_creates_fallback_config_in_config_home() {
    let state_dir = temp_test_dir("init-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = config_home_for_registry(&registry_path)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);

    let output = bindport_with_registry(&registry_path)
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(config_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let config = fs::read_to_string(&config_path).expect("fallback config");

    assert!(stdout.contains(&config_path.display().to_string()));
    assert!(config.contains("default_range = \"29000-29999\""));
}

#[cfg(unix)]
#[test]
fn forwards_sigterm_to_wrapped_child_and_records_exit() {
    let registry_path = temp_registry_path("signal-registry");
    let child_pid_path = temp_registry_path("signal-child-pid");
    let marker_path = temp_registry_path("signal-marker");
    let child_pid_path_arg = child_pid_path.display().to_string();
    let marker_path_arg = marker_path.display().to_string();

    let mut bindport = bindport_with_registry(&registry_path)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s\n' $$ > \"$1\"; trap 'printf forwarded > \"$2\"; exit 42' TERM INT; printf 'ready\n'; while :; do sleep 1; done",
            "sh",
            &child_pid_path_arg,
            &marker_path_arg,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run bindport");

    let stdout = bindport.stdout.take().expect("stdout pipe");
    let mut stdout = BufReader::new(stdout);
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read readiness line");
    assert_eq!(ready, "ready\n");

    let child_pid = fs::read_to_string(&child_pid_path)
        .expect("child pid file")
        .trim()
        .parse::<u32>()
        .expect("child pid");

    send_signal(bindport.id(), libc::SIGTERM);

    let status = match wait_for_child(&mut bindport, Duration::from_secs(5)) {
        Some(status) => status,
        None => {
            send_signal(child_pid, libc::SIGKILL);
            let _ = bindport.kill();
            panic!("bindport did not exit after SIGTERM");
        }
    };

    assert_eq!(status.code(), Some(42));
    assert_eq!(
        fs::read_to_string(&marker_path).expect("marker"),
        "forwarded"
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    assert_eq!(status["services"][0]["state"], "stopped");
    assert_eq!(status["services"][0]["exit_code"], 42);
    assert_eq!(status["runs"][0]["exit_code"], 42);
}

fn temp_registry_path(name: &str) -> PathBuf {
    temp_path(name).with_extension("sqlite")
}

fn temp_test_dir(name: &str) -> PathBuf {
    let path = temp_path(name);
    fs::create_dir_all(&path).expect("temp test dir");
    path
}

fn temp_path(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    std::env::temp_dir().join(format!("bindport-{name}-{}-{now}", std::process::id()))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn run_print_port(registry_path: &Path, cwd: &Path) -> u16 {
    let output = bindport_with_registry(registry_path)
        .current_dir(cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    String::from_utf8(output.stdout)
        .expect("stdout is utf8")
        .parse::<u16>()
        .expect("stdout is a port number")
}

fn doctor_candidate_port(stdout: &str) -> u16 {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("next candidate port: "))
        .and_then(|value| value.split_whitespace().next())
        .expect("next candidate port line")
        .parse::<u16>()
        .expect("candidate is a port")
}

fn reserve_registry_port(registry_path: &Path, port: u16) {
    let mut registry = Registry::open(registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("busy-project"),
        service: String::from("busy-service"),
        git: None,
        identity_key: String::from("v1:busy"),
    };

    registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("busy fixture"),
            cwd: PathBuf::from("/tmp/bindport-busy-fixture"),
        })
        .expect("reserve registry port");
}

fn record_registry_service(registry_path: &Path, service: &str, port: u16) {
    let mut registry = Registry::open(registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("doctor-output-project"),
        service: service.to_string(),
        git: None,
        identity_key: format!("v1:doctor-output-project:{service}"),
    };

    registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port,
            hostname: Some(format!("{service}.localhost")),
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("doctor output fixture"),
            cwd: std::env::temp_dir().join("bindport-doctor-output-fixture"),
        })
        .expect("record registry service");
}

struct DashboardProcess {
    child: Child,
    port: u16,
}

impl Drop for DashboardProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_dashboard(command: Command) -> DashboardProcess {
    start_dashboard_with_args(command, &["dashboard"])
}

fn start_dashboard_with_args(mut command: Command, args: &[&str]) -> DashboardProcess {
    let mut child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start bindport dashboard");
    let stdout = child.stdout.take().expect("dashboard stdout");
    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read dashboard URL");

    let port = line
        .trim()
        .strip_prefix("dashboard: http://")
        .expect("dashboard URL line")
        .rsplit_once(':')
        .map(|(_, port)| port)
        .expect("dashboard URL port")
        .parse::<u16>()
        .expect("dashboard port");

    DashboardProcess { child, port }
}

fn http_get(port: u16, path: &str) -> String {
    http_get_with_host(port, path, &format!("127.0.0.1:{port}"))
}

fn http_get_with_host(port: u16, path: &str, host: &str) -> String {
    http_get_with_headers(port, path, host, &[])
}

fn http_get_with_auth(port: u16, path: &str, authorization: &str) -> String {
    http_get_with_headers(
        port,
        path,
        &format!("127.0.0.1:{port}"),
        &[("Authorization", authorization)],
    )
}

fn http_post_clean(port: u16, path: &str, authorization: Option<&str>) -> String {
    let mut headers = vec![("X-BindPort-Dashboard-Action", "clean")];
    if let Some(authorization) = authorization {
        headers.push(("Authorization", authorization));
    }

    http_request_with_headers(port, "POST", path, &format!("127.0.0.1:{port}"), &headers)
}

fn http_get_with_headers(port: u16, path: &str, host: &str, headers: &[(&str, &str)]) -> String {
    http_request_with_headers(port, "GET", path, host, headers)
}

fn http_request_with_headers(
    port: u16,
    method: &str,
    path: &str,
    host: &str,
    headers: &[(&str, &str)],
) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect dashboard");
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\n")
        .expect("write dashboard request");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("write dashboard request header");
    }
    if method == "POST" {
        write!(stream, "Content-Length: 0\r\n").expect("write dashboard request body length");
    }
    write!(stream, "Connection: close\r\n\r\n").expect("finish dashboard request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read dashboard response");
    response
}

fn http_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("http body separator")
}

fn dotenv_value<'a>(contents: &'a str, name: &str) -> Option<&'a str> {
    contents
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn toml_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    format!("\"{escaped}\"")
}

fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

fn init_git_repo(root: &Path, branch: &str) {
    run_git(root, ["init"]);
    run_git(root, ["config", "user.email", "bindport@example.invalid"]);
    run_git(root, ["config", "user.name", "BindPort Test"]);
    run_git(root, ["config", "commit.gpgsign", "false"]);
    fs::write(root.join("README.md"), "test\n").expect("write git fixture");
    run_git(root, ["add", "README.md"]);
    run_git(root, ["commit", "-m", "initial"]);
    run_git(root, ["checkout", "-B", branch]);
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
