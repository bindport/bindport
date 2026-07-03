// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn previous_identity_port_returns_latest_matching_lease() {
    let mut registry = Registry::open(temp_registry_path("previous-port")).expect("registry");
    let first_identity = test_identity("v1:first");
    let second_identity = test_identity("v1:second");
    let first = registry
        .record_run_started(&RunStart {
            project: first_identity.project.clone(),
            service: first_identity.service.clone(),
            identity: Some(first_identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_123,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("next dev"),
            cwd: PathBuf::from("/tmp/bindport"),
        })
        .expect("record first start");
    registry
        .record_run_finished(first, Some(0))
        .expect("record first finish");
    let second = registry
        .record_run_started(&RunStart {
            project: first_identity.project.clone(),
            service: first_identity.service.clone(),
            identity: Some(first_identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_124,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("next dev"),
            cwd: PathBuf::from("/tmp/bindport"),
        })
        .expect("record second start");
    registry
        .record_run_finished(second, Some(0))
        .expect("record second finish");
    let other = registry
        .record_run_started(&RunStart {
            project: second_identity.project.clone(),
            service: second_identity.service.clone(),
            identity: Some(second_identity),
            host: String::from("127.0.0.1"),
            port: 29_125,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("next dev"),
            cwd: PathBuf::from("/tmp/bindport"),
        })
        .expect("record other start");
    registry
        .record_run_finished(other, Some(0))
        .expect("record other finish");

    assert_eq!(
        registry
            .previous_identity_port(&first_identity.identity_key)
            .expect("previous port"),
        Some(29_124)
    );
    assert_eq!(
        registry
            .previous_identity_port("v1:missing")
            .expect("missing previous port"),
        None
    );
}

#[test]
fn active_ports_reports_active_and_reserved_leases() {
    let mut registry = Registry::open(temp_registry_path("active")).expect("registry");
    registry
        .record_run_started(&test_run_start(
            "bindport",
            "web",
            29_500,
            std::process::id(),
        ))
        .expect("record start");

    assert_eq!(registry.active_ports().expect("ports"), vec![29_500]);
}

#[test]
fn registry_records_reserved_leases_for_status() {
    let mut registry = Registry::open(temp_registry_path("reserved-status")).expect("registry");
    let identity = test_identity("v1:reserved");
    let lease = registry
        .record_reserved_lease(&ReserveLease {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_501,
            hostname: Some(String::from("reserved.localhost")),
            route_url: Some(String::from("http://reserved.localhost")),
            health_url: None,
        })
        .expect("record reserved lease");

    assert_eq!(lease.port, 29_501);
    assert_eq!(registry.active_ports().expect("active ports"), vec![29_501]);
    assert_eq!(
        registry
            .reserved_identity_lease(&identity.identity_key)
            .expect("reserved identity")
            .expect("reserved lease")
            .port,
        29_501
    );

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.services.len(), 1);
    assert!(snapshot.runs.is_empty());
    assert_eq!(snapshot.services[0].state, "reserved");
    assert_eq!(snapshot.services[0].pid, None);
    assert_eq!(snapshot.services[0].command, "reserved");
    assert_eq!(
        snapshot.services[0].route_url.as_deref(),
        Some("http://reserved.localhost")
    );
}

#[test]
fn registry_releases_reserved_leases_by_identity_and_port() {
    let mut registry = Registry::open(temp_registry_path("release-reserved")).expect("registry");
    let first_identity = test_identity("v1:release-first");
    let second_identity = test_identity("v1:release-second");
    registry
        .record_reserved_lease(&ReserveLease {
            project: first_identity.project.clone(),
            service: first_identity.service.clone(),
            identity: Some(first_identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_502,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("record first reserved lease");
    registry
        .record_reserved_lease(&ReserveLease {
            project: second_identity.project.clone(),
            service: second_identity.service.clone(),
            identity: Some(second_identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_503,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("record second reserved lease");

    let released = registry
        .release_reserved_identity(&first_identity.identity_key)
        .expect("release by identity")
        .expect("released lease");
    assert_eq!(released.port, 29_502);
    assert_eq!(registry.active_ports().expect("active ports"), vec![29_503]);

    let released = registry
        .release_reserved_port(29_503)
        .expect("release by port")
        .expect("released lease");
    assert_eq!(released.port, 29_503);
    assert!(registry.active_ports().expect("active ports").is_empty());
    assert!(
        registry
            .release_reserved_port(29_503)
            .expect("second release")
            .is_none()
    );
}

#[test]
fn record_run_started_rejects_duplicate_active_port() {
    let mut registry =
        Registry::open(temp_registry_path("duplicate-active-port")).expect("registry");
    registry
        .record_run_started(&test_run_start(
            "bindport",
            "web",
            29_500,
            std::process::id(),
        ))
        .expect("record first start");

    let error = registry
        .record_run_started(&test_run_start(
            "bindport",
            "api",
            29_500,
            std::process::id(),
        ))
        .expect_err("duplicate active port");

    assert!(matches!(
        error,
        RegistryError::PortConflict { port: 29_500 }
    ));
    assert_eq!(registry.active_ports().expect("ports"), vec![29_500]);
}

#[cfg(target_os = "linux")]
#[test]
fn active_ports_marks_reused_pid_stale_when_start_time_changes() {
    let mut registry = Registry::open(temp_registry_path("stale-reused-pid")).expect("registry");
    let started = registry
        .record_run_started(&test_run_start(
            "bindport",
            "web",
            29_500,
            std::process::id(),
        ))
        .expect("record start");
    registry
        .connection
        .execute(
            "UPDATE runs SET process_start_time = 0 WHERE id = ?1",
            params![started.run_id],
        )
        .expect("force stale start time");

    assert!(registry.active_ports().expect("ports").is_empty());

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.services[0].state, "stale");
    assert!(snapshot.services[0].exited_at.is_some());
}

#[cfg(unix)]
#[test]
fn active_ports_marks_dead_pid_stale() {
    let mut registry = Registry::open(temp_registry_path("stale")).expect("registry");
    registry
        .record_run_started(&test_run_start("bindport", "web", 29_500, 2_000_000_000))
        .expect("record start");

    assert!(registry.active_ports().expect("ports").is_empty());

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.services[0].state, "stale");
    assert!(snapshot.services[0].exited_at.is_some());
    assert_eq!(snapshot.services[0].exit_code, None);
}
