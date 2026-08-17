use super::*;

pub(crate) fn resolve_service_identity_in_current_scope(
    cwd: &Path,
    project: Option<&str>,
    service: &str,
) -> Result<ServiceIdentity, ConfigError> {
    let config = resolve_config(cwd)?;
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let config_project = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.project.as_deref());

    Ok(resolve_identity_in_scope(
        IdentitySources {
            cwd,
            command: &[],
            cli_project: project,
            cli_service: Some(service),
            env_project: env_project.as_deref(),
            env_service: None,
            config_project,
            config_service: None,
        },
        project_identity_scope(cwd, &config),
    ))
}
