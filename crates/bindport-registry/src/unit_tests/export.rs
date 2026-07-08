// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn registry_export_includes_raw_rows_and_output_scope_fields() {
    let mut registry = Registry::open(temp_registry_path("export-snapshot")).expect("registry");
    let run = registry
        .record_run_started(&test_run_start(
            "export-project",
            "web",
            29_900,
            std::process::id(),
        ))
        .expect("record run");
    registry
        .record_run_finished(run, Some(0))
        .expect("record finish");
    let scope = OutputFileScope::new(
        PathBuf::from("/tmp/bindport/worktree/.bindport/generated"),
        PathBuf::from("/tmp/bindport/worktree"),
        Some(PathBuf::from("/tmp/bindport/worktree")),
        Some(String::from("worktree-hash")),
    );
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("debug"),
            scope,
            route_key: String::from("route-1"),
            rendered_path: PathBuf::from("/tmp/bindport/worktree/.bindport/generated/debug.json"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("content-hash")),
            template_hash: Some(String::from("template-hash")),
            lease_id: Some(run.lease_id),
            run_id: Some(run.run_id),
        })
        .expect("record output");
    registry
        .reserve_auto_render_at("debug", 1_000, 10_000)
        .expect("record render state");

    let export = registry.export_snapshot().expect("export snapshot");

    assert_eq!(export.schema_version, EXPORT_SCHEMA_VERSION);
    assert_eq!(export.user_version, 9);
    assert!(export.registry_path.ends_with(".sqlite"));
    assert_eq!(export.leases.len(), 1);
    assert_eq!(export.leases[0].project, "export-project");
    assert_eq!(export.runs.len(), 1);
    assert_eq!(export.runs[0].lease_id, export.leases[0].id);
    assert_eq!(export.output_files.len(), 1);
    assert_eq!(export.output_files[0].output_name, "debug");
    assert_ne!(export.output_files[0].output_scope, UNSCOPED_OUTPUT_SCOPE);
    assert_eq!(
        export.output_files[0].output_root.as_deref(),
        Some("/tmp/bindport/worktree/.bindport/generated")
    );
    assert_eq!(
        export.output_files[0].config_root.as_deref(),
        Some("/tmp/bindport/worktree")
    );
    assert_eq!(
        export.output_files[0].worktree_path.as_deref(),
        Some("/tmp/bindport/worktree")
    );
    assert_eq!(
        export.output_files[0].worktree_hash.as_deref(),
        Some("worktree-hash")
    );
    assert_eq!(export.output_render_state.len(), 1);
    assert_eq!(export.output_render_state[0].output_name, "debug");
}
