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
