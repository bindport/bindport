use super::*;

pub(crate) fn run_reserve_command(args: &[String]) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_reserve_help();
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|arg| arg == "--all") {
        return match reserve_all_command(args) {
            Ok(services) => {
                for service in services {
                    println!(
                        "{} {}\t{}:{}",
                        service.state, service.service, service.host, service.port
                    );
                    if let Some(route_url) = service.route_url {
                        println!("{route_url}");
                    }
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("bindport: {error}");
                eprintln!("usage: bindport reserve --all");
                ExitCode::FAILURE
            }
        };
    }

    match reserve_command(args) {
        Ok(lease) => {
            println!("reserved {}\t{}:{}", lease.service, lease.host, lease.port);
            if let Some(route_url) = lease.route_url {
                println!("{route_url}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport reserve [service] [--hostname TEMPLATE] [--route-url TEMPLATE] [--health-url TEMPLATE]"
            );
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_release_command(args: &[String]) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_reserve_help();
        return ExitCode::SUCCESS;
    }

    match release_command(args) {
        Ok(Some(lease)) => {
            println!("released {}\t{}:{}", lease.service, lease.host, lease.port);
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("bindport: no reserved lease matched");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport release [service|port]");
            ExitCode::FAILURE
        }
    }
}

fn reserve_command(args: &[String]) -> Result<ReservedLease, ReserveCommandError> {
    let (options, command) =
        parse_run_options(args).map_err(ReserveCommandError::InvalidArgument)?;
    if !command.is_empty() {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "reserve does not accept a wrapped command",
        )));
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let identity = resolve_run_identity(&cwd, &[], &options, &config);
    let service_config = configured_service(&config, &identity);
    let templates = resolve_run_templates(&[], &options, service_config);
    let mut registry = Registry::open_default()?;

    if let Some(existing) = registry.reserved_identity_lease(&identity.identity_key)? {
        return Ok(existing);
    }

    match prune_stale_leases_for_range(&cwd, &config, &mut registry) {
        Ok(summary) if summary.total_leases() > 0 => {
            eprintln!(
                "bindport: pruned {} stale registry entries under configured range pressure",
                summary.total_leases()
            );
        }
        Ok(_) => {}
        Err(error) => print_registry_warning("failed to prune stale registry leases", &error),
    }

    let mut skip_ports = config.skip_ports.clone();
    skip_ports.extend(registry.active_ports()?);
    let previous_port = registry.previous_identity_port(&identity.identity_key)?;
    let allocation_hints = AllocationHints {
        preferred_port: previous_port,
        scan_start: identity.port_scan_start(config.port_range),
    };
    let port = allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints)?;
    let metadata = resolve_run_metadata(&identity, port, &templates)?;

    if has_blocking_auto_outputs(&config)? {
        let pending_route = pending_route_record(&identity, port, &metadata, "reserved", &cwd);
        preflight_blocking_outputs(&cwd, &config, &mut registry, pending_route)?;
    }

    let lease = registry.record_reserved_lease(&ReserveLease {
        project: identity.project.clone(),
        service: identity.service.clone(),
        identity: Some(identity),
        host: String::from("127.0.0.1"),
        port,
        hostname: metadata.hostname,
        route_url: metadata.route_url,
        health_url: metadata.health_url,
    })?;

    let events =
        RouteEventCollector::single(RouteEventSource::CliReserve, RouteEventKind::RouteStarted);
    if let Err(error) = auto_render_outputs_for_events(&cwd, &config, &mut registry, &events) {
        print_auto_render_warning(&events.warning_context(), &error);
    }

    Ok(lease)
}

