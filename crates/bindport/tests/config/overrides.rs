// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn local_project_config_overrides_base_project_config() {
    let registry_path = temp_registry_path("local-project-config-registry");
    let root = temp_test_dir("local-project-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"base-project\"\ndefault_range = \"29102-29102\"\nskip_ports = []\n",
    )
    .expect("write base config");
    fs::write(
        root.join(".bindport.local.toml"),
        "project = \"local-project\"\ndefault_range = \"29103-29103\"\n",
    )
    .expect("write local config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29103");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "local-project");

    let doctor_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");
    let stdout = String::from_utf8(doctor_output.stdout).expect("doctor stdout");

    assert!(stdout.contains("config local override:"));
    assert!(stdout.contains(".bindport.local.toml"));
}
#[test]
fn toml_config_wins_over_json_in_same_directory() {
    let registry_path = temp_registry_path("config-precedence-registry");
    let root = temp_test_dir("config-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "default_range = \"29110-29110\"\n",
    )
    .expect("write toml config");
    fs::write(
        root.join(".bindport.json"),
        r#"{"default_range":"29111-29111"}"#,
    )
    .expect("write json config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29110");
}
