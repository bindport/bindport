use super::*;

pub(crate) fn run_hooks_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    events: &RouteEventCollector,
    output_rendered: bool,
    mode: HookRunMode,
) -> usize {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        return 0;
    };
    let hook_events = events.hook_events(output_rendered);
    if hook_events.is_empty() {
        return 0;
    }
    let matching_hooks = plan
        .hooks
        .iter()
        .filter(|hook| hook_matches_events(hook, &hook_events))
        .collect::<Vec<_>>();

    if matching_hooks.is_empty() {
        return 0;
    }

    let store = match read_hook_trust_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("bindport: warning: hook trust store unavailable: {error}");
            return 0;
        }
    };
    let subjects = hook_trust_subjects_for_config(cwd, config);
    let env = HookEnvironment::new(events, &hook_events);
    let mut ran = 0;
    for hook in &matching_hooks {
        let trust = hook_trust_status(hook, &store, &subjects);
        if !trust.is_approved() {
            print_hook_not_trusted_warning(hook, trust);
            continue;
        }

        match mode {
            HookRunMode::DryRun => print_hook_dry_run(hook),
            HookRunMode::Run => {
                if let Err(error) = execute_hook(&plan.base_dir, hook, &env) {
                    eprintln!("bindport: warning: hook `{}` failed: {error}", hook.name);
                }
            }
        }
        ran += 1;
    }

    ran
}

pub(crate) fn hook_matches_events(hook: &EffectiveHook, events: &BTreeSet<HookEvent>) -> bool {
    hook.events.iter().any(|event| events.contains(event))
}

pub(crate) fn print_hook_not_trusted_warning(hook: &EffectiveHook, trust: HookTrustStatus) {
    let reason = match trust {
        HookTrustStatus::Pending => "pending approval",
        HookTrustStatus::Changed => "changed since the last trust decision",
        HookTrustStatus::Denied { .. } => "denied",
        HookTrustStatus::Approved { .. } => return,
    };
    eprintln!(
        "bindport: warning: hook `{}` not run ({reason}); inspect with `bindport hooks status`",
        hook.name
    );
}

pub(crate) fn print_hook_dry_run(hook: &EffectiveHook) {
    println!(
        "would run hook {} ({}): {}",
        hook.name,
        hook.source,
        command_display(&hook.command)
    );
    println!(
        "  env: BINDPORT_HOOK_EVENTS=<redacted> BINDPORT_HOOK_SOURCES=<redacted> BINDPORT_HOOK_CONTEXT=<redacted>"
    );
}
