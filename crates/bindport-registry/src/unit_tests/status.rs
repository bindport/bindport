// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn registry_records_identity_fields_for_status() {
    let mut registry = Registry::open(temp_registry_path("identity")).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("bindport"),
        service: String::from("web"),
        git: Some(bindport_core::GitIdentity {
            worktree_path: PathBuf::from("/tmp/bindport-worktree"),
            worktree_hash: String::from("abc123"),
            git_common_dir: PathBuf::from("/tmp/bindport-worktree/.git"),
            branch: String::from("feature/tree"),
            branch_label: String::from("feature-tree"),
            commit: String::from("1234567"),
        }),
        identity_key: String::from("v1:identity"),
    };
    let started = registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port: 29_124,
            hostname: Some(String::from("feature-tree.bindport.localhost")),
            route_url: Some(String::from("http://feature-tree.bindport.localhost")),
            health_url: None,
            pid: 12_346,
            command: String::from("next dev"),
            cwd: PathBuf::from("/tmp/bindport-worktree"),
        })
        .expect("record start");

    registry
        .record_run_finished(started, Some(0))
        .expect("record finish");

    let snapshot = registry.status_snapshot().expect("snapshot");
    let service = &snapshot.services[0];

    assert_eq!(
        service.worktree_path.as_deref(),
        Some("/tmp/bindport-worktree")
    );
    assert_eq!(service.worktree_hash.as_deref(), Some("abc123"));
    assert_eq!(service.branch.as_deref(), Some("feature/tree"));
    assert_eq!(service.branch_label.as_deref(), Some("feature-tree"));
    assert_eq!(service.commit.as_deref(), Some("1234567"));
    assert_eq!(service.identity_key.as_deref(), Some("v1:identity"));
    assert_eq!(
        service.hostname.as_deref(),
        Some("feature-tree.bindport.localhost")
    );
    assert_eq!(
        service.route_url.as_deref(),
        Some("http://feature-tree.bindport.localhost")
    );
}

#[test]
fn status_snapshot_reports_service_outputs_and_traefik_proxy_alias() {
    let mut registry =
        Registry::open(temp_registry_path("status-service-outputs")).expect("registry");
    let identity = test_identity("v1:status-output");
    let started = registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port: 29_124,
            hostname: Some(String::from("status.localhost")),
            route_url: Some(String::from("http://status.localhost")),
            health_url: None,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: PathBuf::from("/tmp/bindport"),
        })
        .expect("record start");

    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: test_output_scope("/tmp/bindport/traefik"),
            route_key: identity.identity_key,
            rendered_path: PathBuf::from("/tmp/bindport/traefik/web.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("hash-1")),
            template_hash: Some(String::from("template-1")),
            lease_id: Some(started.lease_id),
            run_id: Some(started.run_id),
        })
        .expect("record rendered file");

    let snapshot = registry.status_snapshot().expect("snapshot");
    let service = &snapshot.services[0];

    assert_eq!(snapshot.outputs.len(), 1);
    assert_eq!(snapshot.outputs[0].name, "traefik");
    assert_eq!(snapshot.outputs[0].rendered, 1);
    assert_eq!(service.outputs.len(), 1);
    assert_eq!(service.outputs[0].name, "traefik");
    assert_eq!(service.outputs[0].status, "rendered");
    assert_eq!(service.outputs[0].reason, None);
    assert_eq!(service.outputs[0].path, "/tmp/bindport/traefik/web.yml");
    let proxy = service.proxy.as_ref().expect("traefik proxy alias");
    assert_eq!(proxy.adapter, "traefik");
    assert!(proxy.rendered);
    assert_eq!(
        proxy.target.as_deref(),
        Some("/tmp/bindport/traefik/web.yml")
    );
}

#[test]
fn status_snapshot_links_outputs_for_services_without_identity_keys() {
    let mut registry =
        Registry::open(temp_registry_path("status-output-fallback-key")).expect("registry");
    let started = registry
        .record_run_started(&test_run_start(
            "bindport",
            "next",
            29_123,
            std::process::id(),
        ))
        .expect("record start");
    let route_key = {
        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert!(service.identity_key.is_none());
        format!(
            "{}:{}:{}:{}:{}",
            service.project, service.service, service.host, service.port, service.started_at
        )
    };

    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: test_output_scope("/tmp/bindport/traefik"),
            route_key,
            rendered_path: PathBuf::from("/tmp/bindport/traefik/next.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("hash-1")),
            template_hash: Some(String::from("template-1")),
            lease_id: Some(started.lease_id),
            run_id: Some(started.run_id),
        })
        .expect("record rendered file");

    let snapshot = registry.status_snapshot().expect("snapshot");
    let service = &snapshot.services[0];

    assert_eq!(service.outputs.len(), 1);
    assert_eq!(service.outputs[0].name, "traefik");
    assert_eq!(service.outputs[0].path, "/tmp/bindport/traefik/next.yml");
    assert!(service.proxy.as_ref().expect("proxy").rendered);
}
