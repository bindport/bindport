// SPDX-License-Identifier: MIT

use super::*;
use std::{cell::RefCell, sync::Barrier};

thread_local! {
    static BLOCKING_WRITER: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn with_registry(test: impl FnOnce(&mut Registry)) {
    let directory = TestDirectory(temp_registry_path("contention").with_extension(""));
    fs::create_dir(&directory.0).expect("test directory");
    let path = directory
        .0
        .canonicalize()
        .expect("canonical directory")
        .join("registry.sqlite");
    let mut registry = Registry::open(path).expect("registry");
    test(&mut registry);
}

fn begin_writer(registry: &Registry) -> Connection {
    let writer = Connection::open(registry.path()).expect("writer connection");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold writer");
    writer
}

fn with_blocking_writer<T>(
    registry: &mut Registry,
    writer: Connection,
    operation: impl FnOnce(&mut Registry) -> Result<T, RegistryError>,
) -> (T, bool) {
    BLOCKING_WRITER.with(|slot| *slot.borrow_mut() = Some(writer));
    // Read-to-write upgrades skip the busy handler. Release the competing writer
    // only from that handler so the regression does not depend on thread timing.
    registry
        .connection
        .busy_handler(Some(|_| {
            BLOCKING_WRITER.with(|slot| {
                slot.borrow_mut()
                    .take()
                    .is_some_and(|writer| writer.execute_batch("COMMIT").is_ok())
            })
        }))
        .expect("busy handler");
    let result = operation(registry);
    let waited = BLOCKING_WRITER.with(|slot| slot.borrow_mut().take().is_none());
    registry
        .connection
        .busy_timeout(REGISTRY_BUSY_TIMEOUT)
        .expect("restore timeout");
    (result.expect("operation under contention"), waited)
}

fn stopped_run(registry: &mut Registry, service: &str) -> StartedRun {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test-owned port");
    let port = listener.local_addr().expect("listener address").port();
    let run = registry
        .record_run_started(&test_run_start(
            "contention",
            service,
            port,
            std::process::id(),
        ))
        .expect("start run");
    registry
        .record_run_finished(run, Some(0))
        .expect("stop run");
    run
}

#[test]
fn auto_render_waits_for_writer_before_reading_debounce_state() {
    with_registry(|registry| {
        let writer = begin_writer(registry);
        writer
            .execute(
                "INSERT INTO output_render_state VALUES ('routes', 1000)",
                [],
            )
            .expect("pending reservation");
        let (delay, waited) = with_blocking_writer(registry, writer, |registry| {
            registry.reserve_auto_render_at("routes", 250, 1_100)
        });
        assert!(waited);
        assert_eq!(delay, Duration::from_millis(150));
        let next = registry
            .reserve_auto_render_at("routes", 250, 1_100)
            .expect("next reservation");
        assert_eq!(next, Duration::from_millis(400));
    });
}

#[test]
fn clean_leases_waits_for_writer_before_counting_and_deleting() {
    with_registry(|registry| {
        stopped_run(registry, "web");
        let writer = begin_writer(registry);
        let (summary, waited) = with_blocking_writer(registry, writer, |registry| {
            registry.clean_leases(&[CleanState::Stopped], false)
        });
        assert!(waited);
        assert_eq!(summary.stopped_leases, 1);
        assert_eq!(summary.runs, 1);
        let snapshot = registry.status_snapshot().expect("snapshot");
        assert!(snapshot.services.is_empty());
        assert!(snapshot.runs.is_empty());
    });
}

#[test]
fn prune_stale_leases_waits_for_writer_before_selecting_and_deleting() {
    with_registry(|registry| {
        stopped_run(registry, "web");
        registry
            .connection
            .execute("UPDATE leases SET state = 'stale'", [])
            .expect("mark stale");
        let writer = begin_writer(registry);
        let (summary, waited) = with_blocking_writer(registry, writer, |registry| {
            registry.prune_oldest_stale_leases(0, u16::MAX, 0, false)
        });
        assert!(waited);
        assert_eq!(summary.stale_leases, 1);
        assert_eq!(summary.runs, 1);
        let snapshot = registry.status_snapshot().expect("snapshot");
        assert!(snapshot.services.is_empty());
        assert!(snapshot.runs.is_empty());
    });
}

#[test]
fn prune_pressure_count_includes_the_preceding_writers_deletions() {
    with_registry(|registry| {
        let first = stopped_run(registry, "web");
        stopped_run(registry, "api");
        registry
            .connection
            .execute("UPDATE leases SET state = 'stale'", [])
            .expect("mark stale");
        let writer = begin_writer(registry);
        writer
            .execute("DELETE FROM runs WHERE lease_id = ?1", [first.lease_id])
            .expect("competing run cleanup");
        writer
            .execute("DELETE FROM leases WHERE id = ?1", [first.lease_id])
            .expect("competing lease cleanup");
        let (summary, waited) = with_blocking_writer(registry, writer, |registry| {
            registry.prune_oldest_stale_leases(0, u16::MAX, 1, false)
        });
        assert!(waited);
        assert_eq!(summary, CleanSummary::default());
        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.runs.len(), 1);
    });
}

#[test]
fn cleanup_dry_runs_do_not_wait_for_a_writer_when_no_reconciliation_is_needed() {
    with_registry(|registry| {
        stopped_run(registry, "web");
        let writer = begin_writer(registry);
        let (summary, waited) = with_blocking_writer(registry, writer, |registry| {
            registry.clean_leases(&[CleanState::Stopped], true)
        });
        assert!(!waited);
        assert_eq!(summary.stopped_leases, 1);
        assert_eq!(summary.runs, 1);

        registry
            .connection
            .execute("UPDATE leases SET state = 'stale'", [])
            .expect("mark stale");
        let writer = begin_writer(registry);
        let (summary, waited) = with_blocking_writer(registry, writer, |registry| {
            registry.prune_oldest_stale_leases(0, u16::MAX, 0, true)
        });
        assert!(!waited);
        assert_eq!(summary.stale_leases, 1);
        assert_eq!(summary.runs, 1);
        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.runs.len(), 1);
    });
}

#[test]
fn concurrent_auto_render_reservations_receive_distinct_debounce_slots() {
    with_registry(|registry| {
        const WORKERS: usize = 8;
        const ROUNDS: usize = 20;
        let connections = (0..WORKERS)
            .map(|_| Registry::open(registry.path()).expect("worker registry"))
            .collect::<Vec<_>>();
        let barrier = Barrier::new(WORKERS);
        let mut delays = thread::scope(|scope| {
            let workers = connections
                .into_iter()
                .map(|mut registry| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        (0..ROUNDS)
                            .map(|_| {
                                barrier.wait();
                                registry.reserve_auto_render_at("routes", 250, 1_000)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("worker"))
                .collect::<Result<Vec<_>, _>>()
                .expect("all concurrent reservations succeed")
        });
        delays.sort_unstable();
        let expected = (0..WORKERS * ROUNDS)
            .map(|slot| Duration::from_millis(slot as u64 * 250))
            .collect::<Vec<_>>();
        assert_eq!(delays, expected);
        let last_ms: i64 = registry
            .connection
            .query_row(
                "SELECT last_render_at_ms FROM output_render_state WHERE output_name = 'routes'",
                [],
                |row| row.get(0),
            )
            .expect("persisted schedule");
        assert_eq!(last_ms, 1_000 + (WORKERS * ROUNDS - 1) as i64 * 250);
    });
}
