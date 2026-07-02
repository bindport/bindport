// SPDX-License-Identifier: MIT

use crate::support::*;

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
