use super::*;

pub(crate) fn run_hooks_command(args: &[String]) -> ExitCode {
    match run_hooks_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(HooksCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(HooksCommandError::Io(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(HooksCommandError::InvalidArgument(message)) => {
            eprintln!("bindport: {message}");
            eprintln!("usage: bindport hooks status|trust|deny|reset [options]");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_hooks_command_result(args: &[String]) -> Result<(), HooksCommandError> {
    let options = parse_hooks_command(args)?;
    if options.command == HooksCommand::Help {
        print_hooks_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let Some(plan) = configured_hook_plan(&cwd, &config) else {
        println!("No hooks configured.");
        return Ok(());
    };
    if plan.hooks.is_empty() {
        println!("No enabled hooks configured.");
        return Ok(());
    }

    match options.command {
        HooksCommand::Status => print_hooks_status(&cwd, &config),
        HooksCommand::Trust | HooksCommand::Deny | HooksCommand::Reset => {
            update_hook_trust(&cwd, plan.hooks, &options)
        }
        HooksCommand::Help => Ok(()),
    }
}

pub(crate) fn print_hooks_status(
    cwd: &Path,
    config: &ResolvedConfig,
) -> Result<(), HooksCommandError> {
    let statuses = hook_statuses_for_current_dir(cwd, config);
    if statuses.is_empty() {
        println!("No hooks configured.");
        return Ok(());
    }

    println!("BindPort hooks");
    for status in statuses {
        print_hook_status(&status);
    }

    Ok(())
}

pub(crate) fn print_hook_status(status: &HookStatus) {
    println!(
        "{}\t{}\t{}",
        status.trust.as_str(),
        status.hook.name,
        command_display(&status.hook.command)
    );
    println!("  trust: {}", hook_trust_status_display(status.trust));
    println!("  source: {}", status.hook.source);
    println!("  events: {}", hook_events_display(&status.hook.events));
    println!(
        "  target: {} ({})",
        status.hook.target.display,
        status.hook.target.kind.as_str()
    );
    println!("  hook hash: {}", status.hook.hook_hash);
    println!("  target hash: {}", status.hook.target.hash);
}

pub(crate) fn update_hook_trust(
    cwd: &Path,
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<(), HooksCommandError> {
    let selected = selected_hooks(hooks, options)?;
    let subjects = hook_trust_subjects(cwd);
    let mut store = read_hook_trust_store()?;
    let names = selected
        .iter()
        .map(|hook| hook.name.clone())
        .collect::<BTreeSet<_>>();

    match options.command {
        HooksCommand::Trust | HooksCommand::Deny => {
            let decision = if options.command == HooksCommand::Trust {
                HookDecision::Approved
            } else {
                HookDecision::Denied
            };
            for hook in &selected {
                upsert_hook_trust_entry(&mut store, &subjects, options.scope, hook, decision)
                    .map_err(HooksCommandError::InvalidArgument)?;
            }
            write_hook_trust_store(&store)?;
            println!(
                "{} {} hook(s) for {} scope",
                decision.as_str(),
                selected.len(),
                options.scope.as_str()
            );
        }
        HooksCommand::Reset => {
            let removed = reset_hook_trust_entries(&mut store, &subjects, options.scope, &names);
            write_hook_trust_store(&store)?;
            println!(
                "reset {removed} hook trust entr{} for {} scope",
                if removed == 1 { "y" } else { "ies" },
                options.scope.as_str()
            );
        }
        HooksCommand::Status | HooksCommand::Help => {}
    }

    Ok(())
}

pub(crate) fn selected_hooks(
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<Vec<EffectiveHook>, HooksCommandError> {
    if options.all {
        return Ok(hooks);
    }
    let Some(name) = options.name.as_deref() else {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hook name or --all is required",
        )));
    };
    let selected = hooks
        .into_iter()
        .filter(|hook| hook.name == name)
        .collect::<Vec<_>>();

    if selected.is_empty() {
        Err(HooksCommandError::InvalidArgument(format!(
            "hook `{name}` is not configured or is disabled"
        )))
    } else {
        Ok(selected)
    }
}

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

#[derive(Debug)]
pub(crate) enum HooksCommandError {
    Config(ConfigError),
    Io(io::Error),
    InvalidArgument(String),
}

impl From<ConfigError> for HooksCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<io::Error> for HooksCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn hooks_status_json_for_current_dir() -> serde_json::Value {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    match resolve_config(&cwd) {
        Ok(config) => hooks_status_json(&cwd, &config),
        Err(error) => serde_json::json!({
            "error": error.to_string(),
            "items": [],
        }),
    }
}

pub(crate) fn hooks_status_json(cwd: &Path, config: &ResolvedConfig) -> serde_json::Value {
    let items = hook_statuses_for_current_dir(cwd, config)
        .into_iter()
        .map(|status| {
            serde_json::json!({
                "name": status.hook.name,
                "status": status.trust.as_str(),
                "trust": hook_trust_status_display(status.trust),
                "source": status.hook.source,
                "events": status
                    .hook
                    .events
                    .iter()
                    .map(|event| event.as_str())
                    .collect::<Vec<_>>(),
                "command": status.hook.command,
                "command_display": command_display(&status.hook.command),
                "timeout_ms": status.hook.timeout_ms,
                "hook_hash": status.hook.hook_hash,
                "target": {
                    "kind": status.hook.target.kind.as_str(),
                    "display": status.hook.target.display,
                    "hash": status.hook.target.hash,
                },
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({ "items": items })
}
pub(crate) fn print_doctor_hooks(cwd: &Path, config: &ResolvedConfig) {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        println!("hooks: none configured");
        return;
    };

    if plan.hooks.is_empty() {
        println!("hooks: none enabled");
        return;
    }

    let store = read_hook_trust_store().unwrap_or_default();
    let subjects = hook_trust_subjects(cwd);

    println!("hooks: {} configured", plan.hooks.len());
    for hook in plan.hooks {
        let trust = hook_trust_status(&hook, &store, &subjects);
        println!("  hook {}:", hook.name);
        println!("    trust: {}", hook_trust_status_display(trust));
        println!("    source: {}", hook.source);
        println!("    events: {}", hook_events_display(&hook.events));
        println!("    command: {}", command_display(&hook.command));
        println!("    timeout: {}ms", hook.timeout.as_millis());
        println!(
            "    target: {} ({})",
            hook.target.display,
            hook.target.kind.as_str()
        );
        println!("    hook hash: {}", hook.hook_hash);
        println!("    target hash: {}", hook.target.hash);
        println!(
            "    env: BINDPORT_HOOK_EVENTS=<redacted> BINDPORT_HOOK_SOURCES=<redacted> BINDPORT_HOOK_CONTEXT=<redacted>"
        );
    }
}

pub(crate) fn hook_trust_status_display(status: HookTrustStatus) -> String {
    match status {
        HookTrustStatus::Approved { scope } => format!("approved ({})", scope.as_str()),
        HookTrustStatus::Denied { scope } => format!("denied ({})", scope.as_str()),
        HookTrustStatus::Changed => String::from("changed"),
        HookTrustStatus::Pending => String::from("pending"),
    }
}

pub(crate) fn hook_events_display(events: &[HookEvent]) -> String {
    events
        .iter()
        .map(|event| event.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
