// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn registry_defaults_are_named_for_bindport() {
    assert_eq!(default_registry_directory_name(), "bindport");
    assert_eq!(DEFAULT_REGISTRY_FILE, "registry.sqlite");
}

#[test]
fn default_registry_path_honors_env_precedence() {
    let root = env::temp_dir().join(format!("bindport-registry-env-{}", std::process::id()));
    let explicit = root.join("explicit.sqlite");
    let state_home = root.join("state-home");
    let appdata = root.join("appdata");

    with_env_overrides(&[(REGISTRY_PATH_ENV, Some(explicit.as_path()))], || {
        assert_eq!(default_registry_path().expect("explicit path"), explicit);
    });
    with_env_overrides(
        &[
            (REGISTRY_PATH_ENV, None),
            ("XDG_STATE_HOME", Some(state_home.as_path())),
            ("HOME", None),
            ("APPDATA", None),
        ],
        || {
            assert_eq!(
                default_registry_path().expect("state home path"),
                state_home.join("bindport").join(DEFAULT_REGISTRY_FILE)
            );
        },
    );
    with_env_overrides(
        &[
            (REGISTRY_PATH_ENV, None),
            ("XDG_STATE_HOME", None),
            ("HOME", None),
            ("APPDATA", Some(appdata.as_path())),
        ],
        || {
            assert_eq!(
                default_registry_path().expect("appdata path"),
                appdata.join("bindport").join(DEFAULT_REGISTRY_FILE)
            );
        },
    );
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

#[cfg(unix)]
#[test]
fn registry_rejects_symlink_database_paths() {
    let root = env::temp_dir().join(format!("bindport-symlink-registry-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("registry symlink dir");
    let target = root.join("target.sqlite");
    let link = root.join("registry.sqlite");
    fs::write(&target, "").expect("target file");
    std::os::unix::fs::symlink(&target, &link).expect("registry symlink");

    let error = match Registry::open(&link) {
        Ok(_) => panic!("symlink path should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, RegistryError::UnsafePath { path, .. } if path == link));
}

#[test]
fn registry_open_reports_parent_and_database_open_errors() {
    let root = env::temp_dir().join(format!(
        "bindport-open-error-registry-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("registry error dir");

    let parent_file = root.join("parent-file");
    fs::write(&parent_file, "not a directory").expect("parent file");
    let error = match Registry::open(parent_file.join("registry.sqlite")) {
        Ok(_) => panic!("parent file should fail"),
        Err(error) => error,
    };
    assert!(matches!(error, RegistryError::CreateDirectory { path, .. } if path == parent_file));

    let directory_path = root.join("directory.sqlite");
    fs::create_dir_all(&directory_path).expect("directory db");
    let error = match Registry::open(&directory_path) {
        Ok(_) => panic!("directory database should fail"),
        Err(error) => error,
    };
    assert!(matches!(error, RegistryError::Open { path, .. } if path == directory_path));

    let registry_path = root.join("ok.sqlite");
    let registry = Registry::open(&registry_path).expect("registry");
    assert_eq!(registry.path(), registry_path.as_path());
}

#[test]
fn process_command_line_matching_normalizes_recorded_commands() {
    assert!(command_line_contains_recorded_command(
        "node   ./node_modules/.bin/vite --host 0.0.0.0",
        "node ./node_modules/.bin/vite"
    ));
    assert!(command_line_contains_recorded_command(
        "/usr/bin/python3 -m uvicorn example.main:app --port 29123",
        "python3 -m uvicorn"
    ));
    assert!(command_line_contains_recorded_command(
        "sleep 2",
        "sh -c sleep 2"
    ));
    assert!(command_line_contains_recorded_command(
        "/bin/sleep 2",
        "/bin/sh -c sleep 2"
    ));
    assert!(!command_line_contains_recorded_command(
        "node other dev",
        "next dev"
    ));
    assert!(!command_line_contains_recorded_command(
        "sleep 2",
        "sh -c next dev"
    ));
    assert!(!command_line_contains_recorded_command(
        "node other dev",
        ""
    ));
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
