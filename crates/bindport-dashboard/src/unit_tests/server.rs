// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn dashboard_options_debug_and_server_bind_report_runtime_details() {
    let callback: DashboardCleanCallback = Arc::new(|_, _| Ok(()));
    let options = DashboardOptions {
        preferred_port: 0,
        clean_callback: Some(callback),
        ..DashboardOptions::default()
    };
    let debug = format!("{options:?}");

    assert!(debug.contains("DashboardOptions"));
    assert!(debug.contains("clean_callback: true"));

    let server = DashboardServer::bind(DashboardOptions {
        preferred_port: 0,
        ..DashboardOptions::default()
    })
    .expect("bind dashboard");

    assert_ne!(server.port(), 0);
    assert_eq!(server.url(), format!("http://127.0.0.1:{}", server.port()));
}

#[test]
fn dashboard_bind_reports_no_available_port_when_preferred_and_fallback_are_busy() {
    let held = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).expect("held listener");
    let port = held.local_addr().expect("held address").port();
    let options = DashboardOptions {
        preferred_port: port,
        fallback_range: PortRange {
            start: port,
            end: port,
        },
        skip_ports: Vec::new(),
        ..DashboardOptions::default()
    };

    let error = match DashboardServer::bind(options) {
        Ok(_) => panic!("bind should fail"),
        Err(error) => error,
    };

    assert!(
        matches!(error, DashboardError::NoAvailablePort { range } if range.start == port && range.end == port)
    );
    assert!(error.to_string().contains("no dashboard port available"));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn dashboard_errors_expose_display_and_sources() {
    let bind = DashboardError::Bind {
        port: 27_080,
        source: io::Error::other("blocked"),
    };
    assert_eq!(
        bind.to_string(),
        "failed to bind dashboard port 27080: blocked"
    );
    assert!(std::error::Error::source(&bind).is_some());

    let local = DashboardError::LocalAddress(io::Error::other("gone"));
    assert_eq!(local.to_string(), "failed to read dashboard address: gone");
    assert!(std::error::Error::source(&local).is_some());
}

#[test]
fn fallback_ports_skip_configured_ports() {
    let options = DashboardOptions {
        fallback_range: PortRange {
            start: 29_100,
            end: 29_102,
        },
        skip_ports: vec![29_101],
        ..DashboardOptions::default()
    };
    let ports = fallback_ports(&options).collect::<Vec<_>>();

    assert_eq!(ports, vec![29_100, 29_102]);
}
