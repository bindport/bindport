use super::*;

pub(crate) fn configured_hook_plan(cwd: &Path, config: &ResolvedConfig) -> Option<HookPlan> {
    let loaded = config.loaded.as_ref()?;
    let hooks = loaded.config.hooks.as_ref()?;
    let commands = hooks.commands.as_deref().unwrap_or_default();
    let source = hook_command_source(config);
    let default_timeout = hooks.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);
    let hooks = commands
        .iter()
        .enumerate()
        .filter(|(_, hook)| hook.enabled.unwrap_or(true))
        .filter_map(|(index, hook)| effective_hook(cwd, index, hook, default_timeout, &source))
        .collect::<Vec<_>>();

    Some(HookPlan { hooks })
}

pub(crate) fn effective_hook(
    cwd: &Path,
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
    let target = hook_target(cwd, &command);
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

pub(crate) fn hook_command_source(config: &ResolvedConfig) -> String {
    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("unknown config");
    };

    if let Some(local) = loaded.local_override.as_ref()
        && local
            .config
            .hooks
            .as_ref()
            .and_then(|hooks| hooks.commands.as_ref())
            .is_some()
    {
        return format!("local override config `{}`", local.path.display());
    }

    format!(
        "{} config `{}`",
        loaded.source.as_str(),
        loaded.path.display()
    )
}

pub(crate) fn hook_definition(
    name: &str,
    events: &[HookEvent],
    command: &[String],
    timeout_ms: u64,
    _source: &str,
) -> String {
    let mut definition = String::from("schema=v1\n");
    append_fingerprinted_field(&mut definition, "name", name);
    definition.push_str(&format!("timeout_ms={timeout_ms}\n"));
    definition.push_str(&format!("events={}\n", events.len()));
    for event in events {
        append_fingerprinted_field(&mut definition, "event", event.as_str());
    }
    definition.push_str(&format!("command={}\n", command.len()));
    for value in command {
        append_fingerprinted_field(&mut definition, "argv", value);
    }

    definition
}

pub(crate) fn append_fingerprinted_field(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push(':');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push('\n');
}

pub(crate) fn hook_target(cwd: &Path, command: &[String]) -> HookTarget {
    let Some(program) = command.first().map(String::as_str) else {
        return opaque_hook_target("<empty>");
    };

    if !path_like_command(program) {
        return opaque_hook_target(program);
    }

    let path = PathBuf::from(program);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let display_path = path.display().to_string();

    match fs::read(&path) {
        Ok(contents) => {
            let resolved = path
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(&path));
            let fingerprint = format!(
                "file:{}:{}:{}",
                program,
                contents.len(),
                stable_hex_hash(&contents)
            );
            HookTarget {
                kind: HookTargetKind::LocalFile,
                display: resolved.display().to_string(),
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
        Err(_) => {
            let fingerprint = format!("missing:{program}");
            HookTarget {
                kind: HookTargetKind::MissingFile,
                display: display_path,
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
    }
}

pub(crate) fn opaque_hook_target(program: &str) -> HookTarget {
    let fingerprint = format!("opaque:{program}");
    HookTarget {
        kind: HookTargetKind::Opaque,
        display: program.to_string(),
        hash: stable_hex_hash(fingerprint.as_bytes()),
        fingerprint,
    }
}

pub(crate) fn path_like_command(program: &str) -> bool {
    program.contains('/') || program.contains('\\') || program.starts_with('.')
}

pub(crate) fn path_clean_display_path(path: &Path) -> PathBuf {
    path.components().collect()
}

pub(crate) fn stable_hex_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
