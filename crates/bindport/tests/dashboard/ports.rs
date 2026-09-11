// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn dashboard_falls_back_when_preferred_port_is_busy() {
    let busy_preferred = TcpListener::bind(("127.0.0.1", 0)).expect("bind busy dashboard port");
    let preferred_port = busy_preferred
        .local_addr()
        .expect("busy dashboard port")
        .port();
    let (mut fallback_guards, range_start, range_end) = guarded_port_range();
    let registry_path = temp_registry_path("dashboard-fallback-registry");
    let root = temp_test_dir("dashboard-fallback-root");
    fs::write(
        root.join(".bindport.toml"),
        format!("default_range = \"{range_start}-{range_end}\"\nskip_ports = []\n"),
    )
    .expect("write dashboard fallback config");

    fallback_guards.truncate(1);
    let mut command = bindport_with_registry(&registry_path);
    command.current_dir(&root);
    let preferred_port_arg = preferred_port.to_string();
    let dashboard = start_dashboard_with_args(
        command,
        &["dashboard", "serve", "--port", &preferred_port_arg],
    );

    assert!((range_start + 1..=range_end).contains(&dashboard.port));
    assert_ne!(dashboard.port, preferred_port);
    assert!(http_get(dashboard.port, "/healthz").starts_with("HTTP/1.1 200 OK"));

    drop(busy_preferred);
}
