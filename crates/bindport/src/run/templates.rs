use super::*;

#[derive(Debug, Default)]
pub(crate) struct RunTemplates {
    pub(crate) command: Option<Vec<String>>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct RunMetadata {
    pub(crate) command: Option<Vec<String>>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) enum TemplateError {
    Unclosed {
        template: String,
    },
    Unopened {
        template: String,
    },
    UnknownPlaceholder {
        placeholder: String,
        template: String,
    },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unclosed { template } => {
                write!(f, "unclosed template placeholder in `{template}`")
            }
            Self::Unopened { template } => {
                write!(f, "unmatched `}}` in template `{template}`")
            }
            Self::UnknownPlaceholder {
                placeholder,
                template,
            } => {
                write!(
                    f,
                    "unknown or unavailable template placeholder `{placeholder}` in `{template}`"
                )
            }
        }
    }
}

impl std::error::Error for TemplateError {}

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

pub(crate) fn expand_command_templates(
    command: &[String],
    values: &TemplateValues<'_>,
) -> Result<Vec<String>, TemplateError> {
    command
        .iter()
        .map(|template| expand_template(template, values))
        .collect()
}

pub(crate) fn resolved_child_command(
    explicit_command: &[String],
    metadata: &RunMetadata,
) -> Result<Vec<String>, RunnerError> {
    let command = if explicit_command.is_empty() {
        metadata.command.as_deref().unwrap_or(explicit_command)
    } else {
        explicit_command
    };

    if command
        .first()
        .is_none_or(|program| program.trim().is_empty())
    {
        return Err(RunnerError::NoCommand);
    }

    Ok(command.to_vec())
}

pub(crate) struct TemplateValues<'a> {
    pub(crate) identity: &'a ServiceIdentity,
    pub(crate) port: u16,
    pub(crate) hostname: Option<&'a str>,
    pub(crate) route_url: Option<&'a str>,
    pub(crate) health_url: Option<&'a str>,
    pub(crate) host: &'static str,
    pub(crate) url: String,
}

impl<'a> TemplateValues<'a> {
    pub(crate) fn new(
        identity: &'a ServiceIdentity,
        port: u16,
        hostname: Option<&'a str>,
        route_url: Option<&'a str>,
        health_url: Option<&'a str>,
    ) -> Self {
        let host = "127.0.0.1";

        Self {
            identity,
            port,
            hostname,
            route_url,
            health_url,
            host,
            url: format!("http://{host}:{port}"),
        }
    }

    pub(crate) fn value(&self, name: &str) -> Option<String> {
        match name {
            "port" => Some(self.port.to_string()),
            "host" => Some(self.host.to_string()),
            "url" => Some(self.url.clone()),
            "project" => Some(self.identity.project.clone()),
            "service" => Some(self.identity.service.clone()),
            "hostname" => self.hostname.map(str::to_string),
            "route_url" => Some(self.route_url.unwrap_or(&self.url).to_string()),
            "health_url" => self.health_url.map(str::to_string),
            "branch" | "branch_label" => Some(
                self.identity
                    .git
                    .as_ref()
                    .map(|git| git.branch_label.clone())
                    .unwrap_or_else(|| String::from("no-branch")),
            ),
            "git_branch" => Some(
                self.identity
                    .git
                    .as_ref()
                    .map(|git| git.branch.clone())
                    .unwrap_or_else(|| String::from("no-branch")),
            ),
            "worktree" | "worktree_label" => Some(
                self.identity
                    .git
                    .as_ref()
                    .and_then(|git| {
                        git.worktree_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(normalize_branch_label)
                    })
                    .unwrap_or_else(|| normalize_branch_label(&self.identity.project)),
            ),
            "worktree_hash" => Some(
                self.identity
                    .git
                    .as_ref()
                    .map(|git| git.worktree_hash.clone())
                    .unwrap_or_else(|| String::from("no-git")),
            ),
            _ => None,
        }
    }
}

pub(crate) fn expand_template(
    template: &str,
    values: &TemplateValues<'_>,
) -> Result<String, TemplateError> {
    let mut output = String::new();
    let mut chars = template.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    output.push('{');
                    continue;
                }

                let mut placeholder = String::new();
                let mut closed = false;

                for character in chars.by_ref() {
                    if character == '}' {
                        closed = true;
                        break;
                    }
                    placeholder.push(character);
                }

                if !closed {
                    return Err(TemplateError::Unclosed {
                        template: template.to_string(),
                    });
                }

                let value = values.value(&placeholder).ok_or_else(|| {
                    TemplateError::UnknownPlaceholder {
                        placeholder: placeholder.clone(),
                        template: template.to_string(),
                    }
                })?;
                output.push_str(&value);
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    output.push('}');
                    continue;
                }

                return Err(TemplateError::Unopened {
                    template: template.to_string(),
                });
            }
            _ => output.push(character),
        }
    }

    Ok(output)
}
