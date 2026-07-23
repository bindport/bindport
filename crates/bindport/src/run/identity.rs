use super::*;

pub(crate) fn resolve_run_identity(
    cwd: &Path,
    command: &[String],
    options: &RunOptions,
    config: &ResolvedConfig,
) -> ServiceIdentity {
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let env_service = env::var(BINDPORT_SERVICE_ENV).ok();
    let config_project = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.project.as_deref());
    let config_service = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.configured_service_name_for_cwd(cwd));

    resolve_identity_in_scope(
        IdentitySources {
            cwd,
            command,
            cli_project: None,
            cli_service: options.service.as_deref(),
            env_project: env_project.as_deref(),
            env_service: env_service.as_deref(),
            config_project,
            config_service,
        },
        project_identity_scope(cwd, config),
    )
}

pub(crate) fn project_identity_scope<'a>(cwd: &'a Path, config: &'a ResolvedConfig) -> &'a Path {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
}
