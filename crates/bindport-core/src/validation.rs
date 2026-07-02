use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValidationIssue {
    pub field: String,
    pub message: String,
}

impl ConfigValidationIssue {
    pub(crate) fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

pub(crate) fn validate_services(
    services: Option<&[ServiceConfig]>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(services) = services else {
        return;
    };
    let mut names = BTreeSet::new();

    for (index, service) in services.iter().enumerate() {
        let name_field = format!("services[{index}].name");
        match service.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => {
                if !names.insert(name.to_string()) {
                    issues.push(ConfigValidationIssue::new(
                        name_field,
                        format!("duplicate service name `{name}`; service names must be unique"),
                    ));
                }
            }
            _ => issues.push(ConfigValidationIssue::new(
                name_field,
                "service name is required",
            )),
        }

        if let Some(path) = service.path.as_deref() {
            validate_service_path(index, path, issues);
        }
        validate_service_command(index, service, issues);
        validate_service_env(index, service.env.as_ref(), issues);
        if let Some(hostname) = service.hostname.as_deref() {
            validate_no_control_chars(
                &format!("services[{index}].hostname"),
                hostname,
                "service hostname must not contain control characters",
                issues,
            );
            validate_no_backticks(
                &format!("services[{index}].hostname"),
                hostname,
                "service hostname must not contain backticks",
                issues,
            );
        }
        if let Some(route_url) = service.route_url.as_deref() {
            validate_no_control_chars(
                &format!("services[{index}].route_url"),
                route_url,
                "service route URL must not contain control characters",
                issues,
            );
        }
        if let Some(health_url) = service.health_url.as_deref() {
            if health_url.trim().is_empty() {
                issues.push(ConfigValidationIssue::new(
                    format!("services[{index}].health_url"),
                    "service health URL must not be empty",
                ));
            } else {
                validate_no_control_chars(
                    &format!("services[{index}].health_url"),
                    health_url,
                    "service health URL must not contain control characters",
                    issues,
                );
            }
        }
    }
}

pub(crate) fn validate_service_env(
    index: usize,
    env: Option<&BTreeMap<String, String>>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(env) = env else {
        return;
    };

    for name in env.keys() {
        let field = format!("services[{index}].env.{name}");
        if !is_valid_env_name(name) {
            issues.push(ConfigValidationIssue::new(
                field,
                "service env name must contain only ASCII letters, digits, or `_`, and must not start with a digit",
            ));
        } else if is_restricted_service_env_name(name) {
            issues.push(ConfigValidationIssue::new(
                field,
                "service env name can affect child process execution and must be passed explicitly on the CLI",
            ));
        }
    }
}

pub(crate) fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub fn is_restricted_service_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();

    upper == "PATH"
        || upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || upper.starts_with("MALLOC_")
        || upper == "NODE_OPTIONS"
        || upper == "BASH_ENV"
        || upper == "ENV"
        || upper == "GCONV_PATH"
        || upper == "LOCPATH"
        || upper == "NLSPATH"
        || upper == "PYTHONPATH"
        || upper == "PYTHONHOME"
        || upper == "PERL5OPT"
        || upper == "PERL5LIB"
        || upper == "RUBYOPT"
        || upper == "GEM_HOME"
        || upper == "GEM_PATH"
        || upper == "IFS"
        || upper == "SHELLOPTS"
        || upper == "CDPATH"
        || upper.starts_with("GIT_CONFIG_")
}

pub(crate) fn validate_no_control_chars(
    field: &str,
    value: &str,
    message: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if value.bytes().any(is_control_byte) {
        issues.push(ConfigValidationIssue::new(field, message));
    }
}

pub(crate) fn validate_no_backticks(
    field: &str,
    value: &str,
    message: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if value.contains('`') {
        issues.push(ConfigValidationIssue::new(field, message));
    }
}

pub(crate) fn is_control_byte(byte: u8) -> bool {
    byte < 0x20 || byte == 0x7f
}

pub(crate) fn validate_service_path(
    index: usize,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let field = format!("services[{index}].path");
    let path = path.trim();

    if path.is_empty() {
        issues.push(ConfigValidationIssue::new(
            field,
            "service path must not be empty",
        ));
        return;
    }

    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        issues.push(ConfigValidationIssue::new(
            field,
            "service path must be relative to the config file and must not contain `..`",
        ));
    }
}

