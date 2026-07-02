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
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(config_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let config = fs::read_to_string(&config_path).expect("fallback config");

    assert!(stdout.contains(&config_path.display().to_string()));
    assert!(config.contains("default_range = \"29000-29999\""));
}
