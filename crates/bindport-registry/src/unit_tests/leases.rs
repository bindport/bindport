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
            command: current_process_command(),
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
            command: current_process_command(),
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
            command: current_process_command(),
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
fn failed_reserved_startup_records_exit_and_restores_reservation() {
    let mut registry =
        Registry::open(temp_registry_path("reserved-startup-failure")).expect("registry");
    let identity = test_identity("v1:reserved-startup-failure");
    let lease = registry
        .record_reserved_lease(&ReserveLease {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_504,
            hostname: Some(String::from("reserved.localhost")),
            route_url: Some(String::from("http://reserved.localhost")),
            health_url: Some(String::from("http://reserved.localhost/health")),
        })
        .expect("reservation");
    let started = registry
        .promote_reserved_lease(&ReservedRunStart {
            lease_id: lease.lease_id,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("promotion");

    registry
        .record_reserved_run_failed(started, Some(98))
        .expect("record failed startup");

    let service = registry
        .select_service(&identity)
        .expect("reserved service");
    assert_eq!(service.state, "reserved");
    assert_eq!(service.port, 29_504);
    assert_eq!(
        service.route_url.as_deref(),
        Some("http://reserved.localhost")
    );
    let snapshot = registry.status_snapshot().expect("status");
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.runs[0].exit_code, Some(98));
    assert!(snapshot.runs[0].exited_at.is_some());
}

#[test]
fn failed_reserved_startup_stops_lease_when_port_was_reassigned() {
    let path = temp_registry_path("reserved-restore-conflict");
    let mut registry = Registry::open(&path).expect("registry");
    let failed_identity = test_identity("v1:failed-reservation");
    let replacement_identity = test_identity("v1:replacement-reservation");
    let failed = registry
        .record_reserved_lease(&ReserveLease {
            project: failed_identity.project.clone(),
            service: failed_identity.service.clone(),
            identity: Some(failed_identity),
            host: String::from("127.0.0.1"),
            port: 29_507,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("failed reservation");
    let started = registry
        .promote_reserved_lease(&ReservedRunStart {
            lease_id: failed.lease_id,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("promotion");
    assert_eq!(
        registry
            .mark_observed_runs_stale(&[ActiveRun {
                lease_id: started.lease_id,
                run_id: started.run_id,
                pid: std::process::id(),
                process_start_time: None,
                command: current_process_command(),
            }])
            .expect("stale reconciliation"),
        1
    );

    let mut concurrent = Registry::open(&path).expect("concurrent registry");
    let replacement = concurrent
        .record_reserved_lease(&ReserveLease {
            project: replacement_identity.project.clone(),
            service: replacement_identity.service.clone(),
            identity: Some(replacement_identity),
            host: String::from("127.0.0.1"),
            port: 29_507,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("replacement reservation");

    let error = registry
        .record_reserved_run_failed(started, Some(98))
        .expect_err("restore must reject reassigned port");
    assert!(matches!(
        error,
        RegistryError::ReservationRestoreConflict {
            lease_id,
            port: 29_507
        } if lease_id == failed.lease_id
    ));

    let export = registry.export_snapshot().expect("export");
    assert_eq!(
        export
            .leases
            .iter()
            .find(|lease| lease.id == failed.lease_id)
            .expect("failed lease")
            .state,
        "stopped"
    );
    assert_eq!(
        export
            .leases
            .iter()
            .find(|lease| lease.id == replacement.lease_id)
            .expect("replacement lease")
            .state,
        "reserved"
    );
    assert_eq!(
        export
            .leases
            .iter()
            .filter(|lease| {
                lease.port == 29_507 && matches!(lease.state.as_str(), "active" | "reserved")
            })
            .count(),
        1
    );
    let run = export
        .runs
        .iter()
        .find(|run| run.id == started.run_id)
        .expect("failed run");
    assert_eq!(run.exit_code, Some(98));
    assert!(run.exited_at.is_some());
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

#[test]
fn run_claim_can_be_adopted_and_discarded_without_leaving_registry_rows() {
    let mut registry = Registry::open(temp_registry_path("run-claim-lifecycle")).expect("registry");
    let claim = registry
        .record_run_started(&test_run_start(
            "bindport",
            "web",
            29_504,
            std::process::id(),
        ))
        .expect("claim run port");

    registry
        .adopt_run_claim(
            claim,
            std::process::id(),
            "adopted child command",
            &env::temp_dir(),
        )
        .expect("adopt run claim");
    let active = registry.export_snapshot().expect("active export");
    assert_eq!(active.leases.len(), 1);
    assert_eq!(active.leases[0].state, "active");
    assert_eq!(active.runs.len(), 1);
    assert_eq!(active.runs[0].command, "adopted child command");

    registry
        .discard_run_claim(claim)
        .expect("discard run claim");
    let discarded = registry.export_snapshot().expect("discarded export");
    assert!(discarded.leases.is_empty());
    assert!(discarded.runs.is_empty());
}

#[test]
fn fast_run_claim_finalization_records_child_metadata_and_exit_atomically() {
    let mut registry =
        Registry::open(temp_registry_path("fast-run-claim-finalization")).expect("registry");
    let claim = registry
        .record_run_started(&test_run_start(
            "bindport",
            "web",
            29_506,
            std::process::id(),
        ))
        .expect("claim run port");
    let child_cwd = env::temp_dir().join("bindport-fast-child");

    registry
        .finalize_run_claim(
            claim,
            4_000_000_000,
            "fast child command",
            &child_cwd,
            Some(23),
        )
        .expect("finalize fast run claim");

    let snapshot = registry.export_snapshot().expect("finalized export");
    assert_eq!(snapshot.leases.len(), 1);
    assert_eq!(snapshot.leases[0].state, "stopped");
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.runs[0].pid, 4_000_000_000);
    assert_eq!(snapshot.runs[0].command, "fast child command");
    assert_eq!(snapshot.runs[0].cwd, child_cwd.display().to_string());
    assert_eq!(snapshot.runs[0].exit_code, Some(23));
    assert!(snapshot.runs[0].exited_at.is_some());
}

#[test]
fn reserved_run_claim_can_be_restored_without_leaving_a_run() {
    let mut registry =
        Registry::open(temp_registry_path("reserved-run-claim-rollback")).expect("registry");
    let reservation = registry
        .record_reserved_lease(&ReserveLease {
            project: String::from("bindport"),
            service: String::from("web"),
            identity: Some(test_identity("v1:reserved-run-claim-rollback")),
            host: String::from("127.0.0.1"),
            port: 29_507,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("reservation");
    let claim = registry
        .promote_reserved_lease(&ReservedRunStart {
            lease_id: reservation.lease_id,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("claim reservation");

    registry
        .restore_reserved_run_claim(claim)
        .expect("restore reservation");
    let restored = registry.export_snapshot().expect("restored export");
    assert_eq!(restored.leases.len(), 1);
    assert_eq!(restored.leases[0].id, reservation.lease_id);
    assert_eq!(restored.leases[0].state, "reserved");
    assert!(restored.runs.is_empty());
}

#[test]
fn stale_reconciliation_does_not_clobber_a_concurrently_finished_run() {
    let path = temp_registry_path("stale-finish-race");
    let mut registry = Registry::open(&path).expect("registry");
    let first = registry
        .record_run_started(&test_run_start(
            "bindport",
            "web",
            29_505,
            std::process::id(),
        ))
        .expect("first run");
    let second = registry
        .record_run_started(&test_run_start(
            "bindport",
            "api",
            29_506,
            std::process::id(),
        ))
        .expect("second run");
    let observed_stale = [
        ActiveRun {
            lease_id: first.lease_id,
            run_id: first.run_id,
            pid: std::process::id(),
            process_start_time: None,
            command: current_process_command(),
        },
        ActiveRun {
            lease_id: second.lease_id,
            run_id: second.run_id,
            pid: std::process::id(),
            process_start_time: None,
            command: current_process_command(),
        },
    ];

    let mut concurrent = Registry::open(&path).expect("concurrent registry");
    concurrent
        .record_run_finished(first, Some(0))
        .expect("concurrent finish");

    assert_eq!(
        registry
            .mark_observed_runs_stale(&observed_stale)
            .expect("mark observed stale runs"),
        1
    );
    let export = registry.export_snapshot().expect("registry export");
    let first_lease = export
        .leases
        .iter()
        .find(|lease| lease.id == first.lease_id)
        .expect("first lease");
    let first_run = export
        .runs
        .iter()
        .find(|run| run.id == first.run_id)
        .expect("first run");
    assert_eq!(first_lease.state, "stopped");
    assert_eq!(first_run.exit_code, Some(0));
    assert!(first_run.exited_at.is_some());

    let second_lease = export
        .leases
        .iter()
        .find(|lease| lease.id == second.lease_id)
        .expect("second lease");
    let second_run = export
        .runs
        .iter()
        .find(|run| run.id == second.run_id)
        .expect("second run");
    assert_eq!(second_lease.state, "stale");
    assert_eq!(second_run.exit_code, None);
    assert!(second_run.exited_at.is_some());
}

#[test]
fn stale_observation_does_not_clobber_a_repromoted_live_run() {
    let mut registry =
        Registry::open(temp_registry_path("stale-repromotion-race")).expect("registry");
    let identity = test_identity("v1:stale-repromotion-race");
    let reservation = registry
        .record_reserved_lease(&ReserveLease {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port: 29_508,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("reservation");
    let first = registry
        .promote_reserved_lease(&ReservedRunStart {
            lease_id: reservation.lease_id,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("first promotion");
    let observed_stale = [ActiveRun {
        lease_id: first.lease_id,
        run_id: first.run_id,
        pid: std::process::id(),
        process_start_time: None,
        command: current_process_command(),
    }];
    registry
        .record_reserved_run_failed(first, Some(98))
        .expect("restore reservation");
    let second = registry
        .promote_reserved_lease(&ReservedRunStart {
            lease_id: reservation.lease_id,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("second promotion");

    assert_eq!(
        registry
            .mark_observed_runs_stale(&observed_stale)
            .expect("stale compare-and-set"),
        0
    );
    let export = registry.export_snapshot().expect("export");
    assert_eq!(export.leases[0].state, "active");
    let first_run = export
        .runs
        .iter()
        .find(|run| run.id == first.run_id)
        .expect("first run");
    assert!(first_run.exited_at.is_some());
    let second_run = export
        .runs
        .iter()
        .find(|run| run.id == second.run_id)
        .expect("second run");
    assert!(second_run.exited_at.is_none());
    assert_eq!(second_run.exit_code, None);
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
