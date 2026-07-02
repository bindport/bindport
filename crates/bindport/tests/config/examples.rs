// SPDX-License-Identifier: MIT

use crate::support::*;

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