fn reserve_all_command(args: &[String]) -> Result<Vec<RegistryService>, ReserveCommandError> {
    if args != [String::from("--all")] {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "--all cannot be combined with a service or reservation options",
        )));
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let service_names = configured_service_names_for_all(&config)?;
    let identities = service_names
        .iter()
        .map(|service| {
            let options = RunOptions {
                service: Some(service.clone()),
                ..RunOptions::default()
            };
            resolve_run_identity(&cwd, &[], &options, &config)
        })
        .collect::<Vec<_>>();
    let mut registry = Registry::open_default()?;

    match prune_stale_leases_for_range(&cwd, &config, &mut registry) {
        Ok(summary) if summary.total_leases() > 0 => {
            eprintln!(
                "bindport: pruned {} stale registry entries under configured range pressure",
                summary.total_leases()
            );
        }
        Ok(_) => {}
        Err(error) => print_registry_warning("failed to prune stale registry leases", &error),
    }

    let services =
        registry.reserve_services(&identities, |identity, occupied_ports, previous_port| {
            let mut skip_ports = config.skip_ports.clone();
            skip_ports.extend_from_slice(occupied_ports);
            let allocation_hints = AllocationHints {
                preferred_port: previous_port,
                scan_start: identity.port_scan_start(config.port_range),
            };
            let port = allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints)?;
            let service_config = configured_service(&config, identity);
            let templates = resolve_run_templates(&[], &RunOptions::default(), service_config);
            let metadata = resolve_run_metadata(identity, port, &templates)?;

            Ok::<_, ReserveCommandError>(ReservationCandidate {
                host: String::from("127.0.0.1"),
                port,
                hostname: metadata.hostname,
                route_url: metadata.route_url,
                health_url: metadata.health_url,
            })
        });
    let services = match services {
        Ok(services) => services,
        Err(BatchReservationError::Registry(error)) => return Err(error.into()),
        Err(BatchReservationError::Plan(error)) => return Err(error),
    };

    let events =
        RouteEventCollector::single(RouteEventSource::CliReserve, RouteEventKind::RouteStarted);
    if let Err(error) = auto_render_outputs_for_events(&cwd, &config, &mut registry, &events) {
        print_auto_render_warning(&events.warning_context(), &error);
    }

    Ok(services)
}

fn configured_service_names_for_all(
    config: &ResolvedConfig,
) -> Result<Vec<String>, ReserveCommandError> {
    let loaded = config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .ok_or_else(|| {
            ReserveCommandError::InvalidArgument(String::from(
                "reserve --all requires a discovered project config",
            ))
        })?;
    let mut names = loaded
        .config
        .services
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|service| service.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if names.is_empty()
        && let Some(service) = loaded
            .config
            .service
            .as_deref()
            .map(str::trim)
            .filter(|service| !service.is_empty())
    {
        names.push(service.to_string());
    }
    if names.is_empty() {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "project config does not define any named services",
        )));
    }
    let unique = names.iter().collect::<BTreeSet<_>>();
    if unique.len() != names.len() {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "project config defines duplicate service names",
        )));
    }

    Ok(names)
}

fn release_command(args: &[String]) -> Result<Option<ReservedLease>, ReserveCommandError> {
    let target = parse_release_target(args)?;
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let mut registry = Registry::open_default()?;

    let released = match target {
        ReleaseTarget::Port(port) => registry.release_reserved_port(port)?,
        ReleaseTarget::Service(service) => {
            let options = RunOptions {
                service,
                ..RunOptions::default()
            };
            let identity = resolve_run_identity(&cwd, &[], &options, &config);
            registry.release_reserved_identity(&identity.identity_key)?
        }
    };

    if released.is_some() {
        let events = RouteEventCollector::single(
            RouteEventSource::CliReserve,
            RouteEventKind::RouteFinished,
        );
        if let Err(error) = auto_render_outputs_for_events(&cwd, &config, &mut registry, &events) {
            print_auto_render_warning(&events.warning_context(), &error);
        }
    }

    Ok(released)
}

enum ReleaseTarget {
    Port(u16),
    Service(Option<String>),
}

fn parse_release_target(args: &[String]) -> Result<ReleaseTarget, ReserveCommandError> {
    match args {
        [] => Ok(ReleaseTarget::Service(None)),
        [value] => match value.parse::<u16>() {
            Ok(port) => Ok(ReleaseTarget::Port(port)),
            Err(_) => Ok(ReleaseTarget::Service(Some(value.clone()))),
        },
        _ => Err(ReserveCommandError::InvalidArgument(String::from(
            "release accepts at most one service name or port",
        ))),
    }
}

#[derive(Debug)]
enum ReserveCommandError {
    Config(ConfigError),
    Registry(RegistryError),
    Runner(RunnerError),
    Template(TemplateError),
    Render(RenderCommandError),
    InvalidArgument(String),
}

impl std::fmt::Display for ReserveCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Registry(error) => write!(formatter, "{error}"),
            Self::Runner(error) => write!(formatter, "{error}"),
            Self::Template(error) => write!(formatter, "{error}"),
            Self::Render(error) => write!(formatter, "{error}"),
            Self::InvalidArgument(error) => formatter.write_str(error),
        }
    }
}

impl From<ConfigError> for ReserveCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<RegistryError> for ReserveCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<RunnerError> for ReserveCommandError {
    fn from(error: RunnerError) -> Self {
        Self::Runner(error)
    }
}

impl From<TemplateError> for ReserveCommandError {
    fn from(error: TemplateError) -> Self {
        Self::Template(error)
    }
}

impl From<RenderCommandError> for ReserveCommandError {
    fn from(error: RenderCommandError) -> Self {
        Self::Render(error)
    }
}