pub(crate) fn validate_service_command(
    index: usize,
    service: &ServiceConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(command) = service.command.as_deref() else {
        if service.args.as_ref().is_some_and(|args| !args.is_empty()) {
            issues.push(ConfigValidationIssue::new(
                format!("services[{index}].args"),
                "service args require a service command",
            ));
        }
        return;
    };

    match command.first().map(String::as_str).map(str::trim) {
        Some(program) if !program.is_empty() => {}
        _ => issues.push(ConfigValidationIssue::new(
            format!("services[{index}].command"),
            "service command must start with a program",
        )),
    }
}

pub(crate) fn validate_output_defaults(
    defaults: Option<&OutputDefaultsConfig>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(defaults) = defaults else {
        return;
    };

    if let Some(root) = defaults.root.as_deref() {
        validate_output_root("output_defaults.root", root, issues);
    }
}

pub(crate) fn validate_outputs(
    outputs: Option<&[OutputConfig]>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(outputs) = outputs else {
        return;
    };
    let mut names = BTreeSet::new();

    for (index, output) in outputs.iter().enumerate() {
        let name_field = format!("outputs[{index}].name");
        let Some(name) = output
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            issues.push(ConfigValidationIssue::new(
                name_field,
                "output name is required",
            ));
            continue;
        };

        if !names.insert(name.to_string()) {
            issues.push(ConfigValidationIssue::new(
                name_field,
                format!("duplicate output name `{name}`; output names must be unique"),
            ));
        }

        if !output.enabled.unwrap_or(true) {
            continue;
        }

        if let Some(root) = output.root.as_deref() {
            validate_output_root(&format!("outputs[{index}].root"), root, issues);
        }

        if output
            .template
            .as_deref()
            .map(str::trim)
            .filter(|template| !template.is_empty())
            .is_none()
        {
            issues.push(ConfigValidationIssue::new(
                format!("outputs[{index}].template"),
                format!("output `{name}` is missing required `template`"),
            ));
        }

        if output
            .target
            .as_deref()
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .is_none()
        {
            issues.push(ConfigValidationIssue::new(
                format!("outputs[{index}].target"),
                format!("output `{name}` is missing required `target`"),
            ));
        }
    }
}

pub(crate) fn validate_output_root(
    field: &str,
    root: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let trimmed = root.trim();
    if trimmed.is_empty() {
        issues.push(ConfigValidationIssue::new(
            field,
            "output root must not be empty",
        ));
        return;
    }

    let path = Path::new(trimmed);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        issues.push(ConfigValidationIssue::new(
            field,
            "output root must be relative to the config file and must not contain `..`",
        ));
    }
}

pub(crate) fn validate_hooks(hooks: Option<&HooksConfig>, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(hooks) = hooks else {
        return;
    };

    if hooks.timeout_ms.is_some_and(|timeout| timeout == 0) {
        issues.push(ConfigValidationIssue::new(
            "hooks.timeout_ms",
            "hook timeout must be greater than 0",
        ));
    }

    let Some(commands) = hooks.commands.as_deref() else {
        return;
    };

    let mut names = BTreeSet::new();
    for (index, hook) in commands.iter().enumerate() {
        if !hook.enabled.unwrap_or(true) {
            continue;
        }

        let name = hook
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());

        if let Some(name) = name
            && !names.insert(name.to_string())
        {
            issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].name"),
                format!("duplicate hook name `{name}`; hook names must be unique"),
            ));
        }

        let Some(command) = hook.command.as_deref() else {
            issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].command"),
                "hook command is required",
            ));
            continue;
        };

        match command.first().map(String::as_str).map(str::trim) {
            Some(program) if !program.is_empty() => {}
            _ => issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].command"),
                "hook command must start with a program",
            )),
        }

        match hook.events.as_deref() {
            Some(events) if !events.is_empty() => {}
            Some(_) => issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].events"),
                "hook events must not be empty",
            )),
            None => issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].events"),
                "hook events are required",
            )),
        }

        if hook.timeout_ms.is_some_and(|timeout| timeout == 0) {
            issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].timeout_ms"),
                "hook timeout must be greater than 0",
            ));
        }
    }
}
