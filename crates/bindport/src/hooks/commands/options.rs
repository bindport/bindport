use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HooksCommand {
    Status,
    Trust,
    Deny,
    Reset,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HooksCommandOptions {
    pub(crate) command: HooksCommand,
    pub(crate) scope: HookTrustScope,
    pub(crate) all: bool,
    pub(crate) name: Option<String>,
}

pub(crate) fn parse_hooks_command(
    args: &[String],
) -> Result<HooksCommandOptions, HooksCommandError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(HooksCommandOptions {
            command: HooksCommand::Status,
            scope: HookTrustScope::Worktree,
            all: false,
            name: None,
        });
    };
    let command = match command {
        "status" => HooksCommand::Status,
        "trust" => HooksCommand::Trust,
        "deny" => HooksCommand::Deny,
        "reset" => HooksCommand::Reset,
        "--help" | "-h" | "help" => HooksCommand::Help,
        unknown => {
            return Err(HooksCommandError::InvalidArgument(format!(
                "unknown hooks command `{unknown}`"
            )));
        }
    };

    let mut options = HooksCommandOptions {
        command,
        scope: HookTrustScope::Worktree,
        all: false,
        name: None,
    };
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--scope" => {
                index += 1;
                let Some(scope) = args.get(index).map(String::as_str) else {
                    return Err(HooksCommandError::InvalidArgument(String::from(
                        "--scope requires worktree or repo",
                    )));
                };
                options.scope = HookTrustScope::parse(scope).ok_or_else(|| {
                    HooksCommandError::InvalidArgument(format!(
                        "invalid hook trust scope `{scope}`"
                    ))
                })?;
            }
            "--all" => options.all = true,
            "--help" | "-h" => {
                options.command = HooksCommand::Help;
            }
            value if value.starts_with('-') => {
                return Err(HooksCommandError::InvalidArgument(format!(
                    "unknown hooks option `{value}`"
                )));
            }
            value => {
                if options.name.is_some() {
                    return Err(HooksCommandError::InvalidArgument(String::from(
                        "only one hook name can be provided",
                    )));
                }
                options.name = Some(value.to_string());
            }
        }
        index += 1;
    }

    if options.all && options.name.is_some() {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "use either --all or a hook name, not both",
        )));
    }
    if matches!(
        options.command,
        HooksCommand::Trust | HooksCommand::Deny | HooksCommand::Reset
    ) && !options.all
        && options.name.is_none()
    {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hook name or --all is required",
        )));
    }
    if options.command == HooksCommand::Status && (options.all || options.name.is_some()) {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hooks status does not take a hook selector",
        )));
    }

    Ok(options)
}
