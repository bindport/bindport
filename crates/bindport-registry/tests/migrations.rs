// SPDX-License-Identifier: MIT

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use bindport_registry::{REGISTRY_USER_VERSION, Registry, RegistryError, RegistryExportSnapshot};
use rusqlite::Connection;

struct HistoricalFixture {
    name: &'static str,
    sql: &'static str,
    source_version: i64,
    has_route_metadata: bool,
    has_health_metadata: bool,
    has_outputs: bool,
    has_reservation: bool,
    has_process_start_time: bool,
    has_scoped_outputs: bool,
}

const HISTORICAL_FIXTURES: &[HistoricalFixture] = &[
    HistoricalFixture {
        name: "v0.1.0-user-version-2",
        sql: include_str!("fixtures/v0.1.0-user-version-2.sql"),
        source_version: 2,
        has_route_metadata: false,
        has_health_metadata: false,
        has_outputs: false,
        has_reservation: false,
        has_process_start_time: false,
        has_scoped_outputs: false,
    },
    HistoricalFixture {
        name: "v0.2.0-user-version-3",
        sql: include_str!("fixtures/v0.2.0-user-version-3.sql"),
        source_version: 3,
        has_route_metadata: true,
        has_health_metadata: false,
        has_outputs: false,
        has_reservation: false,
        has_process_start_time: false,
        has_scoped_outputs: false,
    },
    HistoricalFixture {
        name: "v0.3.0-v0.4.0-user-version-6",
        sql: include_str!("fixtures/v0.3.0-v0.4.0-user-version-6.sql"),
        source_version: 6,
        has_route_metadata: true,
        has_health_metadata: false,
        has_outputs: true,
        has_reservation: false,
        has_process_start_time: false,
        has_scoped_outputs: false,
    },
    HistoricalFixture {
        name: "v0.5.0-user-version-7",
        sql: include_str!("fixtures/v0.5.0-user-version-7.sql"),
        source_version: 7,
        has_route_metadata: true,
        has_health_metadata: true,
        has_outputs: true,
        has_reservation: false,
        has_process_start_time: false,
        has_scoped_outputs: false,
    },
    HistoricalFixture {
        name: "v0.5.1-v0.6.x-user-version-8",
        sql: include_str!("fixtures/v0.5.1-v0.6.x-user-version-8.sql"),
        source_version: 8,
        has_route_metadata: true,
        has_health_metadata: true,
        has_outputs: true,
        has_reservation: true,
        has_process_start_time: true,
        has_scoped_outputs: false,
    },
    HistoricalFixture {
        name: "v0.7.0-current-user-version-9",
        sql: include_str!("fixtures/v0.7.0-current-user-version-9.sql"),
        source_version: 9,
        has_route_metadata: true,
        has_health_metadata: true,
        has_outputs: true,
        has_reservation: true,
        has_process_start_time: true,
        has_scoped_outputs: true,
    },
];

#[test]
fn every_shipped_registry_schema_migrates_without_losing_state() {
    for fixture in HISTORICAL_FIXTURES {
        let root = fixture_root(fixture.name);
        let registry_path = install_fixture(&root, fixture);

        let mut registry = Registry::open(&registry_path)
            .unwrap_or_else(|error| panic!("{} did not migrate: {error}", fixture.name));
        verify_migrated_registry(&mut registry, &root, fixture);
        drop(registry);

        let mut reopened = Registry::open(&registry_path)
            .unwrap_or_else(|error| panic!("{} did not reopen: {error}", fixture.name));
        verify_migrated_registry(&mut reopened, &root, fixture);
    }
}

