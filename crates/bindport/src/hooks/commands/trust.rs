use super::*;

pub(crate) fn update_hook_trust(
    cwd: &Path,
    config: &ResolvedConfig,
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<(), HooksCommandError> {
    let selected = selected_hooks(hooks, options)?;
    let subjects = hook_trust_subjects_for_config(cwd, config);
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
