// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn output_file_ownership_returns_rendered_files_with_hashes() {
    let mut registry = Registry::open(temp_registry_path("output-files")).expect("registry");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: test_output_scope("/tmp/bindport"),
            route_key: String::from("route-1"),
            rendered_path: PathBuf::from("/tmp/bindport/route-1.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("hash-1")),
            template_hash: Some(String::from("template-1")),
            lease_id: None,
            run_id: None,
        })
        .expect("record rendered file");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: test_output_scope("/tmp/bindport"),
            route_key: String::from("route-2"),
            rendered_path: PathBuf::from("/tmp/bindport/route-2.yml"),
            status: OutputFileStatus::Error,
            reason: Some(String::from("template_error")),
            content_hash: None,
            template_hash: Some(String::from("template-1")),
            lease_id: None,
            run_id: None,
        })
        .expect("record error file");

    let ownership = registry
        .output_file_ownership("traefik", &test_output_scope("/tmp/bindport"))
        .expect("ownership records");

    assert_eq!(
        ownership,
        vec![OutputFileOwnership {
            route_key: String::from("route-1"),
            path: PathBuf::from("/tmp/bindport/route-1.yml"),
            content_hash: String::from("hash-1")
        }]
    );

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.outputs.len(), 1);
    assert_eq!(snapshot.outputs[0].name, "traefik");
    assert_eq!(snapshot.outputs[0].rendered, 1);
    assert_eq!(snapshot.outputs[0].error, 1);
}

#[test]
fn output_file_ownership_keeps_external_modified_expected_hashes() {
    let mut registry =
        Registry::open(temp_registry_path("output-files-modified")).expect("registry");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: test_output_scope("/tmp/bindport"),
            route_key: String::from("route-1"),
            rendered_path: PathBuf::from("/tmp/bindport/route-1.yml"),
            status: OutputFileStatus::Error,
            reason: Some(String::from("external_modified")),
            content_hash: Some(String::from("hash-1")),
            template_hash: Some(String::from("template-1")),
            lease_id: None,
            run_id: None,
        })
        .expect("record external modification");

    let ownership = registry
        .output_file_ownership("traefik", &test_output_scope("/tmp/bindport"))
        .expect("ownership records");

    assert_eq!(
        ownership,
        vec![OutputFileOwnership {
            route_key: String::from("route-1"),
            path: PathBuf::from("/tmp/bindport/route-1.yml"),
            content_hash: String::from("hash-1")
        }]
    );
}

#[test]
fn record_output_file_upserts_by_output_scope_and_route() {
    let mut registry = Registry::open(temp_registry_path("output-files-upsert")).expect("registry");
    let mut record = OutputFileRecord {
        output_name: String::from("traefik"),
        scope: test_output_scope("/tmp/bindport"),
        route_key: String::from("route-1"),
        rendered_path: PathBuf::from("/tmp/bindport/old.yml"),
        status: OutputFileStatus::Rendered,
        reason: None,
        content_hash: Some(String::from("old-hash")),
        template_hash: Some(String::from("template-1")),
        lease_id: Some(1),
        run_id: Some(2),
    };

    registry
        .record_output_file(&record)
        .expect("record first file");
    record.rendered_path = PathBuf::from("/tmp/bindport/new.yml");
    record.content_hash = Some(String::from("new-hash"));
    registry
        .record_output_file(&record)
        .expect("record updated file");

    let ownership = registry
        .output_file_ownership("traefik", &test_output_scope("/tmp/bindport"))
        .expect("ownership records");

    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership[0].path, PathBuf::from("/tmp/bindport/new.yml"));
    assert_eq!(ownership[0].content_hash, "new-hash");
}

