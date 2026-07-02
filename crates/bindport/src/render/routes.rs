use super::*;

pub(crate) fn route_records(services: Vec<StatusService>) -> Vec<RouteRecord> {
    services
        .into_iter()
        .map(|service| {
            let key = status_service_route_key(&service);
            let updated_at = service
                .exited_at
                .clone()
                .unwrap_or_else(|| service.started_at.clone());

            RouteRecord {
                key,
                project: service.project,
                service: service.service,
                state: service.state,
                health: service.health,
                port: service.port,
                host: service.host,
                url: service.url,
                hostname: service.hostname,
                route_url: service.route_url,
                branch: service.branch,
                branch_label: service.branch_label,
                worktree_path: service.worktree_path,
                worktree_hash: service.worktree_hash,
                pid: service.pid,
                command: service.command,
                cwd: service.cwd,
                started_at: service.started_at,
                updated_at,
            }
        })
        .collect()
}

pub(crate) fn pending_route_record(
    identity: &ServiceIdentity,
    port: u16,
    metadata: &RunMetadata,
    command: &str,
    cwd: &Path,
) -> RouteRecord {
    let git = identity.git.as_ref();

    RouteRecord {
        key: identity.identity_key.clone(),
        project: identity.project.clone(),
        service: identity.service.clone(),
        state: String::from("active"),
        health: String::from("unknown"),
        port,
        host: String::from("127.0.0.1"),
        url: format!("http://127.0.0.1:{port}"),
        hostname: metadata.hostname.clone(),
        route_url: metadata.route_url.clone(),
        branch: git.map(|git| git.branch.clone()),
        branch_label: git.map(|git| git.branch_label.clone()),
        worktree_path: git.map(|git| git.worktree_path.display().to_string()),
        worktree_hash: git.map(|git| git.worktree_hash.clone()),
        pid: None,
        command: command.to_string(),
        cwd: cwd.display().to_string(),
        started_at: String::from("pending"),
        updated_at: String::from("pending"),
    }
}

pub(crate) fn output_base_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .to_path_buf()
}
