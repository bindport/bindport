// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn checked_in_starter_examples_validate_by_format() {
    let examples_dir = workspace_root().join("examples").join("config");

    for (format, filename) in [
        ("toml", ".bindport.toml"),
        ("json", ".bindport.json"),
        ("yaml", ".bindport.yaml"),
    ] {
        let registry_path = temp_registry_path(&format!("starter-example-{format}-registry"));
        let root = temp_test_dir(&format!("starter-example-{format}-root"));
        fs::copy(examples_dir.join(filename), root.join(filename)).expect("copy example config");

        let validate = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args(["config", "validate"])
            .output()
            .expect("validate starter example");
        assert!(
            validate.status.success(),
            "config validate failed for {filename}: stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&validate.stdout),
            String::from_utf8_lossy(&validate.stderr)
        );
        let stdout = String::from_utf8(validate.stdout).expect("validate stdout");
        assert!(
            stdout.contains("validation: ok"),
            "config validate did not report success for {filename}: {stdout}"
        );
        assert!(
            !stdout.contains("ignored unknown top-level keys"),
            "starter example contains unknown config keys for {filename}: {stdout}"
        );
    }
}

#[test]
fn checked_in_monorepo_example_resolves_services() {
    let registry_path = temp_registry_path("monorepo-example-registry");
    let root = workspace_root().join("examples").join("monorepo");

    let validate = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("validate monorepo example");
    assert!(
        validate.status.success(),
        "config validate failed: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
    let validate_stdout = String::from_utf8(validate.stdout).expect("validate stdout");
    assert!(validate_stdout.contains("validation: ok"));

    for (path, service) in [("apps/web", "web"), ("apps/api", "api")] {
        let explain = bindport_with_registry(&registry_path)
            .current_dir(root.join(path))
            .args(["config", "explain"])
            .output()
            .expect("explain monorepo service");
        assert!(
            explain.status.success(),
            "config explain failed for {path}: {}",
            String::from_utf8_lossy(&explain.stderr)
        );
        let stdout = String::from_utf8(explain.stdout).expect("explain stdout");
        assert!(stdout.contains("project: example (project config `project`)"));
        assert!(stdout.contains(&format!(
            "service: {service} (project config `[[services]].path`)"
        )));
    }
}
