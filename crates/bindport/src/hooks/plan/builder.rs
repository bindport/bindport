use super::*;

pub(crate) fn configured_hook_plan(cwd: &Path, config: &ResolvedConfig) -> Option<HookPlan> {
    let loaded = config.loaded.as_ref()?;
    let hooks = loaded.config.hooks.as_ref()?;
    let commands = hooks.commands.as_deref().unwrap_or_default();
    let source = hook_command_source(config);
    let default_timeout = hooks.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);
    let base_dir = hook_base_dir(cwd, config);
    let hooks = commands
        .iter()
        .enumerate()
        .filter(|(_, hook)| hook.enabled.unwrap_or(true))
        .filter_map(|(index, hook)| {
            effective_hook(&base_dir, index, hook, default_timeout, &source)
        })
        .collect::<Vec<_>>();

    Some(HookPlan { base_dir, hooks })
}

pub(crate) fn hook_base_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .to_path_buf()
}

pub(crate) fn effective_hook(
    base_dir: &Path,
    index: usize,
    hook: &HookCommandConfig,
    default_timeout_ms: u64,
    source: &str,
) -> Option<EffectiveHook> {
    let command = hook.command.clone()?;
    let events = hook.events.clone()?;
    let timeout_ms = hook.timeout_ms.unwrap_or(default_timeout_ms);
    let name = hook
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("hook-{}", index + 1));
    let target = hook_target(base_dir, &command);
    let definition = hook_definition(&name, &events, &command, timeout_ms, source);
    let hook_hash = stable_hex_hash(definition.as_bytes());

    Some(EffectiveHook {
        name,
        events,
        command,
        timeout: Duration::from_millis(timeout_ms),
        timeout_ms,
        source: source.to_string(),
        definition,
        hook_hash,
        target,
    })
}
