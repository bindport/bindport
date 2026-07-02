use super::*;

pub(crate) fn run_open_command(args: &[String]) -> ExitCode {
    match run_open_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(OpenCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport open [service] [--project PROJECT] [--browser] [--print]");
            ExitCode::FAILURE
        }
        Err(OpenCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(OpenCommandError::Browser(error)) => {
            eprintln!("bindport: failed to open URL: {error}");
            ExitCode::FAILURE
        }
        Err(OpenCommandError::Selection(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_open_command_result(args: &[String]) -> Result<(), OpenCommandError> {
    let options = parse_open_options(args)?;

    if options.help {
        print_open_help();
        return Ok(());
    }

    let snapshot = Registry::open_default().and_then(|mut registry| registry.status_snapshot())?;
    let service = select_open_service(&snapshot.services, &options)?;
    let url = best_service_url(service);

    if options.browser {
        open_url_in_browser(&url)?;
    }

    println!("{url}");

    Ok(())
}
