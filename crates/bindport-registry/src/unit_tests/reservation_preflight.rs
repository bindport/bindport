// SPDX-License-Identifier: MIT

use super::*;

fn identity(service: &str) -> ServiceIdentity {
    ServiceIdentity {
        project: String::from("preflight"),
        service: service.to_string(),
        git: None,
        identity_key: format!("v1:preflight:{service}:scope"),
    }
}

fn candidate(port: u16) -> ReservationCandidate {
    ReservationCandidate {
        host: String::from("127.0.0.1"),
        port,
        hostname: None,
        route_url: None,
        health_url: None,
    }
}

#[test]
fn preflight_error_prevents_all_batch_inserts() {
    let mut registry = Registry::open(temp_registry_path("preflight-error")).expect("registry");
    let identities = [identity("web"), identity("api")];
    let mut checked = false;
    let result = registry.reserve_services_with_preflight(
        &identities,
        |_, occupied, _| Ok(candidate(31_000 + occupied.len() as u16)),
        |registry, planned| {
            checked = true;
            assert_eq!(planned.len(), 2);
            assert_eq!(planned[0].0, &identities[0]);
            assert_eq!(planned[1].0, &identities[1]);
            assert_ne!(planned[0].1.port, planned[1].1.port);
            assert!(
                registry
                    .export_snapshot()
                    .expect("export")
                    .leases
                    .is_empty()
            );
            Err("preflight rejected")
        },
    );
    assert!(checked);
    assert!(matches!(
        result,
        Err(BatchReservationError::Plan("preflight rejected"))
    ));
    assert!(
        registry
            .export_snapshot()
            .expect("export")
            .leases
            .is_empty()
    );
}

#[test]
fn preflight_skips_existing_services_and_reports_only_committed_mutations() {
    let mut registry = Registry::open(temp_registry_path("preflight-noop")).expect("registry");
    let identities = [identity("web"), identity("api")];
    registry
        .reserve_service(&identities[0], |_, _, _| Ok::<_, ()>(candidate(31_010)))
        .expect("web");
    let mut checked = false;
    let services = registry
        .reserve_services_with_preflight(
            &identities,
            |service, _, _| {
                assert_eq!(service, &identities[1]);
                Ok::<_, ()>(candidate(31_011))
            },
            |_, planned| {
                checked = true;
                assert_eq!(planned.len(), 1);
                assert_eq!(planned[0].0, &identities[1]);
                Ok(())
            },
        )
        .expect("mixed reservation");
    assert!(checked);
    assert!(!services[0].1);
    assert!(services[1].1);
    for inputs in [identities.as_slice(), &[]] {
        let reused = registry
            .reserve_services_with_preflight(
                inputs,
                |_, _, _| -> Result<ReservationCandidate, ()> {
                    panic!("must not plan existing services")
                },
                |_, _| panic!("must not preflight a no-op"),
            )
            .expect("no-op");
        assert_eq!(reused.len(), inputs.len());
        assert!(reused.iter().all(|(_, changed)| !changed));
    }
}

#[test]
fn preflight_is_repeated_for_replanned_candidates_after_a_competing_commit() {
    let path = temp_registry_path("preflight-retry");
    let mut registry = Registry::open(&path).expect("registry");
    let mut writer = Registry::open(&path).expect("other writer");
    let identities = [identity("web"), identity("api")];
    let mut attempts = Vec::new();
    let result = registry.reserve_services_with_preflight(
        &identities,
        |_, occupied, _| {
            let port = (31_020..31_030)
                .find(|port| !occupied.contains(port))
                .expect("candidate");
            Ok(candidate(port))
        },
        |_, planned| {
            attempts.push(
                planned
                    .iter()
                    .map(|(identity, candidate)| (identity.service.clone(), candidate.port))
                    .collect::<Vec<_>>(),
            );
            if attempts.len() == 1 {
                writer
                    .reserve_service(&identities[0], |_, _, _| Ok::<_, ()>(candidate(31_022)))
                    .expect("competing web");
                writer
                    .reserve_service(&identity("other"), |_, _, _| Ok::<_, ()>(candidate(31_021)))
                    .expect("occupy api candidate");
                Ok(())
            } else {
                Err("replanned candidate rejected")
            }
        },
    );
    assert!(matches!(
        result,
        Err(BatchReservationError::Plan("replanned candidate rejected"))
    ));
    assert_eq!(
        attempts,
        vec![
            vec![("web".into(), 31_020), ("api".into(), 31_021)],
            vec![("api".into(), 31_020)]
        ]
    );
    let leases = registry.export_snapshot().expect("export").leases;
    assert_eq!(leases.len(), 2);
    assert!(leases.iter().all(|lease| lease.service != "api"));
}

#[test]
fn preflight_planning_does_not_count_another_writers_reservations_as_new() {
    let path = temp_registry_path("preflight-writer-wins");
    let mut registry = Registry::open(&path).expect("registry");
    let mut writer = Registry::open(&path).expect("other writer");
    let identities = [identity("web"), identity("api")];
    let services = registry
        .reserve_services_with_preflight(
            &identities,
            |_, occupied, _| Ok::<_, ()>(candidate(31_030 + occupied.len() as u16)),
            |_, planned| {
                assert_eq!(planned.len(), 2);
                writer
                    .reserve_services(&identities, |_, occupied, _| {
                        Ok::<_, ()>(candidate(31_040 + occupied.len() as u16))
                    })
                    .expect("competing batch");
                Ok(())
            },
        )
        .expect("reuse competing batch");
    assert!(services.iter().all(|(_, changed)| !changed));
    assert_eq!(services[0].0.port, 31_040);
    assert_eq!(services[1].0.port, 31_041);
    assert_eq!(registry.export_snapshot().expect("export").leases.len(), 2);
}
