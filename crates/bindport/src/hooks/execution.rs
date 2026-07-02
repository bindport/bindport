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
    let subjects = hook_trust_subjects(cwd);
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
                if let Err(error) = execute_hook(cwd, hook, &env) {
                    eprintln!("bindport: warning: {error}");
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

#[derive(Debug)]
pub(crate) struct HookEnvironment {
    pub(crate) events: String,
    pub(crate) sources: String,
    pub(crate) context: String,
}

impl HookEnvironment {
    pub(crate) fn new(
        route_events: &RouteEventCollector,
        hook_events: &BTreeSet<HookEvent>,
    ) -> Self {
        Self {
            events: hook_events
                .iter()
                .map(|event| event.as_str())
                .collect::<Vec<_>>()
                .join(","),
            sources: route_events.hook_sources(),
            context: route_events.warning_context(),
        }
    }
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

pub(crate) fn execute_hook(
    cwd: &Path,
    hook: &EffectiveHook,
    env: &HookEnvironment,
) -> Result<(), HookExecutionError> {
    let Some((program, args)) = hook.command.split_first() else {
        return Err(HookExecutionError::Spawn {
            command: command_display(&hook.command),
            source: io::Error::new(io::ErrorKind::InvalidInput, "empty hook command"),
        });
    };
    let display = command_display(&hook.command);
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env_clear()
        .env("BINDPORT_HOOK_EVENTS", &env.events)
        .env("BINDPORT_HOOK_SOURCES", &env.sources)
        .env("BINDPORT_HOOK_CONTEXT", &env.context);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    configure_hook_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|source| HookExecutionError::Spawn {
            command: display.clone(),
            source,
        })?;
    let deadline = Instant::now() + hook.timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(HookExecutionError::Failed {
                    command: display,
                    status,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_hook_child(&mut child);
                let _ = child.wait();
                return Err(HookExecutionError::Timeout {
                    command: display,
                    timeout: hook.timeout,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(source) => {
                return Err(HookExecutionError::Wait {
                    command: display,
                    source,
                });
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn configure_hook_command(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
pub(crate) fn configure_hook_command(_command: &mut Command) {}

#[cfg(unix)]
pub(crate) fn kill_hook_child(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    if pgid > 0 {
        let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
pub(crate) fn kill_hook_child(child: &mut Child) {
    let _ = child.kill();
}

pub(crate) fn command_display(command: &[String]) -> String {
    if command.is_empty() {
        String::from("<empty>")
    } else {
        command.join(" ")
    }
}
