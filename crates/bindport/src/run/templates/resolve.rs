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
            templates.env.push(EnvTemplate {
                name: name.clone(),
                value: value.clone(),
                configured: true,
            });
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

pub(crate) fn upsert_env_template(env: &mut Vec<EnvTemplate>, name: String, value: String) {
    if let Some(existing) = env.iter_mut().find(|existing| existing.name == name) {
        existing.value = value;
        existing.configured = false;
    } else {
        env.push(EnvTemplate {
            name,
            value,
            configured: false,
        });
    }
}

pub(crate) fn env_template_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

pub(crate) fn configured_sibling_service_names(
    templates: &RunTemplates,
) -> Result<Vec<String>, TemplateError> {
    let configured_templates = templates.command.iter().flatten().chain(
        templates
            .env
            .iter()
            .filter(|template| template.configured)
            .map(|template| &template.value),
    );
    let mut names = BTreeSet::new();

    for template in configured_templates {
        for placeholder in template_placeholders(template)? {
            if !placeholder.starts_with("services.") {
                continue;
            }
            let Some((service, _)) = sibling_reference(&placeholder) else {
                return Err(TemplateError::UnknownPlaceholder {
                    placeholder,
                    template: template.clone(),
                });
            };
            names.insert(service.to_string());
        }
    }

    Ok(names.into_iter().collect())
}

pub(crate) fn resolve_sibling_services(
    cwd: &Path,
    config: &ResolvedConfig,
    names: &[String],
    registry: &mut Registry,
) -> Result<SiblingServices, RegistryError> {
    let identities = names
        .iter()
        .map(|service| {
            let options = RunOptions {
                service: Some(service.clone()),
                ..RunOptions::default()
            };
            resolve_run_identity(cwd, &[], &options, config)
        })
        .collect::<Vec<_>>();
    let services = registry.select_services(&identities)?;

    Ok(names.iter().cloned().zip(services).collect())
}

pub(crate) fn resolve_reservation_metadata(
    identity: &ServiceIdentity,
    port: u16,
    templates: &RunTemplates,
) -> Result<RunMetadata, TemplateError> {
    let sibling_services = configured_sibling_service_names(templates)?
        .into_iter()
        .map(|service| {
            let selected = RegistryService {
                lease_id: 0,
                project: identity.project.clone(),
                service: service.clone(),
                identity_key: String::new(),
                state: String::from("reserved"),
                host: String::new(),
                port: 0,
                hostname: Some(String::new()),
                route_url: Some(String::new()),
                health_url: Some(String::new()),
            };
            (service, selected)
        })
        .collect();

    resolve_run_metadata(identity, port, templates, &sibling_services)
}

pub(crate) fn resolve_run_route_metadata(
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

    Ok(RunMetadata {
        command: None,
        hostname,
        route_url,
        health_url,
        env: Vec::new(),
    })
}

pub(crate) fn resolve_run_metadata(
    identity: &ServiceIdentity,
    port: u16,
    templates: &RunTemplates,
    sibling_services: &SiblingServices,
) -> Result<RunMetadata, TemplateError> {
    let mut metadata = resolve_run_route_metadata(identity, port, templates)?;
    let sibling_values = TemplateValues::new(
        identity,
        port,
        metadata.hostname.as_deref(),
        metadata.route_url.as_deref(),
        metadata.health_url.as_deref(),
    )
    .with_sibling_services(sibling_services);
    let own_values = TemplateValues::new(
        identity,
        port,
        metadata.hostname.as_deref(),
        metadata.route_url.as_deref(),
        metadata.health_url.as_deref(),
    );
    metadata.env = templates
        .env
        .iter()
        .map(|template| {
            let values = if template.configured {
                &sibling_values
            } else {
                &own_values
            };
            expand_template(&template.value, values).map(|value| (template.name.clone(), value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    metadata.command = templates
        .command
        .as_ref()
        .map(|command| expand_command_templates(command, &sibling_values))
        .transpose()?;

    Ok(metadata)
}
