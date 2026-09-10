// SPDX-License-Identifier: MIT

use crate::support::*;

#[cfg(unix)]
#[test]
fn configured_service_local_bin_search_stops_at_nested_workspace_root() {
    let registry_path = temp_registry_path("local-bin-boundary-registry");
    let root = temp_test_dir("local-bin-boundary-root");
    let workspace = root.join("frontend");
    let service_root = workspace.join("apps").join("web");
    let project_bin = root.join("node_modules").join(".bin");
    let ambient_bin = root.join("ambient-bin");
    fs::create_dir_all(&service_root).expect("service root");
    fs::create_dir_all(&project_bin).expect("project bin");
    fs::create_dir_all(&ambient_bin).expect("ambient bin");
    write_executable(
        &project_bin.join("boundary-tool"),
        "#!/bin/sh\nprintf 'above-boundary'\n",
    );
    write_executable(
        &ambient_bin.join("boundary-tool"),
        "#!/bin/sh\nprintf 'ambient'\n",
    );
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"frontend","workspaces":["apps/*"]}"#,
    )
    .expect("write nested workspace package");
    let (mut range_guards, range_start, range_end) = guarded_port_range();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"local-bin-boundary\"\ndefault_range = \"{range_start}-{range_end}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"frontend/apps/web\"\ncommand = [\"boundary-tool\"]\n"
        ),
    )
    .expect("write config");

    range_guards.truncate(1);
    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(
            "PATH",
            std::env::join_paths([&ambient_bin]).expect("ambient PATH"),
        )
        .args(["run", "web"])
        .output()
        .expect("run configured service");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ambient");
}
