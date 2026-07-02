// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn normalizes_branch_labels_for_hostnames() {
    assert_eq!(normalize_branch_label("feature/tree"), "feature-tree");
    assert_eq!(
        normalize_branch_label("BUGFIX/JIRA-123_widget"),
        "bugfix-jira-123-widget"
    );
    assert_eq!(normalize_branch_label("!!!"), "branch");
}

#[test]
fn identity_sources_follow_precedence() {
    let cwd = Path::new("/tmp/bindport");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd,
        command: &command,
        cli_project: None,
        cli_service: Some("cli-service"),
        env_project: Some("env-project"),
        env_service: Some("env-service"),
        config_project: Some("config-project"),
        config_service: Some("config-service"),
    });

    assert_eq!(identity.project, "env-project");
    assert_eq!(identity.service, "cli-service");
}

#[test]
fn config_identity_beats_inference() {
    let cwd = Path::new("/tmp/bindport");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: Some("config-project"),
        config_service: Some("config-service"),
    });

    assert_eq!(identity.project, "config-project");
    assert_eq!(identity.service, "config-service");
}

#[test]
fn package_metadata_infers_standalone_identity() {
    let root = temp_test_dir("package-standalone");
    fs::write(root.join("package.json"), r#"{"name":"@example/portal"}"#)
        .expect("write package json");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &root,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(identity.project, "portal");
    assert_eq!(identity.service, "portal");
}

#[test]
fn package_workspaces_infer_root_project_without_git() {
    let root = temp_test_dir("package-workspaces-root");
    fs::write(
        root.join("package.json"),
        r#"{"name":"example","workspaces":["apps/*"]}"#,
    )
    .expect("write root package json");
    let api = root.join("apps").join("api");
    let api_src = api.join("src");
    fs::create_dir_all(&api_src).expect("api src");
    fs::write(api.join("package.json"), r#"{"name":"@example/api"}"#)
        .expect("write api package json");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &api_src,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(identity.project, "example");
    assert_eq!(identity.service, "api");
}

#[test]
fn package_workspace_object_infers_root_project() {
    let root = temp_test_dir("package-workspace-object");
    fs::write(
        root.join("package.json"),
        r#"{"name":"example-suite","workspaces":{"packages":["packages/*"]}}"#,
    )
    .expect("write root package json");
    let web = root.join("packages").join("web");
    fs::create_dir_all(&web).expect("web dir");
    fs::write(web.join("package.json"), r#"{"name":"@example/web"}"#)
        .expect("write web package json");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &web,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(identity.project, "example-suite");
    assert_eq!(identity.service, "web");
}

#[test]
fn pnpm_workspace_yaml_infers_root_project_without_git() {
    let root = temp_test_dir("pnpm-workspace-root");
    fs::write(root.join("package.json"), r#"{"name":"example"}"#).expect("write root package json");
    fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n")
        .expect("write pnpm workspace");
    let web = root.join("apps").join("web");
    fs::create_dir_all(&web).expect("web dir");
    fs::write(web.join("package.json"), r#"{"name":"@example/web"}"#)
        .expect("write web package json");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &web,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(identity.project, "example");
    assert_eq!(identity.service, "web");
}

#[test]
fn package_workspace_root_beats_outer_git_root_package() {
    let root = temp_test_dir("workspace-below-git-root");
    git(&root, ["init"]);
    git(&root, ["config", "user.email", "bindport@example.invalid"]);
    git(&root, ["config", "user.name", "BindPort Test"]);
    git(&root, ["config", "commit.gpgsign", "false"]);
    fs::write(root.join("package.json"), r#"{"name":"outer"}"#).expect("write outer package json");
    let workspace = root.join("frontend");
    fs::create_dir_all(&workspace).expect("workspace dir");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"example","workspaces":["apps/*"]}"#,
    )
    .expect("write workspace package json");
    let web = workspace.join("apps").join("web");
    fs::create_dir_all(&web).expect("web dir");
    fs::write(web.join("package.json"), r#"{"name":"@example/web"}"#)
        .expect("write web package json");
    fs::write(root.join("README.md"), "test\n").expect("write fixture");
    git(
        &root,
        [
            "add",
            "README.md",
            "package.json",
            "frontend/package.json",
            "frontend/apps/web/package.json",
        ],
    );
    git(&root, ["commit", "-m", "initial"]);
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &web,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(identity.project, "example");
    assert_eq!(identity.service, "web");
    assert!(identity.git.is_some());
}

#[test]
fn package_metadata_uses_git_root_project_and_nearest_service() {
    let root = temp_test_dir("package-monorepo");
    git(&root, ["init"]);
    git(&root, ["config", "user.email", "bindport@example.invalid"]);
    git(&root, ["config", "user.name", "BindPort Test"]);
    git(&root, ["config", "commit.gpgsign", "false"]);
    fs::write(root.join("package.json"), r#"{"name":"example"}"#).expect("write root package json");
    let service = root.join("apps").join("web");
    fs::create_dir_all(&service).expect("service dir");
    fs::write(service.join("package.json"), r#"{"name":"@example/web"}"#)
        .expect("write service package json");
    fs::write(root.join("README.md"), "test\n").expect("write fixture");
    git(
        &root,
        ["add", "README.md", "package.json", "apps/web/package.json"],
    );
    git(&root, ["commit", "-m", "initial"]);
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &service,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(identity.project, "example");
    assert_eq!(identity.service, "web");
    assert!(identity.git.is_some());
}

#[test]
fn explicit_identity_beats_package_metadata() {
    let root = temp_test_dir("package-explicit");
    fs::write(root.join("package.json"), r#"{"name":"package-project"}"#)
        .expect("write package json");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &root,
        command: &command,
        cli_project: None,
        cli_service: Some("cli-service"),
        env_project: Some("env-project"),
        env_service: Some("env-service"),
        config_project: Some("config-project"),
        config_service: Some("config-service"),
    });

    assert_eq!(identity.project, "env-project");
    assert_eq!(identity.service, "cli-service");
}

#[test]
fn invalid_package_metadata_falls_back_to_directory_and_command() {
    let root = temp_test_dir("package-invalid");
    fs::write(root.join("package.json"), r#"{"name":""}"#).expect("write package json");
    let command = [String::from("next")];

    let identity = resolve_identity(IdentitySources {
        cwd: &root,
        command: &command,
        cli_project: None,
        cli_service: None,
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_eq!(
        identity.project,
        root.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(identity.service, "next");
}

#[test]
fn package_identity_handles_scoped_names_and_workspace_fallbacks() {
    assert_eq!(
        package_identity_name("@scope/web"),
        Some(String::from("web"))
    );
    assert_eq!(package_identity_name("@scope/"), None);
    assert_eq!(package_identity_name(" "), None);
    assert_eq!(
        directory_identity_name(Path::new("/")),
        String::from("workspace")
    );

    let root = temp_test_dir("workspace-name-fallback");
    fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n")
        .expect("write pnpm workspace");
    let metadata = workspace_root_metadata(&root);
    assert_eq!(
        metadata.identity_name,
        root.file_name().unwrap().to_str().unwrap()
    );
}

#[test]
fn identity_key_delimits_project_and_service_values() {
    let cwd = Path::new("/tmp/bindport");
    let command = [String::from("next")];
    let first = resolve_identity(IdentitySources {
        cwd,
        command: &command,
        cli_project: Some("a:b"),
        cli_service: Some("c"),
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });
    let second = resolve_identity(IdentitySources {
        cwd,
        command: &command,
        cli_project: Some("a"),
        cli_service: Some("b:c"),
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });

    assert_ne!(first.identity_key, second.identity_key);
    assert!(first.identity_key.starts_with("v1:"));
}

#[test]
fn identity_port_scan_start_is_stable_and_in_range() {
    let identity = ServiceIdentity {
        project: String::from("bindport"),
        service: String::from("web"),
        git: None,
        identity_key: String::from("v1:test"),
    };
    let range = PortRange {
        start: 29_100,
        end: 29_199,
    };
    let scan_start = identity.port_scan_start(range).expect("scan start");

    assert!(range.contains(scan_start));
    assert_eq!(identity.port_scan_start(range), Some(scan_start));
    assert_eq!(
        identity.port_scan_start(PortRange { start: 100, end: 0 }),
        None
    );
}

#[test]
fn detects_git_worktree_branch_and_commit() {
    let root = temp_test_dir("git-identity");
    git(&root, ["init"]);
    git(&root, ["config", "user.email", "bindport@example.invalid"]);
    git(&root, ["config", "user.name", "BindPort Test"]);
    git(&root, ["config", "commit.gpgsign", "false"]);
    fs::write(root.join("README.md"), "test\n").expect("write fixture");
    git(&root, ["add", "README.md"]);
    git(&root, ["commit", "-m", "initial"]);
    git(&root, ["checkout", "-B", "feature/tree"]);
    let nested = root.join("apps").join("web");
    fs::create_dir_all(&nested).expect("nested dir");

    let identity = detect_git_identity(&nested).expect("git identity");

    assert_eq!(identity.worktree_path, root.canonicalize().expect("root"));
    assert_eq!(identity.branch, "feature/tree");
    assert_eq!(identity.branch_label, "feature-tree");
    assert!(!identity.commit.is_empty());
    assert!(!identity.worktree_hash.is_empty());
}
