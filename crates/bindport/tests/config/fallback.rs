// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn fallback_config_from_config_home_is_used_when_no_project_config_exists() {
    let state_dir = temp_test_dir("fallback-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = config_home_for_registry(&registry_path)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);
    let cwd = temp_test_dir("fallback-config-cwd");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("config dir");
    fs::write(&config_path, "default_range = \"29200-29200\"\n").expect("write fallback config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29200");
}
#[test]
fn init_creates_fallback_config_in_config_home() {
    let state_dir = temp_test_dir("init-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = config_home_for_registry(&registry_path)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);

    let output = bindport_with_registry(&registry_path)
        .args(["init", "--user"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(config_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let config = fs::read_to_string(&config_path).expect("fallback config");

    assert!(stdout.contains(&config_path.display().to_string()));
    assert!(config.contains("default_range = \"29000-29999\""));
}

#[test]
fn init_creates_project_config_in_current_directory() {
    let registry_path = temp_registry_path("init-project-config-registry");
    let root = temp_test_dir("init-project-config-root");
    let config_path = root.join(".bindport.toml");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(config_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let config = fs::read_to_string(&config_path).expect("project config");

    assert!(stdout.contains("created project config"));
    assert!(stdout.contains(&config_path.display().to_string()));
    assert!(config.contains("project = \""));
    assert!(config.contains("default_range = \"29000-29999\""));
    assert!(config.contains("skip_ports = ["));
    assert!(!config.contains(&root.display().to_string()));

    let validate = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("validate generated config");
    assert!(
        validate.status.success(),
        "generated config should validate: {}",
        String::from_utf8_lossy(&validate.stdout)
    );
}

#[test]
fn init_reports_existing_project_config_without_overwriting() {
    let registry_path = temp_registry_path("init-existing-project-config-registry");
    let root = temp_test_dir("init-existing-project-config-root");
    let existing_path = root.join(".bindport.json");
    fs::write(&existing_path, "{\"project\":\"existing\"}\n").expect("write existing config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(!root.join(".bindport.toml").exists());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("project config already exists"));
    assert!(stdout.contains(&existing_path.display().to_string()));
    assert_eq!(
        fs::read_to_string(existing_path).expect("existing config"),
        "{\"project\":\"existing\"}\n"
    );
}

#[cfg(unix)]
#[test]
fn init_rejects_dangling_project_config_symlink() {
    let registry_path = temp_registry_path("init-symlink-project-config-registry");
    let root = temp_test_dir("init-symlink-project-config-root");
    let target = temp_test_dir("init-symlink-project-config-target").join("created.toml");
    let symlink = root.join(".bindport.toml");
    std::os::unix::fs::symlink(&target, &symlink).expect("create dangling symlink");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(!output.status.success());
    assert!(!target.exists());

    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("failed to initialize project config"));
    assert!(stderr.contains("exists but is not a regular file"));
}
