use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderCommand {
    Render,
    Help,
}

#[derive(Debug, Default)]
pub(crate) struct RenderCommandOptions {
    pub(crate) output: Option<String>,
    pub(crate) all: bool,
    pub(crate) dry_run: bool,
    pub(crate) repair: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderReport {
    Print,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderMode {
    Normal,
    Repair,
}

pub(crate) struct RenderInvocation<'a> {
    pub(crate) outputs: Vec<EffectiveOutputConfig>,
    pub(crate) dry_run: bool,
    pub(crate) mode: RenderMode,
    pub(crate) report: RenderReport,
    pub(crate) events: &'a RouteEventCollector,
}
pub(crate) fn run_render_command(args: &[String]) -> ExitCode {
    match run_render_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(RenderCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(RenderCommandError::OutputConfig(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport render [output] [--all] [--dry-run] [--repair]");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(RenderCommandError::Template(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::Render(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::File(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_render_command_result(args: &[String]) -> Result<(), RenderCommandError> {
    let (command, options) = parse_render_command(args)?;

    if command == RenderCommand::Help {
        print_render_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let outputs = configured_outputs(&config)?;
    let outputs = selected_outputs(outputs, options.output.as_deref())?;

    if outputs.is_empty() {
        println!("No enabled outputs configured.");
        return Ok(());
    }

    let mut registry = Registry::open_default()?;
    let mode = if options.repair {
        RenderMode::Repair
    } else {
        RenderMode::Normal
    };
    let events = RouteEventCollector::single(
        RouteEventSource::ManualRender,
        RouteEventKind::RenderRequested,
    );
    render_outputs_for_events(
        &cwd,
        &config,
        &mut registry,
        RenderInvocation {
            outputs,
            dry_run: options.dry_run,
            mode,
            report: RenderReport::Print,
            events: &events,
        },
    )?;

    Ok(())
}

pub(crate) fn auto_render_outputs_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    events: &RouteEventCollector,
) -> Result<usize, RenderCommandError> {
    if events.is_empty() {
        return Ok(0);
    }

    let mut outputs = Vec::new();
    for output in configured_outputs(config)?
        .into_iter()
        .filter(|output| output.auto_render)
    {
        let delay = registry.reserve_auto_render(&output.name, output.debounce_ms)?;
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        outputs.push(output);
    }

    render_outputs_for_events(
        cwd,
        config,
        registry,
        RenderInvocation {
            outputs,
            dry_run: false,
            mode: RenderMode::Normal,
            report: RenderReport::Quiet,
            events,
        },
    )
}

pub(crate) fn collect_stale_reconcile_event(
    registry: &mut Registry,
    events: &mut RouteEventCollector,
) -> Result<(), RegistryError> {
    if registry.reconcile_stale_active_leases()? > 0 {
        events.record(
            RouteEventSource::StaleReconcile,
            RouteEventKind::RoutesMarkedStale,
        );
    }

    Ok(())
}

pub(crate) fn has_blocking_auto_outputs(
    config: &ResolvedConfig,
) -> Result<bool, RenderCommandError> {
    Ok(configured_outputs(config)?
        .into_iter()
        .any(|output| output.auto_render && output.on_failure == OutputFailurePolicy::Block))
}

pub(crate) fn preflight_blocking_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    pending_route: RouteRecord,
) -> Result<(), RenderCommandError> {
    let outputs = configured_outputs(config)?
        .into_iter()
        .filter(|output| output.auto_render && output.on_failure == OutputFailurePolicy::Block)
        .collect::<Vec<_>>();

    if outputs.is_empty() {
        return Ok(());
    }

    let snapshot = registry.status_snapshot()?;
    let mut routes = route_records(snapshot.services);
    routes.retain(|route| route.key != pending_route.key);
    routes.push(pending_route);

    validate_render_outputs(cwd, config, registry, outputs, &routes)
}

pub(crate) fn validate_render_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &Registry,
    outputs: Vec<EffectiveOutputConfig>,
    routes: &[RouteRecord],
) -> Result<(), RenderCommandError> {
    let resolver = TemplateResolver::new(
        Some(project_template_dir(cwd, config)),
        global_template_dir(),
    );
    let base_dir = output_base_dir(cwd, config);

    for output in outputs {
        let template = resolver.resolve(&output.template, None)?;
        let render_config = OutputRenderConfig::from(&output);
        let delete_route_keys = delete_route_keys(&output, routes);
        let render_routes = routes
            .iter()
            .filter(|route| !delete_route_keys.contains(&route.key))
            .cloned()
            .collect::<Vec<_>>();
        let plan = render_output_routes(&render_config, &template.contents, &render_routes)?;
        let ownership = registry.output_file_ownership(&output.name)?;
        let write_ownership = ownership
            .iter()
            .map(|owned| AdapterOutputFileOwnership {
                path: owned.path.clone(),
                content_hash: owned.content_hash.clone(),
            })
            .collect::<Vec<_>>();

        verify_render_plan_targets(&plan, &base_dir, &write_ownership)?;
    }

    Ok(())
}

pub(crate) fn parse_render_command(
    args: &[String],
) -> Result<(RenderCommand, RenderCommandOptions), RenderCommandError> {
    let mut options = RenderCommandOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => return Ok((RenderCommand::Help, RenderCommandOptions::default())),
            "--all" => options.all = true,
            "--dry-run" => options.dry_run = true,
            "--repair" => options.repair = true,
            option if option.starts_with("--") => {
                return Err(RenderCommandError::InvalidArgument(format!(
                    "unknown render option `{option}`"
                )));
            }
            output => {
                if options.output.is_some() {
                    return Err(RenderCommandError::InvalidArgument(String::from(
                        "only one output name can be provided",
                    )));
                }
                options.output = Some(output.to_string());
            }
        }

        index += 1;
    }

    if options.all && options.output.is_some() {
        return Err(RenderCommandError::InvalidArgument(String::from(
            "--all cannot be combined with an output name",
        )));
    }
    if options.dry_run && options.repair {
        return Err(RenderCommandError::InvalidArgument(String::from(
            "--repair cannot be combined with --dry-run",
        )));
    }

    Ok((RenderCommand::Render, options))
}
