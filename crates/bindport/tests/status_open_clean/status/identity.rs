// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn status_json_reports_git_identity() {
    let registry_path = temp_registry_path("git-identity-registry");
    let root = temp_test_dir("git-identity-root");
    init_git_repo(&root, "feature/tree");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(
        service["project"],
        root.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(service["branch"], "feature/tree");
    assert_eq!(service["branch_label"], "feature-tree");
    assert_eq!(
        service["worktree_path"],
        root.canonicalize().unwrap().display().to_string()
    );
    assert!(service["commit"].as_str().expect("commit").len() >= 7);
    assert!(
        service["identity_key"]
            .as_str()
            .expect("identity key")
            .starts_with("v1:")
    );
}
#[test]
fn same_service_in_distinct_worktrees_keeps_distinct_identities() {
    let registry_path = temp_registry_path("worktree-collision-registry");
    let first_root = temp_test_dir("worktree-collision-first");
    let second_root = temp_test_dir("worktree-collision-second");
    let config = "project = \"monorepo\"\ndefault_range = \"29440-29449\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n";

    for root in [&first_root, &second_root] {
        fs::create_dir_all(root.join("apps").join("web")).expect("service dir");
        fs::write(root.join(".bindport.toml"), config).expect("write config");
        init_git_repo(root, "feature/tree");
    }

    let first_marker = temp_path("worktree-collision-first-port");
    let first_marker_arg = first_marker.display().to_string();
    let mut first = bindport_with_registry(&registry_path)
        .current_dir(first_root.join("apps").join("web"))
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PORT\" > \"$1\"; sleep 2",
            "sh",
            &first_marker_arg,
        ])
        .spawn()
        .expect("start first service");
    let first_port = wait_for_file_contains(&first_marker, "", Duration::from_secs(5))
        .parse::<u16>()
        .expect("first port");

    let second_output = bindport_with_registry(&registry_path)
        .current_dir(second_root.join("apps").join("web"))
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run second service");
    assert!(
        second_output.status.success(),
        "second service failed: {}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    let second_port = String::from_utf8(second_output.stdout)
        .expect("second stdout")
        .parse::<u16>()
        .expect("second port");

    assert_ne!(first_port, second_port);
    assert!((29440..=29449).contains(&first_port));
    assert!((29440..=29449).contains(&second_port));

    let first_status = wait_for_child(&mut first, Duration::from_secs(5)).expect("first exits");
    assert!(first_status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    assert!(status_output.status.success());
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    assert_eq!(services.len(), 2);

    let identity_keys = services
        .iter()
        .map(|service| service["identity_key"].as_str().expect("identity key"))
        .collect::<BTreeSet<_>>();
    let worktree_paths = services
        .iter()
        .map(|service| service["worktree_path"].as_str().expect("worktree path"))
        .collect::<BTreeSet<_>>();
    let ports = services
        .iter()
        .map(|service| service["port"].as_u64().expect("service port"))
        .collect::<BTreeSet<_>>();

    assert_eq!(identity_keys.len(), 2);
    assert_eq!(worktree_paths.len(), 2);
    assert_eq!(ports.len(), 2);
    for service in services {
        assert_eq!(service["project"], "monorepo");
        assert_eq!(service["service"], "web");
        assert_eq!(service["branch"], "feature/tree");
    }
}
#[test]
fn status_json_reports_package_metadata_identity() {
    let registry_path = temp_registry_path("package-identity-registry");
    let root = temp_test_dir("package-identity-root");
    fs::write(root.join("package.json"), r#"{"name":"@example/portal"}"#)
        .expect("write package json");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(service["project"], "portal");
    assert_eq!(service["service"], "portal");
}
