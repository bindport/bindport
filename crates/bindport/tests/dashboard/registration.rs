// SPDX-License-Identifier: MIT

use crate::support::*;

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
