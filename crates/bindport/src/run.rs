use super::*;

#[derive(Debug, Default)]
pub(crate) struct RunOptions {
    pub(crate) service: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}

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

pub(crate) fn resolve_run_identity(
    cwd: &Path,
    command: &[String],
    options: &RunOptions,
    config: &ResolvedConfig,
) -> ServiceIdentity {
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let env_service = env::var(BINDPORT_SERVICE_ENV).ok();
    let config_project = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.project.as_deref());
    let config_service = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.configured_service_name_for_cwd(cwd));

    resolve_identity(IdentitySources {
        cwd,
        command,
        cli_project: None,
        cli_service: options.service.as_deref(),
        env_project: env_project.as_deref(),
        env_service: env_service.as_deref(),
        config_project,
        config_service,
    })
}

#[derive(Debug, Default)]
pub(crate) struct RunTemplates {
    pub(crate) command: Option<Vec<String>>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct RunMetadata {
    pub(crate) command: Option<Vec<String>>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) enum TemplateError {
    Unclosed {
        template: String,
    },
    Unopened {
        template: String,
    },
    UnknownPlaceholder {
        placeholder: String,
        template: String,
    },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unclosed { template } => {
                write!(f, "unclosed template placeholder in `{template}`")
            }
            Self::Unopened { template } => {
                write!(f, "unmatched `}}` in template `{template}`")
            }
            Self::UnknownPlaceholder {
                placeholder,
                template,
            } => {
                write!(
                    f,
                    "unknown or unavailable template placeholder `{placeholder}` in `{template}`"
                )
            }
        }
    }
}

impl std::error::Error for TemplateError {}

pub(crate) fn configured_service<'a>(
    config: &'a ResolvedConfig,
    identity: &ServiceIdentity,
) -> Option<&'a ServiceConfig> {
    config
        .loaded
        .as_ref()?
        .config
        .service_config(&identity.service)
}

pub(crate) fn resolve_run_templates(
    command: &[String],
    options: &RunOptions,
    service_config: Option<&ServiceConfig>,
) -> RunTemplates {
    let mut templates = RunTemplates::default();
    if command.is_empty() {
        templates.command = service_config.and_then(ServiceConfig::command_argv);
    }

    if let Some(env) = service_config.and_then(|service| service.env.as_ref()) {
        for (name, value) in env {
            if is_restricted_service_env_name(name) {
                eprintln!(
                    "bindport: ignoring restricted service env `{name}` from config; pass it explicitly with --env if needed"
                );
                continue;
            }
            templates.env.push((name.clone(), value.clone()));
        }
    }

    for (name, value) in &options.env {
        upsert_env_template(&mut templates.env, name.clone(), value.clone());
    }

    templates.hostname = options
        .hostname
        .clone()
        .or_else(|| env_template_value(BINDPORT_HOSTNAME_ENV))
        .or_else(|| service_config.and_then(|service| service.hostname.clone()));
    templates.route_url = options
        .route_url
        .clone()
        .or_else(|| env_template_value(BINDPORT_ROUTE_URL_ENV))
        .or_else(|| service_config.and_then(|service| service.route_url.clone()));
    templates.health_url = options
        .health_url
        .clone()
        .or_else(|| env_template_value(BINDPORT_HEALTH_URL_ENV))
        .or_else(|| service_config.and_then(|service| service.health_url.clone()));

    templates
}

pub(crate) fn upsert_env_template(env: &mut Vec<(String, String)>, name: String, value: String) {
    if let Some((_, existing)) = env.iter_mut().find(|(existing, _)| existing == &name) {
        *existing = value;
    } else {
        env.push((name, value));
    }
}

pub(crate) fn env_template_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

