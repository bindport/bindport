use super::*;

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
