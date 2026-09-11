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
