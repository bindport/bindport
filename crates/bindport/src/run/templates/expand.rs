use super::*;

pub(crate) type SiblingServices = BTreeMap<String, RegistryService>;

pub(crate) struct TemplateValues<'a> {
    pub(crate) identity: &'a ServiceIdentity,
    pub(crate) port: u16,
    pub(crate) hostname: Option<&'a str>,
    pub(crate) route_url: Option<&'a str>,
    pub(crate) health_url: Option<&'a str>,
    pub(crate) host: &'static str,
    pub(crate) url: String,
    pub(crate) sibling_services: Option<&'a SiblingServices>,
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
            sibling_services: None,
        }
    }

    pub(crate) fn with_sibling_services(mut self, sibling_services: &'a SiblingServices) -> Self {
        self.sibling_services = Some(sibling_services);
        self
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

    fn sibling_value(&self, placeholder: &str) -> Option<Result<String, TemplateError>> {
        let (service_name, field) = sibling_reference(placeholder)?;
        let service = self.sibling_services?.get(service_name)?;
        let direct_url = format!("http://{}:{}", service.host, service.port);
        let value = match field {
            "port" => Some(service.port.to_string()),
            "host" => Some(service.host.clone()),
            "url" => Some(direct_url.clone()),
            "hostname" => service.hostname.clone(),
            "route_url" => Some(
                service
                    .route_url
                    .clone()
                    .or_else(|| {
                        service
                            .hostname
                            .as_ref()
                            .map(|hostname| format!("http://{hostname}"))
                    })
                    .unwrap_or(direct_url),
            ),
            "health_url" => service.health_url.clone(),
            _ => unreachable!("sibling_reference validates fields"),
        };

        Some(value.ok_or_else(|| TemplateError::UnavailableSiblingField {
            service: service_name.to_string(),
            field: field.to_string(),
        }))
    }
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

pub(crate) fn expand_template(
    template: &str,
    values: &TemplateValues<'_>,
) -> Result<String, TemplateError> {
    let mut output = String::new();

    for part in parse_template(template)? {
        match part {
            TemplatePart::Literal(value) => output.push_str(&value),
            TemplatePart::Placeholder(placeholder) => {
                let value = match values.sibling_value(&placeholder) {
                    Some(value) => value?,
                    None => values.value(&placeholder).ok_or_else(|| {
                        TemplateError::UnknownPlaceholder {
                            placeholder: placeholder.clone(),
                            template: template.to_string(),
                        }
                    })?,
                };
                output.push_str(&value);
            }
        }
    }

    Ok(output)
}

pub(crate) fn template_placeholders(template: &str) -> Result<Vec<String>, TemplateError> {
    Ok(parse_template(template)?
        .into_iter()
        .filter_map(|part| match part {
            TemplatePart::Placeholder(placeholder) => Some(placeholder),
            TemplatePart::Literal(_) => None,
        })
        .collect())
}

pub(crate) fn sibling_reference(placeholder: &str) -> Option<(&str, &str)> {
    let reference = placeholder.strip_prefix("services.")?;
    let (service, field) = reference.rsplit_once('.')?;
    if service.is_empty()
        || !matches!(
            field,
            "port" | "host" | "url" | "hostname" | "route_url" | "health_url"
        )
    {
        return None;
    }

    Some((service, field))
}

enum TemplatePart {
    Literal(String),
    Placeholder(String),
}

fn parse_template(template: &str) -> Result<Vec<TemplatePart>, TemplateError> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = template.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '{' => {
                if !literal.is_empty() {
                    parts.push(TemplatePart::Literal(std::mem::take(&mut literal)));
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
                parts.push(TemplatePart::Placeholder(placeholder));
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '}' => {
                return Err(TemplateError::Unopened {
                    template: template.to_string(),
                });
            }
            _ => literal.push(character),
        }
    }

    if !literal.is_empty() {
        parts.push(TemplatePart::Literal(literal));
    }

    Ok(parts)
}
