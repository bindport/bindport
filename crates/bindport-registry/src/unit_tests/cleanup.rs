// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn clean_leases_dry_run_counts_without_deleting_stopped_runs() {
    let mut registry = Registry::open(temp_registry_path("clean-dry-run")).expect("registry");
    let started = registry
        .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
        .expect("record start");

    registry
        .record_run_finished(started, Some(0))
        .expect("record finish");

    let summary = registry
        .clean_leases(&[CleanState::Stopped], true)
        .expect("clean dry-run");

    assert_eq!(summary.stopped_leases, 1);
    assert_eq!(summary.stale_leases, 0);
    assert_eq!(summary.runs, 1);
    assert_eq!(summary.total_leases(), 1);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.services.len(), 1);
    assert_eq!(snapshot.runs.len(), 1);
}

#[test]
fn clean_leases_removes_stopped_runs() {
    let mut registry = Registry::open(temp_registry_path("clean-stopped")).expect("registry");
    let started = registry
        .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
        .expect("record start");

    registry
        .record_run_finished(started, Some(0))
        .expect("record finish");

    let summary = registry
        .clean_leases(&[CleanState::Stopped, CleanState::Stale], false)
        .expect("clean");

    assert_eq!(summary.stopped_leases, 1);
    assert_eq!(summary.stale_leases, 0);
    assert_eq!(summary.runs, 1);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert!(snapshot.services.is_empty());
    assert!(snapshot.runs.is_empty());
}

#[cfg(unix)]
#[test]
fn clean_leases_reconciles_and_removes_stale_runs() {
    let mut registry = Registry::open(temp_registry_path("clean-stale")).expect("registry");
    registry
        .record_run_started(&test_run_start("bindport", "web", 29_500, 2_000_000_000))
        .expect("record start");

    let summary = registry
        .clean_leases(&[CleanState::Stale], false)
        .expect("clean stale");

    assert_eq!(summary.stopped_leases, 0);
    assert_eq!(summary.stale_leases, 1);
    assert_eq!(summary.runs, 1);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert!(snapshot.services.is_empty());
    assert!(snapshot.runs.is_empty());
}

#[cfg(unix)]
#[test]
fn prune_oldest_stale_leases_removes_only_pressure_excess() {
    let mut registry = Registry::open(temp_registry_path("prune-stale")).expect("registry");

    for index in 0..4 {
        registry
            .record_run_started(&test_run_start(
                "bindport",
                &format!("web-{index}"),
                29_500 + index,
                2_000_000_000 + index as u32,
            ))
            .expect("record stale candidate");
    }

    let dry_run = registry
        .prune_oldest_stale_leases(29_500, 29_503, 2, true)
        .expect("dry-run prune stale");

    assert_eq!(dry_run.stale_leases, 2);
    assert_eq!(dry_run.runs, 2);
    assert_eq!(
        registry.status_snapshot().expect("snapshot").services.len(),
        4
    );

    let summary = registry
        .prune_oldest_stale_leases(29_500, 29_503, 2, false)
        .expect("prune stale");

    assert_eq!(summary.stale_leases, 2);
    assert_eq!(summary.runs, 2);

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.services.len(), 2);
    assert_eq!(snapshot.runs.len(), 2);
}

#[cfg(unix)]
#[test]
fn prune_oldest_stale_leases_preserves_active_reserved_and_stopped() {
    let mut registry = Registry::open(temp_registry_path("prune-preserve")).expect("registry");

    registry
        .record_run_started(&test_run_start(
            "bindport",
            "active",
            29_510,
            std::process::id(),
        ))
        .expect("record active");
    registry
        .record_reserved_lease(&ReserveLease {
            project: String::from("bindport"),
            service: String::from("reserved"),
            identity: None,
            host: String::from("127.0.0.1"),
            port: 29_511,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("record reserved");
    let stopped = registry
        .record_run_started(&test_run_start("bindport", "stopped", 29_512, 12_345))
        .expect("record stopped");
    registry
        .record_run_finished(stopped, Some(0))
        .expect("record stopped finish");

    for index in 0..2 {
        registry
            .record_run_started(&test_run_start(
                "bindport",
                &format!("stale-{index}"),
                29_513 + index,
                2_000_000_000 + index as u32,
            ))
            .expect("record stale candidate");
    }

    let summary = registry
        .prune_oldest_stale_leases(29_510, 29_514, 2, false)
        .expect("prune stale");

    assert_eq!(summary.stale_leases, 2);
    assert_eq!(summary.runs, 2);

    let snapshot = registry.status_snapshot().expect("snapshot");
    let states = snapshot
        .services
        .iter()
        .map(|service| (service.service.as_str(), service.state.as_str()))
        .collect::<Vec<_>>();

    assert!(states.contains(&("active", "active")));
    assert!(states.contains(&("reserved", "reserved")));
    assert!(states.contains(&("stopped", "stopped")));
    assert!(!states.iter().any(|(_, state)| *state == "stale"));
}
