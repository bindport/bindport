// SPDX-License-Identifier: MIT

use crate::support::*;
use bindport_registry::{OutputFileRecord, OutputFileScope, OutputFileStatus};

#[test]
fn registry_export_json_includes_full_registry_rows() {
    let registry_path = temp_registry_path("registry-export");
    let root = temp_test_dir("registry-export-root");
    let generated_root = root.join(".bindport/generated");
    let mut registry = Registry::open(&registry_path).expect("registry");
    let run = registry
        .record_run_started(&RunStart {
            project: String::from("export-project"),
            service: String::from("web"),
            identity: Some(ServiceIdentity {
                project: String::from("export-project"),
                service: String::from("web"),
                git: None,
                identity_key: String::from("v1:export-project:web"),
            }),
            host: String::from("127.0.0.1"),
            port: 29_880,
            hostname: Some(String::from("web.export.localhost")),
            route_url: Some(String::from("http://web.export.localhost")),
            health_url: Some(String::from("http://web.export.localhost/health")),
            pid: std::process::id(),
            command: String::from("sh -c true"),
            cwd: root.clone(),
        })
        .expect("record run");
    registry
        .record_run_finished(run, Some(0))
        .expect("record finished run");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("routes-json"),
            scope: OutputFileScope::new(
                generated_root.clone(),
                root.clone(),
                Some(root.clone()),
                Some(String::from("worktree-hash")),
            ),
            route_key: String::from("v1:export-project:web"),
            rendered_path: generated_root.join("routes.json"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("content-hash")),
            template_hash: Some(String::from("template-hash")),
            lease_id: Some(run.lease_id),
            run_id: Some(run.run_id),
        })
        .expect("record output file");

    let output = bindport_with_registry(&registry_path)
        .args(["registry", "export"])
        .output()
        .expect("registry export");
    assert!(
        output.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let export = serde_json::from_slice::<Value>(&output.stdout).expect("export json");

    assert_eq!(
        object_keys(&export),
        BTreeSet::from([
            "generated_at",
            "leases",
            "output_files",
            "output_render_state",
            "registry_path",
            "runs",
            "schema_version",
            "user_version",
        ])
    );
    assert_eq!(export["schema_version"], "0.1");
    assert_eq!(export["registry_path"], registry_path.display().to_string());
    assert_eq!(export["user_version"], 9);
    assert_eq!(export["leases"][0]["project"], "export-project");
    assert_eq!(export["leases"][0]["service"], "web");
    assert_eq!(export["leases"][0]["identity_key"], "v1:export-project:web");
    assert_eq!(export["runs"][0]["lease_id"], export["leases"][0]["id"]);
    assert_eq!(export["runs"][0]["exit_code"], 0);
    assert_eq!(export["output_files"][0]["output_name"], "routes-json");
    assert_eq!(
        export["output_files"][0]["route_key"],
        "v1:export-project:web"
    );
    assert_eq!(
        export["output_files"][0]["output_root"],
        generated_root.display().to_string()
    );
    assert_eq!(
        export["output_files"][0]["config_root"],
        root.display().to_string()
    );
    assert_eq!(
        export["output_files"][0]["worktree_path"],
        root.display().to_string()
    );
    assert_eq!(export["output_files"][0]["worktree_hash"], "worktree-hash");
    assert_eq!(export["output_files"][0]["content_hash"], "content-hash");
}
