// SPDX-License-Identifier: MIT

use crate::support::*;

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
