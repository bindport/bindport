use super::*;

pub(crate) struct DashboardRegistration {
    pub(crate) registry: Option<Registry>,
    pub(crate) started: Option<StartedRun>,
}

impl DashboardRegistration {
    pub(crate) fn inactive() -> Self {
        Self {
            registry: None,
            started: None,
        }
    }
}

impl Drop for DashboardRegistration {
    fn drop(&mut self) {
        if let (Some(registry), Some(started)) = (self.registry.as_mut(), self.started)
            && let Err(error) = registry.record_run_finished(started, None)
        {
            print_registry_warning("failed to record dashboard stop", &error);
        }
    }
}

pub(crate) fn register_dashboard_service(
    enabled: bool,
    server: &DashboardServer,
    host: &str,
    cwd: &Path,
    identity_scope: &Path,
) -> DashboardRegistration {
    if !enabled {
        return DashboardRegistration::inactive();
    }

    let Some(mut registry) = open_optional_registry() else {
        return DashboardRegistration::inactive();
    };
    let identity = resolve_identity_in_scope(
        IdentitySources {
            cwd,
            command: &[],
            cli_project: Some(SERVICE_NAME),
            cli_service: Some("dashboard"),
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        },
        identity_scope,
    );
    let run = RunStart {
        project: identity.project.clone(),
        service: identity.service.clone(),
        identity: Some(identity),
        host: host.to_string(),
        port: server.port(),
        hostname: None,
        route_url: Some(server.url()),
        health_url: None,
        pid: std::process::id(),
        command: redacted_dashboard_command(),
        cwd: cwd.to_path_buf(),
    };

    match registry.record_run_started(&run) {
        Ok(started) => DashboardRegistration {
            registry: Some(registry),
            started: Some(started),
        },
        Err(error) => {
            print_registry_warning("failed to register dashboard service", &error);
            registry_disabled_warning();
            DashboardRegistration::inactive()
        }
    }
}

pub(crate) fn redacted_dashboard_command() -> String {
    redacted_dashboard_command_from(env::args())
}

pub(crate) fn redacted_dashboard_command_from(args: impl IntoIterator<Item = String>) -> String {
    let mut args = args.into_iter();
    let mut redacted = Vec::new();

    while let Some(arg) = args.next() {
        redacted.push(arg.clone());
        if arg == "--token" && args.next().is_some() {
            redacted.push(String::from("***"));
        }
    }

    redacted.join(" ")
}
