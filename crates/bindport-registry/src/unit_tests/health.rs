// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn health_targets_are_restricted_to_loopback_without_dns() {
    let target = http_health_target("http://127.0.0.1:29100/health")
        .expect("loopback URL parses")
        .expect("loopback URL is supported");
    assert_eq!(
        target.address,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_100)
    );
    assert_eq!(target.authority, "127.0.0.1:29100");
    assert_eq!(target.path, "/health");

    let target = http_health_target("http://feature.branch.localhost:29101/ready")
        .expect("localhost URL parses")
        .expect("localhost URL is supported");
    assert_eq!(
        target.address,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_101)
    );
    assert_eq!(target.authority, "feature.branch.localhost:29101");

    assert!(
        http_health_target("http://169.254.169.254/latest")
            .expect("metadata URL parses")
            .is_none()
    );
    assert!(
        http_health_target("http://example.invalid/health")
            .expect("external URL parses")
            .is_none()
    );
    assert!(
        http_health_target("https://127.0.0.1:29100/health")
            .expect("https URL parses as unsupported")
            .is_none()
    );
}

#[test]
fn health_targets_reject_request_line_injection_bytes() {
    for url in [
        "http://127.0.0.1:6379/\r\nFLUSHALL\r\n",
        "http://127.0.0.1:6379/path with-space",
        "http://local\nhost:6379/health",
    ] {
        assert!(http_health_target(url).is_err(), "{url:?}");
    }
}

#[test]
fn health_targets_parse_authority_edges_without_dns() {
    assert!(http_health_target("ftp://127.0.0.1/health").is_err());
    assert!(http_health_target("http:///health").is_err());
    assert!(http_health_target("http://127.0.0.1:29100/bad\rpath").is_err());

    assert_eq!(
        parse_http_authority("[::1]"),
        Some((String::from("::1"), 80))
    );
    assert_eq!(
        parse_http_authority("[::1]:29100"),
        Some((String::from("::1"), 29_100))
    );

    for authority in [
        "",
        "user@localhost",
        "[]:29100",
        "[::1]:",
        "[::1]extra",
        "localhost:",
        ":29100",
        "a:b:c",
    ] {
        assert!(parse_http_authority(authority).is_none(), "{authority:?}");
    }

    assert_eq!(
        loopback_socket_addr("localhost.", 29_100),
        Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_100))
    );
    assert_eq!(
        loopback_socket_addr("api.localhost", 29_101),
        Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_101))
    );
}

#[test]
fn probe_http_target_reports_empty_invalid_and_malformed_responses() {
    let empty = start_raw_health_server(Vec::new());
    let target = http_health_target(&format!("http://127.0.0.1:{empty}/health"))
        .expect("empty target")
        .expect("loopback");
    let error = probe_http_target(&target).expect_err("empty response");
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);

    let invalid_utf8 = start_raw_health_server(vec![0xff, b'\n']);
    let target = http_health_target(&format!("http://127.0.0.1:{invalid_utf8}/health"))
        .expect("invalid target")
        .expect("loopback");
    let error = probe_http_target(&target).expect_err("invalid utf8");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let malformed = start_raw_health_server(b"HTTP/1.1 OK\r\n\r\n".to_vec());
    let target = http_health_target(&format!("http://127.0.0.1:{malformed}/health"))
        .expect("malformed target")
        .expect("loopback");
    let error = probe_http_target(&target).expect_err("malformed status");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("missing HTTP status"));
}

#[test]
fn status_health_is_pending_during_startup_grace() {
    let mut registry = Registry::open(temp_registry_path("health-pending")).expect("registry");
    let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
    run.health_url = Some(format!("http://127.0.0.1:{}/health", free_loopback_port()));

    registry.record_run_started(&run).expect("record start");

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.services[0].health_url.as_deref(),
        run.health_url.as_deref()
    );
    assert_eq!(snapshot.services[0].health, "pending");
}

#[test]
fn status_health_reports_healthy_http_response() {
    let direct_port = start_health_server("204 No Content");
    let direct_url = format!("http://127.0.0.1:{direct_port}/health");
    let direct_target = http_health_target(&direct_url)
        .expect("direct health URL parses")
        .expect("direct health URL is supported");
    let direct_status = probe_http_target(&direct_target).expect("direct health probe");
    assert!((200..400).contains(&direct_status));

    let health_port = start_health_server("204 No Content");
    let mut registry = Registry::open(temp_registry_path("health-healthy")).expect("registry");
    let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
    run.health_url = Some(format!("http://127.0.0.1:{health_port}/health"));

    registry.record_run_started(&run).expect("record start");
    mark_latest_run_started_before_grace(&registry);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.services[0].health_url.as_deref(),
        run.health_url.as_deref()
    );
    assert_eq!(snapshot.services[0].health, "healthy");
}

#[test]
fn status_health_reports_failing_http_response() {
    let mut registry = Registry::open(temp_registry_path("health-failing")).expect("registry");
    let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
    run.health_url = Some(format!("http://127.0.0.1:{}/health", free_loopback_port()));

    registry.record_run_started(&run).expect("record start");
    mark_latest_run_started_before_grace(&registry);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.services[0].health_url.as_deref(),
        run.health_url.as_deref()
    );
    assert_eq!(snapshot.services[0].health, "failing");
}

#[test]
fn status_health_reports_unknown_for_non_loopback_http_targets() {
    let mut registry = Registry::open(temp_registry_path("health-non-loopback")).expect("registry");
    let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
    run.health_url = Some(String::from("http://192.0.2.1/health"));

    registry.record_run_started(&run).expect("record start");
    mark_latest_run_started_before_grace(&registry);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.services[0].health_url.as_deref(),
        run.health_url.as_deref()
    );
    assert_eq!(snapshot.services[0].health, "unknown");
}

#[test]
fn status_health_reports_unknown_for_unsupported_schemes() {
    let mut registry = Registry::open(temp_registry_path("health-unsupported")).expect("registry");
    let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
    run.health_url = Some(String::from("https://web.localhost/health"));

    registry.record_run_started(&run).expect("record start");
    mark_latest_run_started_before_grace(&registry);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.services[0].health_url.as_deref(),
        run.health_url.as_deref()
    );
    assert_eq!(snapshot.services[0].health, "unknown");
}