#[test]
fn migration_failure_rolls_back_all_schema_and_version_changes() {
    let root = fixture_root("migration-rollback");
    let registry_path = root.join("registry.sqlite");
    let connection = Connection::open(&registry_path).expect("partial registry");
    connection
        .execute_batch(
            "
            CREATE TABLE leases (
                id INTEGER PRIMARY KEY,
                project TEXT NOT NULL,
                service TEXT NOT NULL,
                worktree_path TEXT,
                worktree_hash TEXT,
                git_common_dir TEXT,
                branch TEXT,
                branch_label TEXT,
                git_commit TEXT,
                identity_key TEXT,
                port INTEGER NOT NULL,
                host TEXT NOT NULL,
                state TEXT NOT NULL,
                allocated_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                released_at TEXT
            );
            CREATE INDEX leases_state_port_idx ON leases(state, port);
            CREATE INDEX leases_identity_key_idx ON leases(identity_key);
            CREATE TABLE runs (
                id INTEGER PRIMARY KEY,
                lease_id INTEGER NOT NULL REFERENCES leases(id),
                pid INTEGER NOT NULL,
                command TEXT NOT NULL,
                cwd TEXT NOT NULL,
                started_at TEXT NOT NULL,
                exited_at TEXT,
                exit_code INTEGER
            );
            CREATE INDEX runs_lease_id_idx ON runs(lease_id);
            CREATE TABLE output_files (
                id INTEGER PRIMARY KEY,
                output_name TEXT NOT NULL,
                output_scope TEXT NOT NULL,
                route_key TEXT NOT NULL
            );
            INSERT INTO leases VALUES (
                1, 'rollback-project', 'active', NULL, NULL, NULL, NULL, NULL,
                NULL, 'v1:rollback', 29111, '127.0.0.1', 'active',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL
            );
            INSERT INTO output_files VALUES (1, 'broken', 'scope', 'v1:rollback');
            PRAGMA user_version = 7;
            ",
        )
        .expect("create partial registry");
    let lease_columns_before = table_columns(&connection, "leases");
    let run_columns_before = table_columns(&connection, "runs");
    drop(connection);

    let error = match Registry::open(&registry_path) {
        Ok(_) => panic!("partial schema migration should fail"),
        Err(error) => error,
    };
    assert!(matches!(error, RegistryError::Sqlite(_)));

    let connection = Connection::open(&registry_path).expect("reopen partial registry");
    assert_eq!(user_version(&connection), 7);
    assert_eq!(table_columns(&connection, "leases"), lease_columns_before);
    assert_eq!(table_columns(&connection, "runs"), run_columns_before);
    assert_eq!(row_count(&connection, "leases"), 1);
    assert_eq!(row_count(&connection, "output_files"), 1);
    assert_eq!(row_count(&connection, "output_render_state"), 0);
    assert_eq!(
        connection
            .query_row("SELECT identity_key FROM leases WHERE id = 1", [], |row| {
                row.get::<_, String>(0)
            })
            .expect("preserved lease"),
        "v1:rollback"
    );
}

#[test]
fn newer_registry_version_is_rejected_without_rewriting_it() {
    let root = fixture_root("future-version");
    let registry_path = root.join("registry.sqlite");
    let connection = Connection::open(&registry_path).expect("future registry");
    connection
        .execute_batch(
            "
            CREATE TABLE future_marker (value TEXT NOT NULL);
            INSERT INTO future_marker VALUES ('preserve-me');
            PRAGMA user_version = 10;
            ",
        )
        .expect("seed future registry");
    drop(connection);

    let error = match Registry::open(&registry_path) {
        Ok(_) => panic!("future registry should be rejected"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        RegistryError::UnsupportedRegistryVersion {
            found: 10,
            supported: REGISTRY_USER_VERSION,
            ..
        }
    ));

    let connection = Connection::open(&registry_path).expect("inspect future registry");
    assert_eq!(user_version(&connection), 10);
    assert_eq!(row_count(&connection, "future_marker"), 1);
    assert_eq!(row_count(&connection, "leases"), 0);
}

