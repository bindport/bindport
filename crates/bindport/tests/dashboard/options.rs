// SPDX-License-Identifier: MIT

use crate::support::*;

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
