use super::*;

pub(crate) fn run_subcommand(args: &[String]) -> ExitCode {
    match parse_run_options(args) {
        Ok((options, command)) => run_wrapped_command(command, options),
        Err(error) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport run [service] [--env NAME=VALUE] [--hostname TEMPLATE] [--route-url TEMPLATE] [--health-url TEMPLATE] [-- <command>]"
            );
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn parse_run_options(args: &[String]) -> Result<(RunOptions, &[String]), String> {
    let (option_args, command) = match args.iter().position(|arg| arg == "--") {
        Some(separator) => {
            let (option_args, command) = args.split_at(separator);
            (option_args, &command[1..])
        }
        None => (args, &args[args.len()..]),
    };

    let mut options = RunOptions::default();
    let mut index = 0;
    while index < option_args.len() {
        match option_args[index].as_str() {
            "--env" => {
                index += 1;
                let value = option_args
                    .get(index)
                    .ok_or_else(|| String::from("--env requires NAME=VALUE"))?;
                let (name, value) = parse_env_assignment(value)?;
                options.env.push((name, value));
            }
            "--hostname" => {
                index += 1;
                options.hostname = Some(
                    option_args
                        .get(index)
                        .cloned()
                        .ok_or_else(|| String::from("--hostname requires a value"))?,
                );
            }
            "--route-url" => {
                index += 1;
                options.route_url = Some(
                    option_args
                        .get(index)
                        .cloned()
                        .ok_or_else(|| String::from("--route-url requires a value"))?,
                );
            }
            "--health-url" => {
                index += 1;
                options.health_url = Some(
                    option_args
                        .get(index)
                        .cloned()
                        .ok_or_else(|| String::from("--health-url requires a value"))?,
                );
            }
            option if option.starts_with("--") => {
                return Err(format!("unknown run option `{option}`"));
            }
            service => {
                if options.service.is_some() {
                    return Err(String::from("only one service name can be provided"));
                }
                options.service = Some(service.to_string());
            }
        }

        index += 1;
    }

    Ok((options, command))
}

pub(crate) fn parse_env_assignment(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once('=')
        .ok_or_else(|| format!("invalid env assignment `{value}`; expected NAME=VALUE"))?;
    let name = name.trim();
    if !valid_env_name(name) {
        return Err(format!("invalid env variable name `{name}`"));
    }

    Ok((name.to_string(), value.to_string()))
}

pub(crate) fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }

    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(crate) fn run_wrapped_command(command: &[String], options: RunOptions) -> ExitCode {
    match run_wrapped_command_result(command, &options) {
        Ok(exit_code) => exit_code,
        Err(RunCommandError::Runner(error)) => {
            print_runner_error(&error);
            ExitCode::FAILURE
        }
        Err(RunCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(RunCommandError::Template(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RunCommandError::OutputRender(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_wrapped_command_result(
    command: &[String],
    options: &RunOptions,
) -> Result<ExitCode, RunCommandError> {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let identity = resolve_run_identity(&cwd, command, options, &config);
    let service_config = configured_service(&config, &identity);
    let run_templates = resolve_run_templates(command, options, service_config);
    let requires_output_preflight = has_blocking_auto_outputs(&config)?;
    let mut registry = open_optional_registry();
    let mut skip_ports = config.skip_ports.clone();
    let mut previous_port = None;

    let mut disable_registry = false;
    if let Some(registry) = registry.as_mut() {
        match prune_stale_leases_for_range(&cwd, &config, registry) {
            Ok(summary) if summary.total_leases() > 0 => {
                eprintln!(
                    "bindport: pruned {} stale registry entries under configured range pressure",
                    summary.total_leases()
                );
            }
            Ok(_) => {}
            Err(error) => {
                print_registry_warning("failed to prune stale registry leases", &error);
            }
        }

        match registry.active_ports() {
            Ok(active_ports) => skip_ports.extend(active_ports),
            Err(error) => {
                print_registry_warning("failed to read active registry ports", &error);
                registry_disabled_warning();
                disable_registry = true;
            }
        }

        if !disable_registry {
            match registry.previous_identity_port(&identity.identity_key) {
                Ok(port) => previous_port = port,
                Err(error) => {
                    print_registry_warning("failed to read previous identity port", &error);
                }
            }
        }
    }
    if disable_registry {
        registry = None;
        previous_port = None;
    }

    let mut retries = 0;

    loop {
        let allocation_hints = AllocationHints {
            preferred_port: previous_port,
            scan_start: identity.port_scan_start(config.port_range),
        };
        let port = allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints)?;
        let run_metadata = resolve_run_metadata(&identity, port, &run_templates)?;
        let child_command = resolved_child_command(command, &run_metadata)?;
        let command_display = child_command.join(" ");
        if requires_output_preflight {
            let Some(registry) = registry.as_mut() else {
                return Err(RenderCommandError::InvalidArgument(String::from(
                    "output rendering requires registry recording when on_failure = \"block\"",
                ))
                .into());
            };
            let pending_route =
                pending_route_record(&identity, port, &run_metadata, &command_display, &cwd);
            preflight_blocking_outputs(&cwd, &config, registry, pending_route)?;
        }
        let mut child = spawn_child_on_port(&child_command, port, &run_metadata.env)?;
        let attempt_started_at = Instant::now();
        let run = RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port,
            hostname: run_metadata.hostname.clone(),
            route_url: run_metadata.route_url.clone(),
            health_url: run_metadata.health_url.clone(),
            pid: child.pid(),
            command: command_display.clone(),
            cwd: cwd.clone(),
        };

        let started = if let Some(registry) = registry.as_mut() {
            match registry.record_run_started(&run) {
                Ok(started) => {
                    let events = RouteEventCollector::single(
                        RouteEventSource::CliRunner,
                        RouteEventKind::RouteStarted,
                    );
                    if let Err(error) =
                        auto_render_outputs_for_events(&cwd, &config, registry, &events)
                    {
                        print_auto_render_warning(&events.warning_context(), &error);
                    }
                    Some(started)
                }
                Err(
                    error @ RegistryError::PortConflict {
                        port: conflict_port,
                    },
                ) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    if retries < MAX_ALLOCATION_RETRIES {
                        eprintln!(
                            "bindport: warning: assigned port {conflict_port} was already recorded active; retrying with another port"
                        );
                        skip_ports.push(conflict_port);
                        retries += 1;
                        continue;
                    }
                    return Err(RenderCommandError::Registry(error).into());
                }
                Err(error) => {
                    print_registry_warning("failed to record run start", &error);
                    registry_disabled_warning();
                    None
                }
            }
        } else {
            None
        };

        let status = child.wait()?;
        let attempt_elapsed = attempt_started_at.elapsed();
        let exit_code = status_registry_exit_code(&status);

        if let (Some(registry), Some(started)) = (registry.as_mut(), started) {
            match registry.record_run_finished(started, exit_code) {
                Ok(()) => {
                    let events = RouteEventCollector::single(
                        RouteEventSource::CliRunner,
                        RouteEventKind::RouteFinished,
                    );
                    if let Err(error) =
                        auto_render_outputs_for_events(&cwd, &config, registry, &events)
                    {
                        print_auto_render_warning(&events.warning_context(), &error);
                    }
                }
                Err(error) => print_registry_warning("failed to record run finish", &error),
            }
        }

        if retries < MAX_ALLOCATION_RETRIES
            && should_retry_allocation(&status, attempt_elapsed, port)
        {
            eprintln!(
                "bindport: warning: assigned port {port} became unavailable; retrying with another port"
            );
            skip_ports.push(port);
            retries += 1;
            continue;
        }

        return Ok(status_to_exit_code(&status));
    }
}

pub(crate) fn prune_stale_leases_for_range(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
) -> Result<CleanSummary, RegistryError> {
    let limit = stale_lease_prune_limit(config.port_range, &config.skip_ports);
    let summary = registry.prune_oldest_stale_leases(
        config.port_range.start,
        config.port_range.end,
        limit,
        false,
    )?;

    if summary.total_leases() > 0 {
        let events = RouteEventCollector::single(
            RouteEventSource::StaleReconcile,
            RouteEventKind::RoutesRemoved,
        );
        if let Err(error) = auto_render_outputs_for_events(cwd, config, registry, &events) {
            print_auto_render_warning(&events.warning_context(), &error);
        }
    }

    Ok(summary)
}

pub(crate) fn stale_lease_prune_limit(range: PortRange, skip_ports: &[u16]) -> usize {
    let skipped_in_range = ports_in_range(skip_ports, range).len() as u32;
    let usable_ports = range.len().saturating_sub(skipped_in_range);

    (usable_ports / 2) as usize
}

#[derive(Debug)]
pub(crate) enum RunCommandError {
    Runner(RunnerError),
    Config(ConfigError),
    Template(TemplateError),
    OutputRender(RenderCommandError),
}

impl From<RunnerError> for RunCommandError {
    fn from(error: RunnerError) -> Self {
        Self::Runner(error)
    }
}

impl From<ConfigError> for RunCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<TemplateError> for RunCommandError {
    fn from(error: TemplateError) -> Self {
        Self::Template(error)
    }
}

impl From<RenderCommandError> for RunCommandError {
    fn from(error: RenderCommandError) -> Self {
        Self::OutputRender(error)
    }
}
