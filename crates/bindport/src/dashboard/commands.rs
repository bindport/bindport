use super::*;

pub(crate) fn run_dashboard(args: &[String]) -> ExitCode {
    match run_dashboard_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(DashboardCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(DashboardCommandError::Dashboard(error)) => {
            eprintln!("bindport: dashboard unavailable: {error}");
            ExitCode::FAILURE
        }
        Err(DashboardCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport dashboard [serve|start|status|stop] [--host IP] [--port PORT]"
            );
            ExitCode::FAILURE
        }
        Err(DashboardCommandError::Io(error)) => {
            eprintln!("bindport: dashboard service unavailable: {error}");
            ExitCode::FAILURE
        }
        Err(DashboardCommandError::MissingToken { source_name }) => {
            eprintln!("bindport: {source_name} is required when dashboard auth is enabled");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_dashboard_result(args: &[String]) -> Result<(), DashboardCommandError> {
    let (command, options) = parse_dashboard_command(args)?;

    match command {
        DashboardCommand::Serve => serve_dashboard(&options),
        DashboardCommand::Start => start_dashboard_service(&options),
        DashboardCommand::Status => print_dashboard_service_status(),
        DashboardCommand::Stop => stop_dashboard_service(),
        DashboardCommand::Help => {
            print_dashboard_help();
            Ok(())
        }
    }
}
