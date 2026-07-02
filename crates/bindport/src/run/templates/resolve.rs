use super::*;

pub(crate) fn configured_service<'a>(
    config: &'a ResolvedConfig,
    identity: &ServiceIdentity,
) -> Option<&'a ServiceConfig> {
    config
        .loaded
        .as_ref()?
        .config
        .service_config(&identity.service)
}

pub(crate) fn resolve_run_templates(
    command: &[String],
    options: &RunOptions,
    service_config: Option<&ServiceConfig>,
) -> RunTemplates {
    let mut templates = RunTemplates::default();
    if command.is_empty() {
        templates.command = service_config.and_then(ServiceConfig::command_argv);
    }

    if let Some(env) = service_config.and_then(|service| service.env.as_ref()) {
        for (name, value) in env {
            if is_restricted_service_env_name(name) {
                eprintln!(
                    "bindport: ignoring restricted service env `{name}` from config; pass it explicitly with --env if needed"
                );
                continue;
            }
            templates.env.push((name.clone(), value.clone()));
        }
    }

    for (name, value) in &options.env {
        upsert_env_template(&mut templates.env, name.clone(), value.clone());
    }

    templates.hostname = options
        .hostname
        .clone()
        .or_else(|| env_template_value(BINDPORT_HOSTNAME_ENV))
        .or_else(|| service_config.and_then(|service| service.hostname.clone()));
    templates.route_url = options
        .route_url
        .clone()
        .or_else(|| env_template_value(BINDPORT_ROUTE_URL_ENV))
        .or_else(|| service_config.and_then(|service| service.route_url.clone()));
    templates.health_url = options
        .health_url
        .clone()
        .or_else(|| env_template_value(BINDPORT_HEALTH_URL_ENV))
        .or_else(|| service_config.and_then(|service| service.health_url.clone()));

    templates
}

pub(crate) fn upsert_env_template(env: &mut Vec<(String, String)>, name: String, value: String) {
    if let Some((_, existing)) = env.iter_mut().find(|(existing, _)| existing == &name) {
        *existing = value;
    } else {
        env.push((name, value));
    }
}

pub(crate) fn env_template_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

pub(crate) fn resolve_run_metadata(
    identity: &ServiceIdentity,
    port: u16,
    templates: &RunTemplates,
) -> Result<RunMetadata, TemplateError> {
    let base_values = TemplateValues::new(identity, port, None, None, None);
    let hostname = templates
        .hostname
        .as_deref()
        .map(|template| expand_template(template, &base_values))
        .transpose()?;
    let route_values = TemplateValues::new(identity, port, hostname.as_deref(), None, None);
    let route_url = templates
        .route_url
        .as_deref()
        .map(|template| expand_template(template, &route_values))
        .transpose()?
        .or_else(|| {
            hostname
                .as_ref()
                .map(|hostname| format!("http://{hostname}"))
        });
    let health_values = TemplateValues::new(
        identity,
        port,
        hostname.as_deref(),
        route_url.as_deref(),
        None,
    );
    let health_url = templates
        .health_url
        .as_deref()
        .map(|template| expand_template(template, &health_values))
        .transpose()?;
    let env_values = TemplateValues::new(
        identity,
        port,
        hostname.as_deref(),
        route_url.as_deref(),
        health_url.as_deref(),
    );
    let env = templates
        .env
        .iter()
        .map(|(name, template)| {
            expand_template(template, &env_values).map(|value| (name.clone(), value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let command = templates
        .command
        .as_ref()
        .map(|command| expand_command_templates(command, &env_values))
        .transpose()?;

    Ok(RunMetadata {
        command,
        hostname,
        route_url,
        health_url,
        env,
    })
}
