use super::*;

pub(crate) fn run_open_command(args: &[String]) -> ExitCode {
    match run_open_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(OpenCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport open [service] [--project PROJECT] [--registry-wide] [--browser] [--print]"
            );
            ExitCode::FAILURE
        }
        Err(OpenCommandError::Config(error)) => {
            print_config_error(&error);
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

    let url = match options.service.as_deref() {
        Some(service) if !options.registry_wide => {
            let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
            let identity = resolve_service_identity_in_current_scope(
                &cwd,
                options.project.as_deref(),
                service,
            )?;
            let selected = Registry::open_default()?.select_service(&identity)?;
            best_registry_service_url(&selected)
        }
        _ => {
            let snapshot =
                Registry::open_default().and_then(|mut registry| registry.status_snapshot())?;
            let service = select_open_service(&snapshot.services, &options)?;
            best_service_url(service)
        }
    };

    if options.browser {
        open_url_in_browser(&url)?;
    }

    println!("{url}");

    Ok(())
}
