use super::*;

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
