// SPDX-License-Identifier: MIT

mod support;

use support::*;

fn scoped_identity(root: &Path, project: &str, service: &str) -> ServiceIdentity {
    let root = fs::canonicalize(root).expect("canonical test root");
    resolve_identity(IdentitySources {
        cwd: &root,
        command: &[],
        cli_project: Some(project),
        cli_service: Some(service),
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    })
}

fn reserve_identity(registry_path: &Path, identity: &ServiceIdentity, port: u16) {
    Registry::open(registry_path)
        .expect("registry")
        .record_reserved_lease(&ReserveLease {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port,
            hostname: Some(format!("{}.localhost", identity.service)),
            route_url: Some(format!("http://{}.localhost", identity.service)),
            health_url: Some(format!("http://{}.localhost/health", identity.service)),
        })
        .expect("reservation");
}

fn port_output(registry_path: &Path, root: &Path, args: &[&str]) -> std::process::Output {
    bindport_with_registry(registry_path)
        .current_dir(root)
        .arg("port")
        .args(args)
        .output()
        .expect("port command")
}

#[test]
fn port_prints_exact_reserved_and_active_ports() {
    let registry_path = temp_registry_path("port-active-reserved");
    let root = temp_test_dir("port-active-reserved-root");
    let reserved = scoped_identity(&root, "example", "web");
    let active = scoped_identity(&root, "example", "api");
    reserve_identity(&registry_path, &reserved, 29_530);
    Registry::open(&registry_path)
        .expect("registry")
        .record_run_started(&RunStart {
            project: active.project.clone(),
            service: active.service.clone(),
            identity: Some(active),
            host: String::from("127.0.0.1"),
            port: 29_531,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: fs::canonicalize(&root).expect("canonical root"),
        })
        .expect("active service");

    let reserved_output = port_output(&registry_path, &root, &["web", "--project", "example"]);
    assert!(reserved_output.status.success());
    assert_eq!(reserved_output.stdout, b"29530\n");
    assert!(reserved_output.stderr.is_empty());

    let active_output = port_output(&registry_path, &root, &["api", "--project", "example"]);
    assert!(active_output.status.success());
    assert_eq!(active_output.stdout, b"29531\n");
    assert!(active_output.stderr.is_empty());
}

#[test]
fn port_rejects_missing_stopped_and_ambiguous_services() {
    let registry_path = temp_registry_path("port-selection-errors");
    let root = temp_test_dir("port-selection-errors-root");
    let identity = scoped_identity(&root, "example", "web");

    let missing = port_output(&registry_path, &root, &["web", "--project", "example"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no active or reserved service"));

    reserve_identity(&registry_path, &identity, 29_532);
    Registry::open(&registry_path)
        .expect("registry")
        .release_reserved_identity(&identity.identity_key)
        .expect("release")
        .expect("released");
    let stopped = port_output(&registry_path, &root, &["web", "--project", "example"]);
    assert!(!stopped.status.success());
    assert!(String::from_utf8_lossy(&stopped.stderr).contains("no active or reserved service"));

    reserve_identity(&registry_path, &identity, 29_533);
    reserve_identity(&registry_path, &identity, 29_534);
    let ambiguous = port_output(&registry_path, &root, &["web", "--project", "example"]);
    assert!(!ambiguous.status.success());
    assert!(
        String::from_utf8_lossy(&ambiguous.stderr).contains("multiple active or reserved services")
    );
}

#[cfg(unix)]
#[test]
fn port_rejects_stale_services() {
    let registry_path = temp_registry_path("port-stale");
    let root = temp_test_dir("port-stale-root");
    let identity = scoped_identity(&root, "example", "web");
    Registry::open(&registry_path)
        .expect("registry")
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port: 29_535,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: fs::canonicalize(&root).expect("canonical root"),
        })
        .expect("stale service");

    let output = port_output(&registry_path, &root, &["web", "--project", "example"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no active or reserved service"));
}

#[test]
fn port_never_crosses_current_worktree_or_project_scope() {
    let registry_path = temp_registry_path("port-worktrees");
    let first_root = temp_test_dir("port-worktree-first");
    let second_root = temp_test_dir("port-worktree-second");
    fs::write(first_root.join(".bindport.toml"), "project = \"example\"\n").expect("first config");
    fs::write(
        second_root.join(".bindport.toml"),
        "project = \"example\"\n",
    )
    .expect("second config");
    let first = scoped_identity(&first_root, "example", "web");
    let second = scoped_identity(&second_root, "example", "web");
    let other_project = scoped_identity(&first_root, "other", "web");
    reserve_identity(&registry_path, &first, 29_536);
    reserve_identity(&registry_path, &second, 29_537);
    reserve_identity(&registry_path, &other_project, 29_538);

    let first_output = port_output(&registry_path, &first_root, &["web"]);
    assert_eq!(first_output.stdout, b"29536\n");
    let second_output = port_output(&registry_path, &second_root, &["web"]);
    assert_eq!(second_output.stdout, b"29537\n");
    let project_output = port_output(&registry_path, &first_root, &["web", "--project", "other"]);
    assert_eq!(project_output.stdout, b"29538\n");

    let missing_project = port_output(&registry_path, &second_root, &["web", "--project", "other"]);
    assert!(!missing_project.status.success());
}