pub(crate) fn resolve_run_metadata(
    identity: &ServiceIdentity,
    port: u16,
    templates: &RunTemplates,
) -> Result<RunMetadata, TemplateError> {
    let base_values = TemplateValues::new(identity, port, None, None, None);
    let hostname = templates
        .hostname
        .as_deref()
        .map(|template| expand_template(template, &base_values))
        .transpose()?;
    let route_values = TemplateValues::new(identity, port, hostname.as_deref(), None, None);
    let route_url = templates
        .route_url
        .as_deref()
        .map(|template| expand_template(template, &route_values))
        .transpose()?
        .or_else(|| {
            hostname
                .as_ref()
                .map(|hostname| format!("http://{hostname}"))
        });
    let health_values = TemplateValues::new(
        identity,
        port,
        hostname.as_deref(),
        route_url.as_deref(),
        None,
    );
    let health_url = templates
        .health_url
        .as_deref()
        .map(|template| expand_template(template, &health_values))
        .transpose()?;
    let env_values = TemplateValues::new(
        identity,
        port,
        hostname.as_deref(),
        route_url.as_deref(),
        health_url.as_deref(),
    );
    let env = templates
        .env
        .iter()
        .map(|(name, template)| {
            expand_template(template, &env_values).map(|value| (name.clone(), value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let command = templates
        .command
        .as_ref()
        .map(|command| expand_command_templates(command, &env_values))
        .transpose()?;

    Ok(RunMetadata {
        command,
        hostname,
        route_url,
        health_url,
        env,
    })
}

pub(crate) fn expand_command_templates(
    command: &[String],
    values: &TemplateValues<'_>,
) -> Result<Vec<String>, TemplateError> {
    command
        .iter()
        .map(|template| expand_template(template, values))
        .collect()
}

pub(crate) fn resolved_child_command(
    explicit_command: &[String],
    metadata: &RunMetadata,
) -> Result<Vec<String>, RunnerError> {
    let command = if explicit_command.is_empty() {
        metadata.command.as_deref().unwrap_or(explicit_command)
    } else {
        explicit_command
    };

    if command
        .first()
        .is_none_or(|program| program.trim().is_empty())
    {
        return Err(RunnerError::NoCommand);
    }

    Ok(command.to_vec())
}

pub(crate) struct TemplateValues<'a> {
    pub(crate) identity: &'a ServiceIdentity,
    pub(crate) port: u16,
    pub(crate) hostname: Option<&'a str>,
    pub(crate) route_url: Option<&'a str>,
    pub(crate) health_url: Option<&'a str>,
    pub(crate) host: &'static str,
    pub(crate) url: String,
}

impl<'a> TemplateValues<'a> {
    pub(crate) fn new(
        identity: &'a ServiceIdentity,
        port: u16,
        hostname: Option<&'a str>,
        route_url: Option<&'a str>,
        health_url: Option<&'a str>,
    ) -> Self {
        let host = "127.0.0.1";

        Self {
            identity,
            port,
            hostname,
            route_url,
            health_url,
            host,
            url: format!("http://{host}:{port}"),
        }
    }

    pub(crate) fn value(&self, name: &str) -> Option<String> {
        match name {
            "port" => Some(self.port.to_string()),
            "host" => Some(self.host.to_string()),
            "url" => Some(self.url.clone()),
            "project" => Some(self.identity.project.clone()),
            "service" => Some(self.identity.service.clone()),
            "hostname" => self.hostname.map(str::to_string),
            "route_url" => Some(self.route_url.unwrap_or(&self.url).to_string()),
            "health_url" => self.health_url.map(str::to_string),
            "branch" | "branch_label" => Some(
                self.identity
                    .git
                    .as_ref()
                    .map(|git| git.branch_label.clone())
                    .unwrap_or_else(|| String::from("no-branch")),
            ),
            "git_branch" => Some(
                self.identity
                    .git
                    .as_ref()
                    .map(|git| git.branch.clone())
                    .unwrap_or_else(|| String::from("no-branch")),
            ),
            "worktree" | "worktree_label" => Some(
                self.identity
                    .git
                    .as_ref()
                    .and_then(|git| {
                        git.worktree_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(normalize_branch_label)
                    })
                    .unwrap_or_else(|| normalize_branch_label(&self.identity.project)),
            ),
            "worktree_hash" => Some(
                self.identity
                    .git
                    .as_ref()
                    .map(|git| git.worktree_hash.clone())
                    .unwrap_or_else(|| String::from("no-git")),
            ),
            _ => None,
        }
    }
}

pub(crate) fn expand_template(
    template: &str,
    values: &TemplateValues<'_>,
) -> Result<String, TemplateError> {
    let mut output = String::new();
    let mut chars = template.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    output.push('{');
                    continue;
                }

                let mut placeholder = String::new();
                let mut closed = false;

                for character in chars.by_ref() {
                    if character == '}' {
                        closed = true;
                        break;
                    }
                    placeholder.push(character);
                }

                if !closed {
                    return Err(TemplateError::Unclosed {
                        template: template.to_string(),
                    });
                }

                let value = values.value(&placeholder).ok_or_else(|| {
                    TemplateError::UnknownPlaceholder {
                        placeholder: placeholder.clone(),
                        template: template.to_string(),
                    }
                })?;
                output.push_str(&value);
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    output.push('}');
                    continue;
                }

                return Err(TemplateError::Unopened {
                    template: template.to_string(),
                });
            }
            _ => output.push(character),
        }
    }

    Ok(output)
}

pub(crate) fn status_to_exit_code(status: &ExitStatus) -> ExitCode {
    match status_registry_exit_code(status) {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

pub(crate) fn status_registry_exit_code(status: &ExitStatus) -> Option<i32> {
    status.code().or_else(|| signal_exit_code(status))
}

pub(crate) fn should_retry_allocation(status: &ExitStatus, elapsed: Duration, port: u16) -> bool {
    matches!(status.code(), Some(code) if code != 0)
        && elapsed <= ALLOCATION_RETRY_WINDOW
        && !is_port_available(port)
}

#[cfg(unix)]
pub(crate) fn signal_exit_code(status: &ExitStatus) -> Option<i32> {
    status.signal().map(|signal| 128 + signal)
}

#[cfg(not(unix))]
pub(crate) fn signal_exit_code(_status: &ExitStatus) -> Option<i32> {
    None
}
