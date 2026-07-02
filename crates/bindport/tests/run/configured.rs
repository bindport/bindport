// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn configured_service_command_expands_port_arguments() {
    let registry_path = temp_registry_path("configured-command-registry");
    let root = temp_test_dir("configured-command-root");
    let range_start = free_loopback_port();
    let range_end = range_start.saturating_add(10);
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "storybook-project"
default_range = "{range_start}-{range_end}"
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
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let parts = stdout.split('|').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[1], "--port");
    assert_eq!(parts[0], parts[2]);
    let assigned_port = parts[0].parse::<u16>().expect("assigned port");
    assert!(
        (range_start..=range_end).contains(&assigned_port),
        "assigned port {assigned_port} outside configured range {range_start}-{range_end}"
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
            .ends_with(&format!("--port {assigned_port}"))
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
            "project = \"example-app\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"{{branch}}.{{project}}.localhost\"\nhealth_url = \"{{route_url}}/health\"\nenv.BINDPORT_ASSIGNED_PORT = \"{{port}}\"\nenv.BINDPORT_ROUTE = \"{{route_url}}\"\nenv.BINDPORT_HEALTH = \"{{health_url}}\"\nenv.BINDPORT_DIRECT_URL = \"{{url}}\"\nenv.HOSTNAME = \"0.0.0.0\"\n"
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
            "{port}|http://feature-tree.example-app.localhost|http://feature-tree.example-app.localhost/health|http://127.0.0.1:{port}|0.0.0.0"
        )
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(service["project"], "example-app");
    assert_eq!(service["service"], "web");
    assert_eq!(service["hostname"], "feature-tree.example-app.localhost");
    assert_eq!(
        service["route_url"],
        "http://feature-tree.example-app.localhost"
    );
    assert_eq!(
        service["health_url"],
        "http://feature-tree.example-app.localhost/health"
    );
    assert_eq!(service["port"], port);
}
#[test]
fn service_config_rejects_execution_sensitive_env_names() {
    let registry_path = temp_registry_path("service-env-deny-registry");
    let root = temp_test_dir("service-env-deny-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"env-deny\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nenv.NODE_OPTIONS = \"--require ./evil.js\"\nenv.LD_AUDIT = \"./audit.so\"\nenv.GCONV_PATH = \"./gconv\"\nenv.SAFE_VALUE = \"allowed\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s' \"${NODE_OPTIONS-unset}\" \"${LD_AUDIT-unset}\" \"${GCONV_PATH-unset}\" \"$SAFE_VALUE\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"unset|unset|unset|allowed");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `NODE_OPTIONS`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `LD_AUDIT`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `GCONV_PATH`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
