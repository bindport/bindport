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

pub(crate) fn configured_outputs(
    config: &ResolvedConfig,
) -> Result<Vec<EffectiveOutputConfig>, OutputConfigError> {
    config
        .loaded
        .as_ref()
        .map(|loaded| loaded.config.effective_outputs())
        .transpose()
        .map(|outputs| outputs.unwrap_or_default())
}

pub(crate) fn selected_outputs(
    outputs: Vec<EffectiveOutputConfig>,
    output_name: Option<&str>,
) -> Result<Vec<EffectiveOutputConfig>, RenderCommandError> {
    let Some(name) = output_name else {
        return Ok(outputs);
    };

    let selected = outputs
        .into_iter()
        .filter(|output| output.name == name)
        .collect::<Vec<_>>();

    if selected.is_empty() {
        return Err(RenderCommandError::InvalidArgument(format!(
            "output `{name}` is not configured or is disabled"
        )));
    }

    Ok(selected)
}

pub(crate) fn render_outputs_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    invocation: RenderInvocation<'_>,
) -> Result<usize, RenderCommandError> {
    if invocation.events.is_empty() {
        return Ok(0);
    }

    let mut events = invocation.events.clone();
    collect_stale_reconcile_event(registry, &mut events)?;

    let rendered = render_outputs(
        cwd,
        config,
        registry,
        invocation.outputs,
        invocation.dry_run,
        invocation.mode,
        invocation.report,
    )?;
    let mode = if invocation.dry_run {
        HookRunMode::DryRun
    } else {
        HookRunMode::Run
    };
    run_hooks_for_events(cwd, config, &events, rendered > 0, mode);

    Ok(rendered)
}

