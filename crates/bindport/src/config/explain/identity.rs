use super::*;

#[derive(Debug)]
pub(crate) struct IdentityExplanation {
    pub(crate) identity: ServiceIdentity,
    pub(crate) project_source: String,
    pub(crate) service_source: String,
}

pub(crate) fn explain_run_identity(
    cwd: &Path,
    command: &[String],
    options: &RunOptions,
    config: &ResolvedConfig,
) -> IdentityExplanation {
    let identity = resolve_run_identity(cwd, command, options, config);
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let env_service = env::var(BINDPORT_SERVICE_ENV).ok();

    IdentityExplanation {
        project_source: identity_project_source(config, env_project.as_deref()),
        service_source: identity_service_source(cwd, config, options, env_service.as_deref()),
        identity,
    }
}

pub(crate) fn identity_project_source(
    config: &ResolvedConfig,
    env_project: Option<&str>,
) -> String {
    if non_empty_value(env_project).is_some() {
        return format!("environment {BINDPORT_PROJECT_ENV}");
    }

    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("inference");
    };

    if non_empty_value(local_config(loaded).and_then(|local| local.project.as_deref())).is_some() {
        String::from("local override config `project`")
    } else if non_empty_value(loaded.config.project.as_deref()).is_some() {
        format!("{} `project`", source_config_label(loaded.source))
    } else {
        String::from("inference")
    }
}

pub(crate) fn identity_service_source(
    cwd: &Path,
    config: &ResolvedConfig,
    options: &RunOptions,
    env_service: Option<&str>,
) -> String {
    if non_empty_value(options.service.as_deref()).is_some() {
        return String::from("CLI service argument");
    }

    if non_empty_value(env_service).is_some() {
        return format!("environment {BINDPORT_SERVICE_ENV}");
    }

    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("inference");
    };

    if let Some((_, source)) = config_service_source_for_cwd(loaded, cwd) {
        source
    } else {
        String::from("inference")
    }
}

pub(crate) fn config_service_source_for_cwd(
    loaded: &LoadedConfig,
    cwd: &Path,
) -> Option<(String, String)> {
    let service = loaded.configured_service_for_cwd(cwd)?;
    let name = non_empty_value(Some(service.name))?;

    Some((
        name.to_string(),
        configured_service_source_label(loaded, service.source),
    ))
}

pub(crate) fn configured_service_source_label(
    loaded: &LoadedConfig,
    source: ConfiguredServiceSource,
) -> String {
    match source {
        ConfiguredServiceSource::ServiceField => {
            if non_empty_value(local_config(loaded).and_then(|local| local.service.as_deref()))
                .is_some()
            {
                String::from("local override config `service`")
            } else {
                format!("{} `service`", source_config_label(loaded.source))
            }
        }
        ConfiguredServiceSource::PathMatch => {
            format!("{} `[[services]].path`", services_config_label(loaded))
        }
        ConfiguredServiceSource::SingleService => {
            format!("{} single `[[services]]`", services_config_label(loaded))
        }
    }
}