#[test]
fn output_file_ownership_is_scoped_by_output_root() {
    let mut registry = Registry::open(temp_registry_path("output-files-scoped")).expect("registry");
    let first_scope = test_output_scope("/tmp/bindport/worktree-a/.bindport/generated");
    let second_scope = test_output_scope("/tmp/bindport/worktree-b/.bindport/generated");

    for (scope, path, hash) in [
        (
            first_scope.clone(),
            "/tmp/bindport/worktree-a/.bindport/generated/route.yml",
            "hash-a",
        ),
        (
            second_scope.clone(),
            "/tmp/bindport/worktree-b/.bindport/generated/route.yml",
            "hash-b",
        ),
    ] {
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                scope,
                route_key: String::from("route-1"),
                rendered_path: PathBuf::from(path),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from(hash)),
                template_hash: None,
                lease_id: None,
                run_id: None,
            })
            .expect("record scoped file");
    }

    let first_ownership = registry
        .output_file_ownership("traefik", &first_scope)
        .expect("first scope ownership");
    let second_ownership = registry
        .output_file_ownership("traefik", &second_scope)
        .expect("second scope ownership");

    assert_eq!(first_ownership.len(), 1);
    assert_eq!(
        first_ownership[0].path,
        PathBuf::from("/tmp/bindport/worktree-a/.bindport/generated/route.yml")
    );
    assert_eq!(first_ownership[0].content_hash, "hash-a");
    assert_eq!(second_ownership.len(), 1);
    assert_eq!(
        second_ownership[0].path,
        PathBuf::from("/tmp/bindport/worktree-b/.bindport/generated/route.yml")
    );
    assert_eq!(second_ownership[0].content_hash, "hash-b");
}

#[test]
fn legacy_unscoped_rows_are_adopted_only_within_current_root() {
    let mut registry = Registry::open(temp_registry_path("output-files-legacy")).expect("registry");
    let current_scope = test_output_scope("/tmp/bindport/current/.bindport/generated");

    for (path, hash) in [
        (
            "/tmp/bindport/current/.bindport/generated/route.yml",
            "current-hash",
        ),
        (
            "/tmp/bindport/deleted/.bindport/generated/route.yml",
            "foreign-hash",
        ),
    ] {
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                scope: OutputFileScope::unscoped(),
                route_key: format!("route-{hash}"),
                rendered_path: PathBuf::from(path),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from(hash)),
                template_hash: None,
                lease_id: None,
                run_id: None,
            })
            .expect("record legacy file");
    }

    let ownership = registry
        .output_file_ownership("traefik", &current_scope)
        .expect("ownership records");

    assert_eq!(ownership.len(), 1);
    assert_eq!(
        ownership[0].path,
        PathBuf::from("/tmp/bindport/current/.bindport/generated/route.yml")
    );
    assert_eq!(ownership[0].content_hash, "current-hash");
}

#[test]
fn opening_old_registry_migrates_output_rows_to_unscoped_scope() {
    let path = temp_registry_path("output-files-scope-migration");
    {
        let connection = Connection::open(&path).expect("old registry connection");
        connection
            .execute_batch(
                "
                CREATE TABLE output_files (
                    id INTEGER PRIMARY KEY,
                    output_name TEXT NOT NULL,
                    route_key TEXT NOT NULL,
                    rendered_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    reason TEXT,
                    content_hash TEXT,
                    template_hash TEXT,
                    lease_id INTEGER,
                    run_id INTEGER,
                    rendered_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(output_name, route_key)
                );

                INSERT INTO output_files (
                    output_name, route_key, rendered_path, status, reason,
                    content_hash, template_hash, lease_id, run_id, rendered_at,
                    updated_at
                ) VALUES (
                    'traefik', 'route-1',
                    '/tmp/bindport/current/.bindport/generated/route.yml',
                    'rendered', NULL, 'legacy-hash', NULL, NULL, NULL,
                    '2026-07-08T00:00:00Z', '2026-07-08T00:00:00Z'
                );
                ",
            )
            .expect("create old output table");
    }

    let registry = Registry::open(&path).expect("migrated registry");
    let ownership = registry
        .output_file_ownership(
            "traefik",
            &test_output_scope("/tmp/bindport/current/.bindport/generated"),
        )
        .expect("ownership records");

    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership[0].content_hash, "legacy-hash");
}