pub(crate) fn render_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    outputs: Vec<EffectiveOutputConfig>,
    dry_run: bool,
    mode: RenderMode,
    report: RenderReport,
) -> Result<usize, RenderCommandError> {
    let resolver = TemplateResolver::new(
        Some(project_template_dir(cwd, config)),
        global_template_dir(),
    );
    let snapshot = registry.status_snapshot()?;
    let routes = route_records(snapshot.services);
    let current_route_keys = routes
        .iter()
        .map(|route| route.key.clone())
        .collect::<BTreeSet<_>>();
    let base_dir = output_base_dir(cwd, config);
    let mut rendered = 0;

    for output in outputs {
        let template = resolver.resolve(&output.template, None)?;
        let render_config = OutputRenderConfig::from(&output);
        let delete_route_keys = delete_route_keys(&output, &routes);
        let render_routes = routes
            .iter()
            .filter(|route| !delete_route_keys.contains(&route.key))
            .cloned()
            .collect::<Vec<_>>();
        let plan = render_output_routes(&render_config, &template.contents, &render_routes)?;

        if dry_run {
            if report == RenderReport::Print {
                println!("would render {}: {} files", output.name, plan.files.len());
                for file in &plan.files {
                    println!("  {}", file.target);
                }
            }
            rendered += plan.files.len();
            continue;
        }

        let ownership = registry.output_file_ownership(&output.name)?;
        let write_ownership = ownership
            .iter()
            .map(|owned| AdapterOutputFileOwnership {
                path: owned.path.clone(),
                content_hash: owned.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        let removed = remove_output_files_for_lifecycle(
            registry,
            &output,
            &ownership,
            &current_route_keys,
            &delete_route_keys,
            &base_dir,
            &render_config,
        )?;
        let write_summary = match mode {
            RenderMode::Normal => {
                let written = write_render_plan(&plan, &base_dir, &write_ownership)?;
                record_written_output_files(registry, &output, &written)?;
                RenderWriteSummary {
                    written: written.len(),
                    external_modified: 0,
                }
            }
            RenderMode::Repair => {
                write_repair_render_plan(registry, &output, &plan, &base_dir, &write_ownership)?
            }
        };

        if report == RenderReport::Print {
            let verb = if mode == RenderMode::Repair {
                "repaired"
            } else {
                "rendered"
            };
            println!("{verb} {}: {} files", output.name, write_summary.written);
            if removed > 0 {
                println!("removed {}: {} files", output.name, removed);
            }
            if write_summary.external_modified > 0 {
                println!(
                    "preserved {}: {} externally modified files",
                    output.name, write_summary.external_modified
                );
            }
        }
        rendered += write_summary.written;
    }

    Ok(rendered)
}

pub(crate) struct RenderWriteSummary {
    pub(crate) written: usize,
    pub(crate) external_modified: usize,
}

pub(crate) fn write_repair_render_plan(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[AdapterOutputFileOwnership],
) -> Result<RenderWriteSummary, RenderCommandError> {
    let mut summary = RenderWriteSummary {
        written: 0,
        external_modified: 0,
    };

    for file in &plan.files {
        let single_file_plan = RenderPlan {
            output: plan.output.clone(),
            files: vec![file.clone()],
        };
        match write_render_plan(&single_file_plan, base_dir, ownership) {
            Ok(written) => {
                record_written_output_files(registry, output, &written)?;
                summary.written += written.len();
            }
            Err(OutputFileError::ExternalModified { path }) => {
                let expected_hash = ownership
                    .iter()
                    .find(|owned| owned.path == path)
                    .map(|owned| owned.content_hash.clone());
                registry.record_output_file(&OutputFileRecord {
                    output_name: output.name.clone(),
                    route_key: file.route_key.clone(),
                    rendered_path: path,
                    status: OutputFileStatus::Error,
                    reason: Some(String::from("external_modified")),
                    content_hash: expected_hash,
                    template_hash: None,
                    lease_id: None,
                    run_id: None,
                })?;
                summary.external_modified += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(summary)
}

pub(crate) fn record_written_output_files(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    written: &[bindport_adapters::WrittenOutputFile],
) -> Result<(), RegistryError> {
    for file in written {
        registry.record_output_file(&OutputFileRecord {
            output_name: output.name.clone(),
            route_key: file.route_key.clone(),
            rendered_path: file.path.clone(),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(file.content_hash.clone()),
            template_hash: None,
            lease_id: None,
            run_id: None,
        })?;
    }

    Ok(())
}

pub(crate) fn delete_route_keys(
    output: &EffectiveOutputConfig,
    routes: &[RouteRecord],
) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| {
            route_delete_state(route).is_some_and(|state| output.delete_on.contains(&state))
        })
        .map(|route| route.key.clone())
        .collect()
}

pub(crate) fn route_delete_state(route: &RouteRecord) -> Option<OutputDeleteState> {
    match route.state.as_str() {
        "stopped" => Some(OutputDeleteState::Stopped),
        "stale" => Some(OutputDeleteState::Stale),
        _ => None,
    }
}

pub(crate) fn remove_output_files_for_lifecycle(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    ownership: &[bindport_registry::OutputFileOwnership],
    current_route_keys: &BTreeSet<String>,
    delete_route_keys: &BTreeSet<String>,
    base_dir: &Path,
    render_config: &OutputRenderConfig,
) -> Result<usize, RenderCommandError> {
    let delete_removed = output.delete_on.contains(&OutputDeleteState::Removed);
    let candidates = ownership
        .iter()
        .filter(|owned| {
            delete_route_keys.contains(&owned.route_key)
                || (delete_removed && !current_route_keys.contains(&owned.route_key))
        })
        .map(|owned| AdapterRemovableOutputFile {
            route_key: owned.route_key.clone(),
            path: owned.path.clone(),
            content_hash: owned.content_hash.clone(),
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Ok(0);
    }

    let removed = remove_owned_output_files(&candidates, base_dir, &render_config.context)?;
    let mut removed_count = 0;

    for file in removed {
        let expected_hash = candidates
            .iter()
            .find(|candidate| candidate.route_key == file.route_key && candidate.path == file.path)
            .map(|candidate| candidate.content_hash.clone());
        let (status, reason, content_hash) = match file.status {
            AdapterOutputFileRemovalStatus::Removed => {
                removed_count += 1;
                (OutputFileStatus::Removed, None, None)
            }
            AdapterOutputFileRemovalStatus::Missing => (
                OutputFileStatus::Removed,
                Some(String::from("missing")),
                None,
            ),
            AdapterOutputFileRemovalStatus::ExternalModified => (
                OutputFileStatus::Error,
                Some(String::from("external_modified")),
                expected_hash,
            ),
        };

        registry.record_output_file(&OutputFileRecord {
            output_name: output.name.clone(),
            route_key: file.route_key,
            rendered_path: file.path,
            status,
            reason,
            content_hash,
            template_hash: None,
            lease_id: None,
            run_id: None,
        })?;
    }

    Ok(removed_count)
}

pub(crate) fn route_records(services: Vec<StatusService>) -> Vec<RouteRecord> {
    services
        .into_iter()
        .map(|service| {
            let key = status_service_route_key(&service);
            let updated_at = service
                .exited_at
                .clone()
                .unwrap_or_else(|| service.started_at.clone());

            RouteRecord {
                key,
                project: service.project,
                service: service.service,
                state: service.state,
                health: service.health,
                port: service.port,
                host: service.host,
                url: service.url,
                hostname: service.hostname,
                route_url: service.route_url,
                branch: service.branch,
                branch_label: service.branch_label,
                worktree_path: service.worktree_path,
                worktree_hash: service.worktree_hash,
                pid: service.pid,
                command: service.command,
                cwd: service.cwd,
                started_at: service.started_at,
                updated_at,
            }
        })
        .collect()
}

pub(crate) fn pending_route_record(
    identity: &ServiceIdentity,
    port: u16,
    metadata: &RunMetadata,
    command: &str,
    cwd: &Path,
) -> RouteRecord {
    let git = identity.git.as_ref();

    RouteRecord {
        key: identity.identity_key.clone(),
        project: identity.project.clone(),
        service: identity.service.clone(),
        state: String::from("active"),
        health: String::from("unknown"),
        port,
        host: String::from("127.0.0.1"),
        url: format!("http://127.0.0.1:{port}"),
        hostname: metadata.hostname.clone(),
        route_url: metadata.route_url.clone(),
        branch: git.map(|git| git.branch.clone()),
        branch_label: git.map(|git| git.branch_label.clone()),
        worktree_path: git.map(|git| git.worktree_path.display().to_string()),
        worktree_hash: git.map(|git| git.worktree_hash.clone()),
        pid: None,
        command: command.to_string(),
        cwd: cwd.display().to_string(),
        started_at: String::from("pending"),
        updated_at: String::from("pending"),
    }
}

pub(crate) fn output_base_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .to_path_buf()
}

#[derive(Debug)]
pub(crate) enum RenderCommandError {
    Config(ConfigError),
    OutputConfig(OutputConfigError),
    InvalidArgument(String),
    Registry(RegistryError),
    Template(AdapterTemplateError),
    Render(RenderError),
    File(OutputFileError),
}

impl std::fmt::Display for RenderCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::OutputConfig(error) => write!(f, "{error}"),
            Self::InvalidArgument(error) => write!(f, "{error}"),
            Self::Registry(error) => write!(f, "{error}"),
            Self::Template(error) => write!(f, "{error}"),
            Self::Render(error) => write!(f, "{error}"),
            Self::File(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RenderCommandError {}

impl From<ConfigError> for RenderCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<OutputConfigError> for RenderCommandError {
    fn from(error: OutputConfigError) -> Self {
        Self::OutputConfig(error)
    }
}

impl From<RegistryError> for RenderCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<AdapterTemplateError> for RenderCommandError {
    fn from(error: AdapterTemplateError) -> Self {
        Self::Template(error)
    }
}

impl From<RenderError> for RenderCommandError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<OutputFileError> for RenderCommandError {
    fn from(error: OutputFileError) -> Self {
        Self::File(error)
    }
}
