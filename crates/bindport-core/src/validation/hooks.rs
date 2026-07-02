use super::*;

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
