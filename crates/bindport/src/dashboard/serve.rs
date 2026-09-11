use super::*;

pub(crate) fn serve_dashboard(options: &DashboardCliOptions) -> Result<(), DashboardCommandError> {
    #[cfg(unix)]
    let shutdown = shutdown::DashboardShutdown::install()?;
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let identity_scope = project_identity_scope(&cwd, &config).to_path_buf();
    let mut skip_ports = config.skip_ports.clone();

    if let Some(mut registry) = open_optional_registry() {
        match registry.active_ports() {
            Ok(active_ports) => skip_ports.extend(active_ports),
            Err(error) => print_registry_warning("failed to read active registry ports", &error),
        }
    }

    let mut dashboard = resolve_dashboard_options(&config, options, skip_ports)?;
    let register_service = resolve_dashboard_registration(&config, options)?;
    dashboard.clean_callback = Some(dashboard_clean_callback(cwd.clone(), config));
    dashboard.status_callback = Some(dashboard_status_callback(cwd.clone()));
    let host = dashboard.host.to_string();
    let server = DashboardServer::bind(dashboard)?;
    let _registration =
        register_dashboard_service(register_service, &server, &host, &cwd, &identity_scope);
    println!("dashboard: {}", server.url());
    io::stdout().flush().ok();
    #[cfg(unix)]
    {
        background::redirect_background_stdout()?;
        server.serve_until(|| shutdown.requested())?;
    }
    #[cfg(not(unix))]
    server.serve()?;

    Ok(())
}
