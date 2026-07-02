use super::*;

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
