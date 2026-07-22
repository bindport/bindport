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
        Err(RunCommandError::ExecutionContext(error)) => {
            eprintln!("bindport: {error}");
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

struct ServiceExecutionContext {
    cwd: PathBuf,
    local_bin_dirs: Vec<PathBuf>,
}

fn resolve_service_execution_context(
    invoker_cwd: &Path,
    config: &ResolvedConfig,
    service_name: &str,
    service_config: Option<&ServiceConfig>,
) -> Result<ServiceExecutionContext, ServiceExecutionContextError> {
    let Some(service_path) = service_config.and_then(|service| service.path.as_deref()) else {
        return Ok(ServiceExecutionContext {
            cwd: invoker_cwd
                .canonicalize()
                .unwrap_or_else(|_| invoker_cwd.to_path_buf()),
            local_bin_dirs: Vec::new(),
        });
    };
    let config_root = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(invoker_cwd);
    let config_root = config_root
        .canonicalize()
        .unwrap_or_else(|_| config_root.to_path_buf());
    let configured_path = config_root.join(service_path);
    let service_root = configured_path.canonicalize().map_err(|source| {
        ServiceExecutionContextError::InvalidPath {
            service: service_name.to_string(),
            path: configured_path.clone(),
            source,
        }
    })?;
    if !service_root.is_dir() {
        return Err(ServiceExecutionContextError::NotDirectory {
            service: service_name.to_string(),
            path: service_root,
        });
    }
    if !service_root.starts_with(&config_root) {
        return Err(ServiceExecutionContextError::OutsideProject {
            service: service_name.to_string(),
            path: service_root,
            project_root: config_root,
        });
    }

    let boundary = package_workspace_root(&service_root, &config_root).unwrap_or(config_root);
    let mut local_bin_dirs = Vec::new();
    for directory in service_root.ancestors() {
        let bin_dir = directory.join("node_modules").join(".bin");
        if bin_dir.is_dir() {
            local_bin_dirs.push(bin_dir);
        }
        if directory == boundary {
            break;
        }
    }

    Ok(ServiceExecutionContext {
        cwd: service_root,
        local_bin_dirs,
    })
}

fn child_environment(
    configured_env: &[(String, String)],
    local_bin_dirs: &[PathBuf],
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, ServiceExecutionContextError> {
    let mut child_env = configured_env
        .iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect::<Vec<_>>();
    if local_bin_dirs.is_empty() {
        return Ok(child_env);
    }

    let configured_path = configured_env
        .iter()
        .find(|(name, _)| name == "PATH")
        .map(|(_, value)| std::ffi::OsString::from(value));
    let ambient_path = configured_path.or_else(|| env::var_os("PATH"));
    let path_entries = local_bin_dirs.iter().cloned().chain(
        ambient_path
            .as_deref()
            .into_iter()
            .flat_map(env::split_paths),
    );
    let path = env::join_paths(path_entries)
        .map_err(|source| ServiceExecutionContextError::InvalidPathEnvironment { source })?;
    child_env.retain(|(name, _)| name != "PATH");
    child_env.push(("PATH".into(), path));

    Ok(child_env)
}

pub(crate) fn run_wrapped_command_result(
    command: &[String],
    options: &RunOptions,
) -> Result<ExitCode, RunCommandError> {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let identity = resolve_run_identity(&cwd, command, options, &config);
    let service_config = configured_service(&config, &identity);
    let execution_context =
        resolve_service_execution_context(&cwd, &config, &identity.service, service_config)?;
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
        let child_env = child_environment(&run_metadata.env, &execution_context.local_bin_dirs)?;
        let command_display = child_command.join(" ");
        if requires_output_preflight {
            let Some(registry) = registry.as_mut() else {
                return Err(RenderCommandError::InvalidArgument(String::from(
                    "output rendering requires registry recording when on_failure = \"block\"",
                ))
                .into());
            };
            let pending_route = pending_route_record(
                &identity,
                port,
                &run_metadata,
                &command_display,
                &execution_context.cwd,
            );
            preflight_blocking_outputs(&cwd, &config, registry, pending_route)?;
        }
        let mut child = spawn_child_on_port_with_context(
            &child_command,
            port,
            Some(&execution_context.cwd),
            &child_env,
        )?;
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
            cwd: execution_context.cwd.clone(),
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