#[test]
fn concurrent_opens_apply_one_idempotent_migration() {
    let fixture = HISTORICAL_FIXTURES
        .iter()
        .find(|fixture| fixture.source_version == 8)
        .expect("version 8 fixture");
    let root = fixture_root("concurrent-migration");
    let registry_path = install_fixture(&root, fixture);
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let registry_path = registry_path.clone();
            thread::spawn(move || {
                barrier.wait();
                Registry::open(registry_path).map(|_| ())
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle
            .join()
            .expect("migration thread")
            .expect("concurrent registry open");
    }

    let mut registry = Registry::open(&registry_path).expect("reopen concurrent registry");
    verify_migrated_registry(&mut registry, &root, fixture);
}

fn verify_migrated_registry(registry: &mut Registry, root: &Path, fixture: &HistoricalFixture) {
    let export = registry.export_snapshot().expect("registry export");
    verify_export(&export, root, fixture);

    let snapshot = registry.status_snapshot().expect("status snapshot");
    assert_eq!(snapshot.schema_version, "1.0");
    assert_eq!(
        snapshot.services.len(),
        if fixture.has_reservation { 3 } else { 2 }
    );
    assert_eq!(snapshot.runs.len(), 2);

    let active = snapshot
        .services
        .iter()
        .find(|service| service.identity_key.as_deref() == Some("v1:migration-active"))
        .expect("active service remains visible");
    assert_eq!(active.state, "active");
    assert_eq!(active.port, 29101);
    assert_eq!(active.pid, Some(std::process::id()));
    assert_eq!(active.worktree_hash.as_deref(), Some("active-hash"));
    assert_eq!(
        active.hostname.as_deref(),
        fixture.has_route_metadata.then_some("active.localhost")
    );
    assert_eq!(
        active.health_url.as_deref(),
        fixture
            .has_health_metadata
            .then_some("https://active.localhost/health")
    );
    assert_eq!(active.outputs.len(), usize::from(fixture.has_outputs));
    if fixture.has_outputs {
        assert_eq!(snapshot.outputs.len(), 2);
        assert_eq!(active.outputs[0].name, "traefik");
        assert_eq!(active.outputs[0].status, "rendered");
        assert!(
            active
                .proxy
                .as_ref()
                .expect("preserved proxy output")
                .rendered
        );
    } else {
        assert!(snapshot.outputs.is_empty());
        assert!(active.proxy.is_none());
    }

    let stopped = snapshot
        .services
        .iter()
        .find(|service| service.identity_key.as_deref() == Some("v1:migration-stopped"))
        .expect("stopped service remains visible");
    assert_eq!(stopped.state, "stopped");
    assert_eq!(stopped.port, 29102);
    assert_eq!(stopped.exit_code, Some(0));

    if fixture.has_reservation {
        let reserved = snapshot
            .services
            .iter()
            .find(|service| service.identity_key.as_deref() == Some("v1:migration-reserved"))
            .expect("reserved service remains visible");
        assert_eq!(reserved.state, "reserved");
        assert_eq!(reserved.port, 29103);
        assert_eq!(reserved.pid, None);
        assert_eq!(reserved.command, "reserved");
        assert_eq!(reserved.cwd, "");
        assert_eq!(reserved.hostname.as_deref(), Some("reserved.localhost"));
        assert_eq!(
            reserved.route_url.as_deref(),
            Some("http://reserved.localhost")
        );
        assert_eq!(
            reserved.health_url.as_deref(),
            Some("https://reserved.localhost/health")
        );
    }
}

fn verify_export(export: &RegistryExportSnapshot, root: &Path, fixture: &HistoricalFixture) {
    assert_eq!(export.user_version, REGISTRY_USER_VERSION);
    assert_eq!(
        export.leases.len(),
        if fixture.has_reservation { 3 } else { 2 }
    );
    assert_eq!(export.runs.len(), 2);
    assert_eq!(
        export.output_files.len(),
        if fixture.has_outputs { 2 } else { 0 }
    );
    assert_eq!(
        export.output_render_state.len(),
        usize::from(fixture.has_outputs)
    );
    if fixture.has_outputs {
        assert_eq!(export.output_render_state[0].output_name, "traefik");
        assert_eq!(
            export.output_render_state[0].last_render_at_ms,
            1_767_225_780_000
        );
    }

    let active = export
        .leases
        .iter()
        .find(|lease| lease.identity_key.as_deref() == Some("v1:migration-active"))
        .expect("active lease");
    assert_eq!(active.state, "active");
    assert_eq!(active.port, 29101);
    assert_eq!(active.host, "127.0.0.1");
    assert_eq!(
        active.worktree_path.as_deref(),
        Some(path(root, "active").as_str())
    );
    assert_eq!(active.worktree_hash.as_deref(), Some("active-hash"));
    assert_eq!(active.branch.as_deref(), Some("feature/migrations"));
    assert_eq!(
        active.hostname.as_deref(),
        fixture.has_route_metadata.then_some("active.localhost")
    );
    assert_eq!(
        active.route_url.as_deref(),
        fixture
            .has_route_metadata
            .then_some("http://active.localhost")
    );
    assert_eq!(
        active.health_url.as_deref(),
        fixture
            .has_health_metadata
            .then_some("https://active.localhost/health")
    );

    let active_run = export
        .runs
        .iter()
        .find(|run| run.id == 201)
        .expect("active run");
    assert_eq!(active_run.lease_id, 101);
    assert_eq!(active_run.pid, std::process::id());
    assert_eq!(active_run.exited_at, None);
    assert_eq!(active_run.exit_code, None);
    assert_eq!(active_run.process_start_time, None);

    let stopped_run = export
        .runs
        .iter()
        .find(|run| run.id == 202)
        .expect("stopped run");
    assert_eq!(stopped_run.exit_code, Some(0));
    assert_eq!(
        stopped_run.process_start_time,
        fixture.has_process_start_time.then_some(4242)
    );

    if fixture.has_outputs {
        let output = export
            .output_files
            .iter()
            .find(|output| output.id == 301)
            .expect("active output");
        assert_eq!(output.route_key, "v1:migration-active");
        assert_eq!(output.status, "rendered");
        assert_eq!(output.content_hash.as_deref(), Some("active-content"));
        assert_eq!(
            output.output_scope,
            if fixture.has_scoped_outputs {
                "fixture-scope"
            } else {
                "unscoped"
            }
        );
        assert_eq!(
            output.output_root.as_deref(),
            fixture
                .has_scoped_outputs
                .then(|| path(root, "outputs"))
                .as_deref()
        );
    }
}

fn install_fixture(root: &Path, fixture: &HistoricalFixture) -> PathBuf {
    for directory in ["active", "stopped", "reserved", "outputs", ".git"] {
        fs::create_dir_all(root.join(directory)).expect("fixture directory");
    }
    let registry_path = root.join("registry.sqlite");
    let sql = fixture
        .sql
        .replace("__ROOT__", &sql_literal(root.to_string_lossy().as_ref()))
        .replace("__TEST_PID__", &std::process::id().to_string())
        .replace("__TEST_COMMAND__", &sql_literal(&current_process_command()));
    let connection = Connection::open(&registry_path).expect("fixture registry");
    connection
        .execute_batch(&sql)
        .unwrap_or_else(|error| panic!("failed to install {}: {error}", fixture.name));
    assert_eq!(user_version(&connection), fixture.source_version);
    drop(connection);

    registry_path
}

fn fixture_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bindport-migration-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("fixture root");
    root.canonicalize().expect("canonical fixture root")
}

fn current_process_command() -> String {
    std::env::current_exe()
        .expect("current test executable")
        .file_name()
        .expect("test executable name")
        .to_string_lossy()
        .into_owned()
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn path(root: &Path, suffix: &str) -> String {
    root.join(suffix).display().to_string()
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user_version")
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table info");
    statement
        .query_map([], |row| row.get(1))
        .expect("table columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns")
}

fn row_count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap_or(0)
}
