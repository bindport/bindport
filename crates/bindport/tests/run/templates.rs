// SPDX-License-Identifier: MIT

use crate::support::*;

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