#[test]
fn recording_scoped_file_claims_matching_legacy_row() {
    let path = temp_registry_path("output-files-claim-legacy");
    {
        let connection = Connection::open(&path).expect("old registry connection");
        connection
            .execute_batch(
                "
                CREATE TABLE output_files (
                    id INTEGER PRIMARY KEY,
                    output_name TEXT NOT NULL,
                    route_key TEXT NOT NULL,
                    rendered_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    reason TEXT,
                    content_hash TEXT,
                    template_hash TEXT,
                    lease_id INTEGER,
                    run_id INTEGER,
                    rendered_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(output_name, route_key)
                );

                INSERT INTO output_files (
                    output_name, route_key, rendered_path, status, reason,
                    content_hash, template_hash, lease_id, run_id, rendered_at,
                    updated_at
                ) VALUES (
                    'traefik', 'route-1',
                    '/tmp/bindport/current/.bindport/generated/route.yml',
                    'rendered', NULL, 'legacy-hash', NULL, NULL, NULL,
                    '2026-07-08T00:00:00Z', '2026-07-08T00:00:00Z'
                );
                ",
            )
            .expect("create old output table");
    }

    let mut registry = Registry::open(&path).expect("migrated registry");
    let scope = test_output_scope("/tmp/bindport/current/.bindport/generated");
    let mut record = OutputFileRecord {
        output_name: String::from("traefik"),
        scope: scope.clone(),
        route_key: String::from("route-1"),
        rendered_path: PathBuf::from("/tmp/bindport/current/.bindport/generated/route.yml"),
        status: OutputFileStatus::Rendered,
        reason: None,
        content_hash: Some(String::from("scoped-hash")),
        template_hash: None,
        lease_id: None,
        run_id: None,
    };

    registry
        .record_output_file(&record)
        .expect("record scoped file");
    record.content_hash = Some(String::from("rerendered-hash"));
    registry
        .record_output_file(&record)
        .expect("record rerendered scoped file");

    let ownership = registry
        .output_file_ownership("traefik", &scope)
        .expect("ownership records");
    let row_count = registry
        .connection
        .query_row(
            "SELECT COUNT(*)
             FROM output_files
             WHERE output_name = 'traefik' AND route_key = 'route-1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("row count");

    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership[0].content_hash, "rerendered-hash");
    assert_eq!(row_count, 1);
}

#[test]
fn ownership_prefers_exact_scope_when_duplicate_legacy_path_exists() {
    let mut registry =
        Registry::open(temp_registry_path("output-files-dedupe-legacy")).expect("registry");
    let scope = test_output_scope("/tmp/bindport/current/.bindport/generated");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: scope.clone(),
            route_key: String::from("route-1"),
            rendered_path: PathBuf::from("/tmp/bindport/current/.bindport/generated/route.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("scoped-hash")),
            template_hash: None,
            lease_id: None,
            run_id: None,
        })
        .expect("record scoped file");
    registry
        .connection
        .execute(
            "INSERT INTO output_files (
                output_name, output_scope, route_key, rendered_path, status,
                reason, content_hash, template_hash, lease_id, run_id,
                rendered_at, updated_at
             ) VALUES (
                'traefik', 'unscoped', 'route-1',
                '/tmp/bindport/current/.bindport/generated/route.yml',
                'rendered', NULL, 'legacy-hash', NULL, NULL, NULL,
                '2026-07-08T00:00:00Z', '2026-07-08T00:00:00Z'
             )",
            [],
        )
        .expect("insert duplicate legacy path");

    let ownership = registry
        .output_file_ownership("traefik", &scope)
        .expect("ownership records");

    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership[0].content_hash, "scoped-hash");
}

#[test]
fn auto_render_reservations_apply_debounce_windows() {
    let mut registry =
        Registry::open(temp_registry_path("auto-render-reservation")).expect("registry");

    let first = registry
        .reserve_auto_render_at("traefik", 250, 1_000)
        .expect("first reservation");
    let second = registry
        .reserve_auto_render_at("traefik", 250, 1_100)
        .expect("debounced reservation");
    let disabled = registry
        .reserve_auto_render_at("traefik", 0, 1_100)
        .expect("disabled debounce");

    assert_eq!(first, Duration::from_millis(0));
    assert_eq!(second, Duration::from_millis(150));
    assert_eq!(disabled, Duration::from_millis(0));
}
