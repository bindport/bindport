use super::*;

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
    let subjects = hook_trust_subjects_for_config(cwd, config);

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
