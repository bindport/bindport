// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn output_file_ownership_returns_rendered_files_with_hashes() {
    let mut registry = Registry::open(temp_registry_path("output-files")).expect("registry");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
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
        .output_file_ownership("traefik")
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
        .output_file_ownership("traefik")
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
fn record_output_file_upserts_by_output_and_route() {
    let mut registry = Registry::open(temp_registry_path("output-files-upsert")).expect("registry");
    let mut record = OutputFileRecord {
        output_name: String::from("traefik"),
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
        .output_file_ownership("traefik")
        .expect("ownership records");

    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership[0].path, PathBuf::from("/tmp/bindport/new.yml"));
    assert_eq!(ownership[0].content_hash, "new-hash");
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
