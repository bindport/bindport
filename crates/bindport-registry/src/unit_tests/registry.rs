// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn registry_defaults_are_named_for_bindport() {
    assert_eq!(default_registry_directory_name(), "bindport");
    assert_eq!(DEFAULT_REGISTRY_FILE, "registry.sqlite");
}

#[cfg(unix)]
#[test]
fn registry_creates_private_state_dir_and_database() {
    let path = env::temp_dir()
        .join(format!("bindport-private-registry-{}", std::process::id()))
        .join(DEFAULT_REGISTRY_FILE);
    let parent = path.parent().expect("registry parent");
    let _ = fs::remove_dir_all(parent);

    let _registry = Registry::open(&path).expect("registry");

    let dir_mode = fs::metadata(parent)
        .expect("parent metadata")
        .permissions()
        .mode()
        & 0o777;
    let file_mode = fs::metadata(&path)
        .expect("registry metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[test]
fn registry_records_finished_runs_for_status() {
    let mut registry = Registry::open(temp_registry_path("finished")).expect("registry");
    let started = registry
        .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
        .expect("record start");

    registry
        .record_run_finished(started, Some(0))
        .expect("record finish");

    let snapshot = registry.status_snapshot().expect("snapshot");
    assert_eq!(snapshot.schema_version, STATUS_SCHEMA_VERSION);
    assert!(snapshot.outputs.is_empty());
    assert_eq!(snapshot.services.len(), 1);
    assert_eq!(snapshot.services[0].state, "stopped");
    assert_eq!(snapshot.services[0].port, 29_123);
    assert_eq!(snapshot.services[0].url, "http://127.0.0.1:29123");
    assert_eq!(snapshot.services[0].hostname.as_deref(), None);
    assert_eq!(snapshot.services[0].route_url.as_deref(), None);
    assert!(snapshot.services[0].outputs.is_empty());
    assert!(snapshot.services[0].proxy.is_none());
    assert_eq!(snapshot.services[0].exit_code, Some(0));
    assert_eq!(snapshot.runs.len(), 1);
}
