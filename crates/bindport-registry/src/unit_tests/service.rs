// SPDX-License-Identifier: MIT

use super::*;
use std::sync::{Arc, Barrier};

fn candidate(port: u16) -> ReservationCandidate {
    ReservationCandidate {
        host: String::from("127.0.0.1"),
        port,
        hostname: Some(String::from("web.localhost")),
        route_url: Some(String::from("http://web.localhost")),
        health_url: Some(String::from("http://web.localhost/health")),
    }
}

fn scoped_identity(project: &str, service: &str, worktree: &str) -> ServiceIdentity {
    ServiceIdentity {
        project: project.to_string(),
        service: service.to_string(),
        git: None,
        identity_key: format!("v1:{project}:{service}:{worktree}"),
    }
}

#[test]
fn selector_accepts_only_active_or_reserved_exact_scope() {
    let mut registry = Registry::open(temp_registry_path("scoped-selector")).expect("registry");
    let first = scoped_identity("example", "web", "worktree-a");
    let second = scoped_identity("example", "web", "worktree-b");
    let other_project = scoped_identity("other", "web", "worktree-a");

    registry
        .record_reserved_lease(&ReserveLease {
            project: first.project.clone(),
            service: first.service.clone(),
            identity: Some(first.clone()),
            host: String::from("127.0.0.1"),
            port: 29_510,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("first reservation");
    registry
        .record_reserved_lease(&ReserveLease {
            project: second.project.clone(),
            service: second.service.clone(),
            identity: Some(second.clone()),
            host: String::from("127.0.0.1"),
            port: 29_511,
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("second reservation");

    assert_eq!(registry.select_service(&first).expect("first").port, 29_510);
    assert_eq!(
        registry.select_service(&second).expect("second").port,
        29_511
    );
    assert!(matches!(
        registry.select_service(&other_project),
        Err(RegistryError::ServiceNotFound { .. })
    ));

    registry
        .release_reserved_identity(&first.identity_key)
        .expect("release")
        .expect("released");
    assert!(matches!(
        registry.select_service(&first),
        Err(RegistryError::ServiceNotFound { .. })
    ));
}

#[test]
fn selector_returns_one_active_reserved_metadata_snapshot() {
    let mut registry = Registry::open(temp_registry_path("selector-snapshot")).expect("registry");
    let reserved = scoped_identity("example", "api", "worktree");
    let active = scoped_identity("example", "web", "worktree");
    registry
        .record_reserved_lease(&ReserveLease {
            project: reserved.project.clone(),
            service: reserved.service.clone(),
            identity: Some(reserved.clone()),
            host: String::from("127.0.0.2"),
            port: 29_540,
            hostname: Some(String::from("api.localhost")),
            route_url: Some(String::from("https://api.localhost")),
            health_url: Some(String::from("https://api.localhost/health")),
        })
        .expect("reservation");
    registry
        .record_run_started(&RunStart {
            project: active.project.clone(),
            service: active.service.clone(),
            identity: Some(active.clone()),
            host: String::from("127.0.0.3"),
            port: 29_541,
            hostname: Some(String::from("web.localhost")),
            route_url: Some(String::from("https://web.localhost")),
            health_url: Some(String::from("https://web.localhost/health")),
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("active run");

    let services = registry
        .select_services(&[reserved, active])
        .expect("service snapshot");

    assert_eq!(services[0].state, "reserved");
    assert_eq!(services[0].host, "127.0.0.2");
    assert_eq!(services[0].port, 29_540);
    assert_eq!(services[0].hostname.as_deref(), Some("api.localhost"));
    assert_eq!(
        services[0].route_url.as_deref(),
        Some("https://api.localhost")
    );
    assert_eq!(
        services[0].health_url.as_deref(),
        Some("https://api.localhost/health")
    );
    assert_eq!(services[1].state, "active");
    assert_eq!(services[1].host, "127.0.0.3");
    assert_eq!(services[1].port, 29_541);
    assert_eq!(services[1].hostname.as_deref(), Some("web.localhost"));
    assert_eq!(
        services[1].route_url.as_deref(),
        Some("https://web.localhost")
    );
    assert_eq!(
        services[1].health_url.as_deref(),
        Some("https://web.localhost/health")
    );
}

#[test]
fn selector_rejects_ambiguous_exact_scope() {
    let mut registry = Registry::open(temp_registry_path("ambiguous-selector")).expect("registry");
    let identity = scoped_identity("example", "web", "worktree");

    for port in [29_512, 29_513] {
        registry
            .record_reserved_lease(&ReserveLease {
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity: Some(identity.clone()),
                host: String::from("127.0.0.1"),
                port,
                hostname: None,
                route_url: None,
                health_url: None,
            })
            .expect("reservation");
    }

    assert!(matches!(
        registry.select_service(&identity),
        Err(RegistryError::AmbiguousService { .. })
    ));
}

#[test]
fn batch_reservation_is_idempotent_and_preserves_existing_services() {
    let mut registry = Registry::open(temp_registry_path("batch-idempotent")).expect("registry");
    let identities = vec![
        scoped_identity("example", "web", "worktree"),
        scoped_identity("example", "api", "worktree"),
    ];
    let mut planned = 0;
    let first = registry
        .reserve_services(&identities, |identity, occupied, _| {
            planned += 1;
            let port = if identity.service == "web" {
                29_514
            } else {
                29_515
            };
            assert!(!occupied.contains(&port));
            Ok::<_, ()>(candidate(port))
        })
        .expect("first batch");
    assert_eq!(planned, 2);

    let second = registry
        .reserve_services(&identities, |_, _, _| -> Result<ReservationCandidate, ()> {
            panic!("existing services must not be replanned")
        })
        .expect("second batch");

    assert_eq!(second, first);
    assert!(second.iter().all(|service| service.state == "reserved"));
}

#[test]
fn batch_reservation_preserves_active_services_and_allocates_only_missing_services() {
    let mut registry = Registry::open(temp_registry_path("batch-active")).expect("registry");
    let web = scoped_identity("example", "web", "worktree");
    let api = scoped_identity("example", "api", "worktree");
    registry
        .record_run_started(&RunStart {
            project: web.project.clone(),
            service: web.service.clone(),
            identity: Some(web.clone()),
            host: String::from("127.0.0.1"),
            port: 29_520,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("active web");

    let services = registry
        .reserve_services(&[web, api], |identity, occupied, _| {
            assert_eq!(identity.service, "api");
            assert!(occupied.contains(&29_520));
            Ok::<_, ()>(candidate(29_521))
        })
        .expect("batch");

    assert_eq!(services[0].state, "active");
    assert_eq!(services[0].port, 29_520);
    assert_eq!(services[1].state, "reserved");
    assert_eq!(services[1].port, 29_521);
}

#[test]
fn batch_reservation_rolls_back_every_new_service_on_plan_failure() {
    let mut registry = Registry::open(temp_registry_path("batch-rollback")).expect("registry");
    let identities = vec![
        scoped_identity("example", "web", "worktree"),
        scoped_identity("example", "api", "worktree"),
    ];
    let mut planned = 0;

    let result = registry.reserve_services(&identities, |_, _, _| {
        planned += 1;
        if planned == 2 {
            Err("planned failure")
        } else {
            Ok(candidate(29_516))
        }
    });

    assert!(matches!(result, Err(BatchReservationError::Plan(_))));
    assert!(
        registry
            .status_snapshot()
            .expect("status")
            .services
            .is_empty()
    );
}

#[test]
fn batch_reservation_rolls_back_on_late_port_conflict() {
    let mut registry = Registry::open(temp_registry_path("batch-port-conflict")).expect("registry");
    let identities = vec![
        scoped_identity("example", "web", "worktree"),
        scoped_identity("example", "api", "worktree"),
    ];

    let result = registry.reserve_services(&identities, |_, _, _| Ok::<_, ()>(candidate(29_522)));

    assert!(matches!(
        result,
        Err(BatchReservationError::Registry(
            RegistryError::PortConflict { port: 29_522 }
        ))
    ));
    assert!(
        registry
            .status_snapshot()
            .expect("status")
            .services
            .is_empty()
    );
}

#[test]
fn concurrent_batch_reservations_reuse_one_atomic_result() {
    let path = temp_registry_path("batch-concurrent");
    let first_registry = Registry::open(&path).expect("first registry");
    let second_registry = Registry::open(&path).expect("second registry");
    let identities = Arc::new(vec![
        scoped_identity("example", "web", "worktree"),
        scoped_identity("example", "api", "worktree"),
    ]);
    let barrier = Arc::new(Barrier::new(2));

    let run =
        |mut registry: Registry, identities: Arc<Vec<ServiceIdentity>>, barrier: Arc<Barrier>| {
            std::thread::spawn(move || {
                barrier.wait();
                registry
                    .reserve_services(&identities, |identity, _, _| {
                        Ok::<_, ()>(candidate(if identity.service == "web" {
                            29_517
                        } else {
                            29_518
                        }))
                    })
                    .expect("concurrent batch")
            })
        };
    let first = run(
        first_registry,
        Arc::clone(&identities),
        Arc::clone(&barrier),
    );
    let second = run(second_registry, identities, barrier);

    assert_eq!(
        first.join().expect("first thread"),
        second.join().expect("second thread")
    );
    let mut registry = Registry::open(&path).expect("final registry");
    assert_eq!(
        registry.status_snapshot().expect("status").services.len(),
        2
    );
}

#[test]
fn promotion_is_atomic_and_preserves_reservation_identity_and_metadata() {
    let mut registry = Registry::open(temp_registry_path("promotion")).expect("registry");
    let identity = scoped_identity("example", "web", "worktree");
    let reserved = registry
        .record_reserved_lease(&ReserveLease {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_519,
            hostname: Some(String::from("web.localhost")),
            route_url: Some(String::from("http://web.localhost")),
            health_url: Some(String::from("http://web.localhost/health")),
        })
        .expect("reservation");

    assert!(matches!(
        registry.promote_reserved_lease(&ReservedRunStart {
            lease_id: reserved.lease_id + 1,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        }),
        Err(RegistryError::ReservationNotFound { .. })
    ));
    assert_eq!(
        registry
            .select_service(&identity)
            .expect("still reserved")
            .state,
        "reserved"
    );

    let started = registry
        .promote_reserved_lease(&ReservedRunStart {
            lease_id: reserved.lease_id,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: env::temp_dir(),
        })
        .expect("promotion");
    assert_eq!(started.lease_id, reserved.lease_id);

    let active = registry.select_service(&identity).expect("active service");
    assert_eq!(active.lease_id, reserved.lease_id);
    assert_eq!(active.state, "active");
    assert_eq!(active.port, 29_519);
    assert_eq!(active.route_url.as_deref(), Some("http://web.localhost"));
    let snapshot = registry.status_snapshot().expect("status");
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.runs[0].lease_id, reserved.lease_id);
}
