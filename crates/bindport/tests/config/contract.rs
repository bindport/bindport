// SPDX-License-Identifier: MIT

use crate::support::*;

const TOML_CONTRACT: &str =
    include_str!("../../../bindport-core/tests/fixtures/config-v1-candidate.toml");
const JSON_CONTRACT: &str =
    include_str!("../../../bindport-core/tests/fixtures/config-v1-candidate.json");
const YAML_CONTRACT: &str =
    include_str!("../../../bindport-core/tests/fixtures/config-v1-candidate.yaml");

#[test]
fn config_validate_accepts_complete_stable_candidate_in_every_format() {
    for (filename, format, contents) in [
        (".bindport.toml", "toml", TOML_CONTRACT),
        (".bindport.json", "json", JSON_CONTRACT),
        (".bindport.yaml", "yaml", YAML_CONTRACT),
    ] {
        let registry_path = temp_registry_path(&format!("config-contract-{format}-registry"));
        let root = temp_test_dir(&format!("config-contract-{format}-root"));
        let config_path = root.join(filename);
        fs::write(&config_path, contents).expect("write config contract fixture");

        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args(["config", "validate"])
            .output()
            .expect("validate config contract fixture");
        let stdout = String::from_utf8(output.stdout).expect("config validate stdout");

        assert!(
            output.status.success(),
            "{format} validation failed:\n{stdout}"
        );
        assert!(stdout.contains(&format!(
            "config: {} (project {format})",
            config_path.display()
        )));
        assert!(stdout.contains("validation: ok"));
        assert!(!stdout.contains("config warning:"));
        assert!(!stdout.contains("deprecated"));
    }
}

#[test]
fn range_remains_an_ordered_unknown_key_in_every_format() {
    for (filename, format, contents) in [
        (
            ".bindport.toml",
            "toml",
            "project = \"unknown-toml\"\nrange = \"29000-29999\"\nz_unknown = true\n",
        ),
        (
            ".bindport.json",
            "json",
            r#"{"project":"unknown-json","range":"29000-29999","z_unknown":true}"#,
        ),
        (
            ".bindport.yaml",
            "yaml",
            "project: unknown-yaml\nrange: 29000-29999\nz_unknown: true\n",
        ),
    ] {
        let registry_path = temp_registry_path(&format!("config-unknown-{format}-registry"));
        let root = temp_test_dir(&format!("config-unknown-{format}-root"));
        fs::write(root.join(filename), contents).expect("write unknown config");

        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args(["config", "validate"])
            .output()
            .expect("validate unknown config");
        let stdout = String::from_utf8(output.stdout).expect("config validate stdout");

        assert!(
            output.status.success(),
            "{format} validation failed:\n{stdout}"
        );
        assert!(
            stdout.contains("config warning: ignored unknown top-level keys: range, z_unknown")
        );
        assert!(stdout.contains("validation: ok"));
        assert!(!stdout.contains("deprecated"));
    }
}

#[test]
fn unknown_keys_preserve_fallback_and_local_override_sources() {
    let fallback_state = temp_test_dir("config-contract-fallback-state");
    let fallback_registry = fallback_state.join("registry.sqlite");
    let fallback_path = config_home_for_registry(&fallback_registry)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);
    let fallback_cwd = temp_test_dir("config-contract-fallback-cwd");
    fs::create_dir_all(fallback_path.parent().expect("fallback parent"))
        .expect("create fallback parent");
    fs::write(&fallback_path, "range = \"29000-29999\"\n").expect("write fallback config");

    let fallback_output = bindport_with_registry(&fallback_registry)
        .current_dir(&fallback_cwd)
        .args(["config", "validate"])
        .output()
        .expect("validate fallback config");
    let fallback_stdout =
        String::from_utf8(fallback_output.stdout).expect("fallback config validate stdout");

    assert!(fallback_output.status.success());
    assert!(fallback_stdout.contains(&format!(
        "config: {} (fallback toml)",
        fallback_path.display()
    )));
    assert!(fallback_stdout.contains("config warning: ignored unknown top-level keys: range"));
    assert!(!fallback_stdout.contains("deprecated"));

    let local_registry = temp_registry_path("config-contract-local-registry");
    let local_root = temp_test_dir("config-contract-local-root");
    let base_path = local_root.join(".bindport.toml");
    let local_path = local_root.join(".bindport.local.json");
    fs::write(&base_path, "default_range = \"29000-29999\"\n").expect("write base config");
    fs::write(&local_path, r#"{"range":"29100-29199"}"#).expect("write local config");

    let local_output = bindport_with_registry(&local_registry)
        .current_dir(&local_root)
        .args(["config", "validate"])
        .output()
        .expect("validate local config");
    let local_stdout =
        String::from_utf8(local_output.stdout).expect("local config validate stdout");

    assert!(local_output.status.success());
    assert!(local_stdout.contains(&format!("config: {} (project toml)", base_path.display())));
    assert!(local_stdout.contains(&format!(
        "config local override: {} (project json)",
        local_path.display()
    )));
    assert!(local_stdout.contains("config warning: ignored unknown top-level keys: range"));
    assert!(local_stdout.contains("validation: ok"));
    assert!(!local_stdout.contains("deprecated"));
}
