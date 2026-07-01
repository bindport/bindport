// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};

use bindport_adapters::{
    AdapterKind, OutputFileError, OutputFileOwnership as AdapterOutputFileOwnership,
    OutputFileRemovalStatus as AdapterOutputFileRemovalStatus, OutputRenderConfig,
    RemovableOutputFile as AdapterRemovableOutputFile, RenderError, RenderPlan, RouteRecord,
    TemplateError as AdapterTemplateError, TemplateResolver, TemplateSource,
    remove_owned_output_files, render_output_routes, render_plan_paths, verify_render_plan_targets,
    write_render_plan,
};
use bindport_core::{
    APPLIED_CONFIG_KEYS, BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, BindPortConfig, ConfigError,
    ConfigSource, ConfiguredServiceSource, DEFAULT_HOOK_TIMEOUT_MS, DEFAULT_PORT_RANGE,
    DEFAULT_SKIP_PORTS, EffectiveOutputConfig, FALLBACK_CONFIG_FILE, HookCommandConfig, HookEvent,
    IdentitySources, LoadedConfig, OutputConfigError, OutputDeleteState, OutputFailurePolicy,
    PortRange, SERVICE_NAME, ServiceConfig, ServiceIdentity, default_fallback_config,
    detect_git_identity, discover_config, normalize_branch_label, resolve_identity,
};
use bindport_dashboard::{
    DashboardCleanCallback, DashboardOptions, DashboardServer, DashboardStatusCallback,
};
use bindport_registry::{
    CleanState, CleanSummary, OutputFileRecord, OutputFileStatus, REGISTRY_PATH_ENV, Registry,
    RegistryError, RunStart, StartedRun, StatusService, default_registry_path,
    status_service_route_key,
};
use bindport_runner::{
    AllocationHints, RunnerError, allocate_port_with_hints, is_port_available, spawn_child_on_port,
};

const DOCTOR_PORT_DISPLAY_LIMIT: usize = 10;
const DOCTOR_MAX_LISTENER_PROBES: u32 = 1024;
const ALLOCATION_RETRY_WINDOW: Duration = Duration::from_secs(2);
const MAX_ALLOCATION_RETRIES: usize = 1;
const DASHBOARD_HOST_ENV: &str = "BINDPORT_DASHBOARD_HOST";
const DASHBOARD_PORT_ENV: &str = "BINDPORT_DASHBOARD_PORT";
const DASHBOARD_AUTH_REQUIRED_ENV: &str = "BINDPORT_DASHBOARD_AUTH_REQUIRED";
const DASHBOARD_REGISTER_SERVICE_ENV: &str = "BINDPORT_DASHBOARD_REGISTER_SERVICE";
const DASHBOARD_TOKEN_ENV: &str = "BINDPORT_DASHBOARD_TOKEN";
const DASHBOARD_STATIC_DIR_ENV: &str = "BINDPORT_DASHBOARD_STATIC_DIR";
const BINDPORT_HOSTNAME_ENV: &str = "BINDPORT_HOSTNAME";
const BINDPORT_ROUTE_URL_ENV: &str = "BINDPORT_ROUTE_URL";
const BINDPORT_HEALTH_URL_ENV: &str = "BINDPORT_HEALTH_URL";
const DASHBOARD_STATE_FILE: &str = "dashboard.state";
const DASHBOARD_LOG_FILE: &str = "dashboard.log";
const HOOK_TRUST_FILE: &str = "hooks-trust.json";
const HOOK_TRUST_SCHEMA_VERSION: &str = "0.1";

fn main() -> ExitCode {
    run(env::args().skip(1))
}

fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let args = args.into_iter().collect::<Vec<_>>();

    match args.first().map(String::as_str) {
        None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("{SERVICE_NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("status") => {
            if args.iter().any(|arg| arg == "--json") {
                print_status_json()
            } else {
                print_status()
            }
        }
        Some("open") => run_open_command(&args[1..]),
        Some("clean") => clean_registry(&args[1..]),
        Some("config") => run_config_command(&args[1..]),
        Some("doctor") => run_doctor_command(&args[1..]),
        Some("dashboard") => run_dashboard(&args[1..]),
        Some("hooks") => run_hooks_command(&args[1..]),
        Some("render") => run_render_command(&args[1..]),
        Some("templates") => run_template_command(&args[1..]),
        Some("init") => init_fallback_config(),
        Some("--") => run_wrapped_command(&args[1..], RunOptions::default()),
        Some("run") => run_subcommand(&args[1..]),
        Some(command) => {
            eprintln!("unknown bindport command: {command}");
            eprintln!("run `bindport --help` for available bootstrap commands");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Default)]
struct RunOptions {
    service: Option<String>,
    hostname: Option<String>,
    route_url: Option<String>,
    health_url: Option<String>,
    env: Vec<(String, String)>,
}

fn run_subcommand(args: &[String]) -> ExitCode {
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

fn parse_run_options(args: &[String]) -> Result<(RunOptions, &[String]), String> {
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

fn parse_env_assignment(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once('=')
        .ok_or_else(|| format!("invalid env assignment `{value}`; expected NAME=VALUE"))?;
    let name = name.trim();
    if !valid_env_name(name) {
        return Err(format!("invalid env variable name `{name}`"));
    }

    Ok((name.to_string(), value.to_string()))
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }

    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn run_wrapped_command(command: &[String], options: RunOptions) -> ExitCode {
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

fn run_wrapped_command_result(
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

fn prune_stale_leases_for_range(
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

fn stale_lease_prune_limit(range: PortRange, skip_ports: &[u16]) -> usize {
    let skipped_in_range = ports_in_range(skip_ports, range).len() as u32;
    let usable_ports = range.len().saturating_sub(skipped_in_range);

    (usable_ports / 2) as usize
}

#[derive(Debug)]
enum RunCommandError {
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

struct ResolvedConfig {
    loaded: Option<LoadedConfig>,
    fallback_path: Option<PathBuf>,
    port_range: PortRange,
    skip_ports: Vec<u16>,
}

fn resolve_config(cwd: &Path) -> Result<ResolvedConfig, ConfigError> {
    let fallback_path = fallback_config_path().ok();
    let loaded = discover_config(cwd, fallback_path.as_deref())?;
    let port_range = loaded
        .as_ref()
        .map(LoadedConfig::port_range)
        .transpose()?
        .unwrap_or(DEFAULT_PORT_RANGE);
    let skip_ports = loaded
        .as_ref()
        .map(LoadedConfig::skip_ports)
        .unwrap_or_else(|| DEFAULT_SKIP_PORTS.to_vec());

    Ok(ResolvedConfig {
        loaded,
        fallback_path,
        port_range,
        skip_ports,
    })
}

fn resolve_run_identity(
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
struct RunTemplates {
    command: Option<Vec<String>>,
    hostname: Option<String>,
    route_url: Option<String>,
    health_url: Option<String>,
    env: Vec<(String, String)>,
}

#[derive(Debug)]
struct RunMetadata {
    command: Option<Vec<String>>,
    hostname: Option<String>,
    route_url: Option<String>,
    health_url: Option<String>,
    env: Vec<(String, String)>,
}

#[derive(Debug)]
enum TemplateError {
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

fn configured_service<'a>(
    config: &'a ResolvedConfig,
    identity: &ServiceIdentity,
) -> Option<&'a ServiceConfig> {
    config
        .loaded
        .as_ref()?
        .config
        .service_config(&identity.service)
}

fn resolve_run_templates(
    command: &[String],
    options: &RunOptions,
    service_config: Option<&ServiceConfig>,
) -> RunTemplates {
    let mut templates = RunTemplates::default();
    if command.is_empty() {
        templates.command = service_config.and_then(ServiceConfig::command_argv);
    }

    if let Some(env) = service_config.and_then(|service| service.env.as_ref()) {
        templates.env.extend(
            env.iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
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

fn upsert_env_template(env: &mut Vec<(String, String)>, name: String, value: String) {
    if let Some((_, existing)) = env.iter_mut().find(|(existing, _)| existing == &name) {
        *existing = value;
    } else {
        env.push((name, value));
    }
}

fn env_template_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn resolve_run_metadata(
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

fn expand_command_templates(
    command: &[String],
    values: &TemplateValues<'_>,
) -> Result<Vec<String>, TemplateError> {
    command
        .iter()
        .map(|template| expand_template(template, values))
        .collect()
}

fn resolved_child_command(
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

struct TemplateValues<'a> {
    identity: &'a ServiceIdentity,
    port: u16,
    hostname: Option<&'a str>,
    route_url: Option<&'a str>,
    health_url: Option<&'a str>,
    host: &'static str,
    url: String,
}

impl<'a> TemplateValues<'a> {
    fn new(
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

    fn value(&self, name: &str) -> Option<String> {
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

fn expand_template(template: &str, values: &TemplateValues<'_>) -> Result<String, TemplateError> {
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

fn fallback_config_path() -> io::Result<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(config_home)
            .join(SERVICE_NAME)
            .join(FALLBACK_CONFIG_FILE));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join(SERVICE_NAME)
            .join(FALLBACK_CONFIG_FILE));
    }

    if let Some(appdata) = env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(appdata)
            .join(SERVICE_NAME)
            .join(FALLBACK_CONFIG_FILE));
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not determine config directory; set XDG_CONFIG_HOME, HOME, or APPDATA",
    ))
}

fn open_optional_registry() -> Option<Registry> {
    match Registry::open_default() {
        Ok(registry) => Some(registry),
        Err(error) => {
            print_registry_warning("registry unavailable", &error);
            registry_disabled_warning();
            None
        }
    }
}

fn run_dashboard(args: &[String]) -> ExitCode {
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

fn run_dashboard_result(args: &[String]) -> Result<(), DashboardCommandError> {
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

fn serve_dashboard(options: &DashboardCliOptions) -> Result<(), DashboardCommandError> {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
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
    let _registration = register_dashboard_service(register_service, &server, &host, &cwd);
    println!("dashboard: {}", server.url());
    io::stdout().flush().ok();
    server.serve()?;

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardCommand {
    Serve,
    Start,
    Status,
    Stop,
    Help,
}

#[derive(Debug, Default)]
struct DashboardCliOptions {
    host: Option<Ipv4Addr>,
    port: Option<u16>,
    auth_required: Option<bool>,
    register_service: Option<bool>,
    token: Option<String>,
    token_env: Option<String>,
    allowed_hosts: Vec<String>,
    static_dir: Option<PathBuf>,
    serve_args: Vec<String>,
}

impl DashboardCliOptions {
    fn token_env_name(&self) -> &str {
        self.token_env.as_deref().unwrap_or(DASHBOARD_TOKEN_ENV)
    }
}

fn parse_dashboard_command(
    args: &[String],
) -> Result<(DashboardCommand, DashboardCliOptions), DashboardCommandError> {
    let (command, option_args) = match args.first().map(String::as_str) {
        None => (DashboardCommand::Serve, args),
        Some("serve") => (DashboardCommand::Serve, &args[1..]),
        Some("start") => (DashboardCommand::Start, &args[1..]),
        Some("status") => (DashboardCommand::Status, &args[1..]),
        Some("stop") => (DashboardCommand::Stop, &args[1..]),
        Some("--help" | "-h") => {
            return Ok((DashboardCommand::Help, DashboardCliOptions::default()));
        }
        Some(_) => (DashboardCommand::Serve, args),
    };

    let options = parse_dashboard_options(option_args)?;
    Ok((command, options))
}

fn parse_dashboard_options(args: &[String]) -> Result<DashboardCliOptions, DashboardCommandError> {
    let mut options = DashboardCliOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--host" => {
                let value = dashboard_option_value(args, &mut index, "--host")?;
                options.host = Some(value.parse::<Ipv4Addr>().map_err(|_| {
                    DashboardCommandError::InvalidArgument(format!(
                        "invalid dashboard host `{value}`"
                    ))
                })?);
                options.serve_args.extend([String::from("--host"), value]);
            }
            "--port" => {
                let value = dashboard_option_value(args, &mut index, "--port")?;
                options.port = Some(value.parse::<u16>().map_err(|_| {
                    DashboardCommandError::InvalidArgument(format!(
                        "invalid dashboard port `{value}`"
                    ))
                })?);
                options.serve_args.extend([String::from("--port"), value]);
            }
            "--auth" => {
                let value = dashboard_option_value(args, &mut index, "--auth")?;
                options.auth_required = Some(parse_dashboard_auth_mode(&value)?);
                options.serve_args.extend([String::from("--auth"), value]);
            }
            "--auth-required" => {
                options.auth_required = Some(true);
                options.serve_args.push(String::from("--auth-required"));
            }
            "--no-auth" => {
                options.auth_required = Some(false);
                options.serve_args.push(String::from("--no-auth"));
            }
            "--register-service" => {
                options.register_service = Some(true);
                options.serve_args.push(String::from("--register-service"));
            }
            "--no-register-service" => {
                options.register_service = Some(false);
                options
                    .serve_args
                    .push(String::from("--no-register-service"));
            }
            "--token" => {
                let value = dashboard_option_value(args, &mut index, "--token")?;
                options.token = Some(value);
            }
            "--token-env" => {
                let value = dashboard_option_value(args, &mut index, "--token-env")?;
                options.token_env = Some(value.clone());
                options
                    .serve_args
                    .extend([String::from("--token-env"), value]);
            }
            "--allowed-host" => {
                let value = dashboard_option_value(args, &mut index, "--allowed-host")?;
                options.allowed_hosts.push(value.clone());
                options
                    .serve_args
                    .extend([String::from("--allowed-host"), value]);
            }
            "--static-dir" => {
                let value = dashboard_option_value(args, &mut index, "--static-dir")?;
                options.static_dir = Some(PathBuf::from(&value));
                options
                    .serve_args
                    .extend([String::from("--static-dir"), value]);
            }
            unknown => {
                return Err(DashboardCommandError::InvalidArgument(format!(
                    "unknown dashboard option `{unknown}`"
                )));
            }
        }

        index += 1;
    }

    Ok(options)
}

fn dashboard_option_value(
    args: &[String],
    index: &mut usize,
    option: &'static str,
) -> Result<String, DashboardCommandError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| DashboardCommandError::InvalidArgument(format!("{option} requires a value")))
}

fn parse_dashboard_auth_mode(value: &str) -> Result<bool, DashboardCommandError> {
    parse_dashboard_bool(value, "dashboard auth mode")
}

fn parse_dashboard_bool(value: &str, setting: &str) -> Result<bool, DashboardCommandError> {
    match value {
        "required" | "require" | "enabled" | "true" | "1" | "yes" => Ok(true),
        "disabled" | "disable" | "false" | "0" | "no" => Ok(false),
        _ => Err(DashboardCommandError::InvalidArgument(format!(
            "invalid {setting} `{value}`"
        ))),
    }
}

fn resolve_dashboard_options(
    config: &ResolvedConfig,
    cli: &DashboardCliOptions,
    skip_ports: Vec<u16>,
) -> Result<DashboardOptions, DashboardCommandError> {
    let dashboard_config = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.dashboard.as_ref());
    let auth_config = dashboard_config.and_then(|dashboard| dashboard.auth.as_ref());
    let env_host = env_dashboard_host()?;
    let env_port = env_dashboard_port()?;
    let env_auth_required = env_dashboard_auth_required()?;

    let host = match cli.host.or(env_host).or_else(|| {
        dashboard_config
            .and_then(|dashboard| dashboard.host.as_deref())
            .and_then(|host| host.parse::<Ipv4Addr>().ok())
    }) {
        Some(host) => host,
        None => DashboardOptions::default().host,
    };
    let preferred_port = cli
        .port
        .or(env_port)
        .or_else(|| dashboard_config.and_then(|dashboard| dashboard.port))
        .unwrap_or(DashboardOptions::default().preferred_port);
    let auth_required = cli
        .auth_required
        .or(env_auth_required)
        .or_else(|| auth_config.and_then(|auth| auth.required))
        .unwrap_or(false);
    if !host.is_loopback() && !auth_required {
        return Err(DashboardCommandError::InvalidArgument(format!(
            "binding the dashboard to {host} requires auth; pass --auth-required with a token or use --host 127.0.0.1"
        )));
    }
    let token_env = cli
        .token_env
        .as_deref()
        .or_else(|| auth_config.and_then(|auth| auth.token_env.as_deref()))
        .unwrap_or(DASHBOARD_TOKEN_ENV);
    let token = cli
        .token
        .clone()
        .or_else(|| env::var(token_env).ok())
        .or_else(|| auth_config.and_then(|auth| auth.token.clone()));

    if auth_required && token.is_none() {
        return Err(DashboardCommandError::MissingToken {
            source_name: token_env.to_string(),
        });
    }

    let mut allowed_hosts = DashboardOptions::default().allowed_hosts;
    if let Some(configured) = dashboard_config.and_then(|dashboard| dashboard.allowed_hosts.clone())
    {
        allowed_hosts.extend(configured);
    }
    allowed_hosts.extend(cli.allowed_hosts.clone());
    allowed_hosts.sort();
    allowed_hosts.dedup();

    let static_dir = cli
        .static_dir
        .clone()
        .or_else(|| env::var_os(DASHBOARD_STATIC_DIR_ENV).map(PathBuf::from));

    Ok(DashboardOptions {
        host,
        preferred_port,
        fallback_range: config.port_range,
        skip_ports,
        allowed_hosts,
        auth: bindport_dashboard::DashboardAuth {
            required: auth_required,
            token,
        },
        static_dir,
        clean_callback: None,
        status_callback: None,
    })
}

fn resolve_dashboard_registration(
    config: &ResolvedConfig,
    cli: &DashboardCliOptions,
) -> Result<bool, DashboardCommandError> {
    let dashboard_config = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.dashboard.as_ref());
    let env_register_service = env_dashboard_register_service()?;

    Ok(cli
        .register_service
        .or(env_register_service)
        .or_else(|| dashboard_config.and_then(|dashboard| dashboard.register_service))
        .unwrap_or(false))
}

fn env_dashboard_host() -> Result<Option<Ipv4Addr>, DashboardCommandError> {
    env::var(DASHBOARD_HOST_ENV)
        .ok()
        .map(|value| {
            value.parse::<Ipv4Addr>().map_err(|_| {
                DashboardCommandError::InvalidArgument(format!(
                    "invalid {DASHBOARD_HOST_ENV} host `{value}`"
                ))
            })
        })
        .transpose()
}

fn env_dashboard_port() -> Result<Option<u16>, DashboardCommandError> {
    env::var(DASHBOARD_PORT_ENV)
        .ok()
        .map(|value| {
            value.parse::<u16>().map_err(|_| {
                DashboardCommandError::InvalidArgument(format!(
                    "invalid {DASHBOARD_PORT_ENV} port `{value}`"
                ))
            })
        })
        .transpose()
}

fn env_dashboard_auth_required() -> Result<Option<bool>, DashboardCommandError> {
    env::var(DASHBOARD_AUTH_REQUIRED_ENV)
        .ok()
        .map(|value| parse_dashboard_auth_mode(&value))
        .transpose()
}

fn env_dashboard_register_service() -> Result<Option<bool>, DashboardCommandError> {
    env::var(DASHBOARD_REGISTER_SERVICE_ENV)
        .ok()
        .map(|value| parse_dashboard_bool(&value, DASHBOARD_REGISTER_SERVICE_ENV))
        .transpose()
}

fn start_dashboard_service(options: &DashboardCliOptions) -> Result<(), DashboardCommandError> {
    if let Some(state) = read_dashboard_state()? {
        if dashboard_process_is_running(&state) {
            println!("dashboard running: {} pid {}", state.url, state.pid);
            return Ok(());
        }
        remove_dashboard_state().ok();
    }

    let stderr = open_dashboard_log()?;
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("dashboard")
        .arg("serve")
        .args(&options.serve_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr));

    if let Some(token) = options.token.as_ref() {
        command.env(options.token_env_name(), token);
    }

    let mut child = command.spawn()?;
    let pid = child.id();
    let process_start_time = process_start_time(pid);
    let stdout = child.stdout.take().ok_or_else(|| {
        DashboardCommandError::Io(io::Error::other("failed to capture dashboard stdout"))
    })?;
    let mut stdout = io::BufReader::new(stdout);
    let mut line = String::new();
    stdout.read_line(&mut line)?;
    let url = match line.trim().strip_prefix("dashboard: ") {
        Some(url) => url.to_string(),
        None => return Err(DashboardCommandError::Io(dashboard_start_error())),
    };
    let state = DashboardServiceState {
        pid,
        url,
        process_start_time,
    };
    write_dashboard_state(&state)?;
    println!("dashboard started: {} pid {}", state.url, state.pid);

    Ok(())
}

fn dashboard_start_error() -> io::Error {
    let message = dashboard_log_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let message = message.trim();

    if message.is_empty() {
        return io::Error::other("dashboard did not start");
    }

    io::Error::other(format!(
        "dashboard did not start: {}",
        message.chars().take(500).collect::<String>()
    ))
}

fn print_dashboard_service_status() -> Result<(), DashboardCommandError> {
    let Some(state) = read_dashboard_state()? else {
        println!("dashboard stopped");
        return Ok(());
    };

    if dashboard_process_is_running(&state) {
        println!("dashboard running: {} pid {}", state.url, state.pid);
    } else if process_is_running(state.pid) {
        println!(
            "dashboard stale: pid {} no longer matches dashboard",
            state.pid
        );
    } else {
        println!("dashboard stale: {} pid {}", state.url, state.pid);
    }

    Ok(())
}

fn stop_dashboard_service() -> Result<(), DashboardCommandError> {
    let Some(state) = read_dashboard_state()? else {
        println!("dashboard stopped");
        return Ok(());
    };

    if dashboard_process_is_running(&state) {
        terminate_process(state.pid)?;
        println!("dashboard stopped: pid {}", state.pid);
    } else if process_is_running(state.pid) {
        println!(
            "dashboard state removed: pid {} no longer matches dashboard",
            state.pid
        );
    } else {
        println!("dashboard state removed: stale pid {}", state.pid);
    }
    remove_dashboard_state()?;

    Ok(())
}

struct DashboardRegistration {
    registry: Option<Registry>,
    started: Option<StartedRun>,
}

impl DashboardRegistration {
    fn inactive() -> Self {
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

fn register_dashboard_service(
    enabled: bool,
    server: &DashboardServer,
    host: &str,
    cwd: &Path,
) -> DashboardRegistration {
    if !enabled {
        return DashboardRegistration::inactive();
    }

    let Some(mut registry) = open_optional_registry() else {
        return DashboardRegistration::inactive();
    };
    let identity = resolve_identity(IdentitySources {
        cwd,
        command: &[],
        cli_project: Some(SERVICE_NAME),
        cli_service: Some("dashboard"),
        env_project: None,
        env_service: None,
        config_project: None,
        config_service: None,
    });
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

fn redacted_dashboard_command() -> String {
    let mut args = env::args();
    let mut redacted = Vec::new();

    while let Some(arg) = args.next() {
        redacted.push(arg.clone());
        if arg == "--token" && args.next().is_some() {
            redacted.push(String::from("***"));
        }
    }

    redacted.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DashboardServiceState {
    pid: u32,
    url: String,
    process_start_time: Option<u64>,
}

fn read_dashboard_state() -> Result<Option<DashboardServiceState>, DashboardCommandError> {
    let path = dashboard_state_path()?;
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)?;
    let mut pid = None;
    let mut url = None;
    let mut process_start_time = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("pid=") {
            pid = value.trim().parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("url=") {
            url = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("process_start_time=") {
            process_start_time = value.trim().parse::<u64>().ok();
        }
    }

    Ok(pid.zip(url).map(|(pid, url)| DashboardServiceState {
        pid,
        url,
        process_start_time,
    }))
}

fn write_dashboard_state(state: &DashboardServiceState) -> Result<(), DashboardCommandError> {
    let path = dashboard_state_path()?;
    create_dashboard_state_dir()?;
    let mut contents = format!("pid={}\nurl={}\n", state.pid, state.url);
    if let Some(process_start_time) = state.process_start_time {
        contents.push_str(&format!("process_start_time={process_start_time}\n"));
    }
    fs::write(path, contents)?;
    Ok(())
}

fn remove_dashboard_state() -> io::Result<()> {
    match fs::remove_file(dashboard_state_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn dashboard_state_path() -> io::Result<PathBuf> {
    Ok(dashboard_state_dir()?.join(DASHBOARD_STATE_FILE))
}

fn dashboard_log_path() -> io::Result<PathBuf> {
    Ok(dashboard_state_dir()?.join(DASHBOARD_LOG_FILE))
}

fn hook_trust_path() -> io::Result<PathBuf> {
    Ok(bindport_state_dir()?.join(HOOK_TRUST_FILE))
}

fn create_dashboard_state_dir() -> io::Result<PathBuf> {
    let path = dashboard_state_dir()?;
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn open_dashboard_log() -> io::Result<fs::File> {
    let path = create_dashboard_state_dir()?.join(DASHBOARD_LOG_FILE);
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

fn dashboard_state_dir() -> io::Result<PathBuf> {
    bindport_state_dir()
}

fn bindport_state_dir() -> io::Result<PathBuf> {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(state_home).join(SERVICE_NAME));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join(SERVICE_NAME));
    }

    if let Some(appdata) = env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(appdata).join(SERVICE_NAME));
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not determine state directory; set XDG_STATE_HOME, HOME, or APPDATA",
    ))
}

fn dashboard_process_is_running(state: &DashboardServiceState) -> bool {
    process_is_running(state.pid) && dashboard_process_matches_state(state)
}

#[cfg(target_os = "linux")]
fn dashboard_process_matches_state(state: &DashboardServiceState) -> bool {
    match state.process_start_time {
        Some(expected) => {
            process_start_time(state.pid) == Some(expected)
                && process_cmdline_is_dashboard(state.pid)
        }
        None => process_cmdline_is_dashboard(state.pid),
    }
}

#[cfg(not(target_os = "linux"))]
fn dashboard_process_matches_state(_state: &DashboardServiceState) -> bool {
    // Non-Linux targets do not have the /proc fields used above. Dashboard stop
    // falls back to PID liveness there, which can be fooled by PID reuse.
    true
}

#[cfg(target_os = "linux")]
fn process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(19)?.parse::<u64>().ok()
}

#[cfg(not(target_os = "linux"))]
fn process_start_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn process_cmdline_is_dashboard(pid: u32) -> bool {
    let Ok(cmdline) = fs::read(Path::new("/proc").join(pid.to_string()).join("cmdline")) else {
        return false;
    };
    let args = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .collect::<Vec<_>>();

    args.windows(2)
        .any(|window| window[0] == b"dashboard" && window[1] == b"serve")
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn process_is_running(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> io::Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn dashboard_clean_callback(cwd: PathBuf, config: ResolvedConfig) -> DashboardCleanCallback {
    Arc::new(move |registry, _summary| {
        let events = RouteEventCollector::single(
            RouteEventSource::DashboardClean,
            RouteEventKind::RoutesRemoved,
        );

        auto_render_outputs_for_events(&cwd, &config, registry, &events)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
}

fn dashboard_status_callback(cwd: PathBuf) -> DashboardStatusCallback {
    Arc::new(move || match resolve_config(&cwd) {
        Ok(config) => hooks_status_json(&cwd, &config),
        Err(error) => serde_json::json!({
            "error": error.to_string(),
            "items": [],
        }),
    })
}

#[cfg(not(unix))]
fn terminate_process(_pid: u32) -> io::Result<()> {
    Err(io::Error::other(
        "dashboard stop is not implemented on this platform",
    ))
}

#[derive(Debug)]
enum DashboardCommandError {
    Config(ConfigError),
    Dashboard(bindport_dashboard::DashboardError),
    InvalidArgument(String),
    Io(io::Error),
    MissingToken { source_name: String },
}

impl From<ConfigError> for DashboardCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<bindport_dashboard::DashboardError> for DashboardCommandError {
    fn from(error: bindport_dashboard::DashboardError) -> Self {
        Self::Dashboard(error)
    }
}

impl From<io::Error> for DashboardCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderCommand {
    Render,
    Help,
}

#[derive(Debug, Default)]
struct RenderCommandOptions {
    output: Option<String>,
    all: bool,
    dry_run: bool,
    repair: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderReport {
    Print,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Normal,
    Repair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteEventSource {
    CliRunner,
    CliClean,
    DashboardClean,
    ManualRender,
    StaleReconcile,
}

impl RouteEventSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CliRunner => "cli_runner",
            Self::CliClean => "cli_clean",
            Self::DashboardClean => "dashboard_clean",
            Self::ManualRender => "manual_render",
            Self::StaleReconcile => "stale_reconcile",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteEventKind {
    RouteStarted,
    RouteFinished,
    RoutesRemoved,
    RoutesMarkedStale,
    RenderRequested,
}

impl RouteEventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RouteStarted => "route_started",
            Self::RouteFinished => "route_finished",
            Self::RoutesRemoved => "routes_removed",
            Self::RoutesMarkedStale => "routes_marked_stale",
            Self::RenderRequested => "render_requested",
        }
    }
}

impl From<RouteEventKind> for HookEvent {
    fn from(kind: RouteEventKind) -> Self {
        match kind {
            RouteEventKind::RouteStarted => Self::RouteStarted,
            RouteEventKind::RouteFinished => Self::RouteFinished,
            RouteEventKind::RoutesRemoved => Self::RoutesRemoved,
            RouteEventKind::RoutesMarkedStale => Self::RoutesMarkedStale,
            RouteEventKind::RenderRequested => Self::RenderRequested,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RouteEvent {
    source: RouteEventSource,
    kind: RouteEventKind,
}

impl RouteEvent {
    const fn new(source: RouteEventSource, kind: RouteEventKind) -> Self {
        Self { source, kind }
    }
}

#[derive(Debug, Clone, Default)]
struct RouteEventCollector {
    events: Vec<RouteEvent>,
}

struct RenderInvocation<'a> {
    outputs: Vec<EffectiveOutputConfig>,
    dry_run: bool,
    mode: RenderMode,
    report: RenderReport,
    events: &'a RouteEventCollector,
}

impl RouteEventCollector {
    fn single(source: RouteEventSource, kind: RouteEventKind) -> Self {
        let mut collector = Self::default();
        collector.record(source, kind);
        collector
    }

    fn record(&mut self, source: RouteEventSource, kind: RouteEventKind) {
        self.events.push(RouteEvent::new(source, kind));
    }

    fn is_empty(&self) -> bool {
        self.events().is_empty()
    }

    fn warning_context(&self) -> String {
        match self.events.as_slice() {
            [event] => format!("{} {}", event.source.as_str(), event.kind.as_str()),
            [] => String::from("route event"),
            events => {
                let sources = events
                    .iter()
                    .map(|event| event.source.as_str())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(",");
                format!("route events from {sources}")
            }
        }
    }

    fn events(&self) -> &[RouteEvent] {
        &self.events
    }

    fn hook_events(&self, output_rendered: bool) -> BTreeSet<HookEvent> {
        let mut events = self
            .events
            .iter()
            .map(|event| HookEvent::from(event.kind))
            .collect::<BTreeSet<_>>();

        if output_rendered {
            events.insert(HookEvent::OutputRendered);
        }

        events
    }

    fn hook_sources(&self) -> String {
        self.events
            .iter()
            .map(|event| event.source.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Debug, Clone)]
struct EffectiveHook {
    name: String,
    events: Vec<HookEvent>,
    command: Vec<String>,
    timeout: Duration,
    timeout_ms: u64,
    source: String,
    definition: String,
    hook_hash: String,
    target: HookTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookTrustScope {
    Worktree,
    Repo,
}

impl HookTrustScope {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::Repo => "repo",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "worktree" => Some(Self::Worktree),
            "repo" => Some(Self::Repo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct HookPlan {
    hooks: Vec<EffectiveHook>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HookTarget {
    kind: HookTargetKind,
    display: String,
    fingerprint: String,
    hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookTargetKind {
    LocalFile,
    MissingFile,
    Opaque,
}

impl HookTargetKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::LocalFile => "local_file",
            Self::MissingFile => "missing_file",
            Self::Opaque => "opaque",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookDecision {
    Approved,
    Denied,
}

impl HookDecision {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "approved" => Some(Self::Approved),
            "denied" => Some(Self::Denied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookTrustStatus {
    Approved { scope: HookTrustScope },
    Denied { scope: HookTrustScope },
    Changed,
    Pending,
}

impl HookTrustStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Approved { .. } => "approved",
            Self::Denied { .. } => "denied",
            Self::Changed => "changed",
            Self::Pending => "pending",
        }
    }

    const fn is_approved(self) -> bool {
        matches!(self, Self::Approved { .. })
    }
}

#[derive(Debug, Clone)]
struct HookStatus {
    hook: EffectiveHook,
    trust: HookTrustStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookRunMode {
    Run,
    DryRun,
}

#[derive(Debug)]
enum HookExecutionError {
    Spawn { command: String, source: io::Error },
    Wait { command: String, source: io::Error },
    Timeout { command: String, timeout: Duration },
    Failed { command: String, status: ExitStatus },
}

impl std::fmt::Display for HookExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn { command, source } => {
                write!(f, "failed to spawn hook `{command}`: {source}")
            }
            Self::Wait { command, source } => {
                write!(f, "failed waiting for hook `{command}`: {source}")
            }
            Self::Timeout { command, timeout } => {
                write!(
                    f,
                    "hook `{command}` timed out after {}ms",
                    timeout.as_millis()
                )
            }
            Self::Failed { command, status } => {
                write!(f, "hook `{command}` exited with {status}")
            }
        }
    }
}

impl std::error::Error for HookExecutionError {}

fn configured_hook_plan(cwd: &Path, config: &ResolvedConfig) -> Option<HookPlan> {
    let loaded = config.loaded.as_ref()?;
    let hooks = loaded.config.hooks.as_ref()?;
    let commands = hooks.commands.as_deref().unwrap_or_default();
    let source = hook_command_source(config);
    let default_timeout = hooks.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);
    let hooks = commands
        .iter()
        .enumerate()
        .filter(|(_, hook)| hook.enabled.unwrap_or(true))
        .filter_map(|(index, hook)| effective_hook(cwd, index, hook, default_timeout, &source))
        .collect::<Vec<_>>();

    Some(HookPlan { hooks })
}

fn effective_hook(
    cwd: &Path,
    index: usize,
    hook: &HookCommandConfig,
    default_timeout_ms: u64,
    source: &str,
) -> Option<EffectiveHook> {
    let command = hook.command.clone()?;
    let events = hook.events.clone()?;
    let timeout_ms = hook.timeout_ms.unwrap_or(default_timeout_ms);
    let name = hook
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("hook-{}", index + 1));
    let target = hook_target(cwd, &command);
    let definition = hook_definition(&name, &events, &command, timeout_ms, source);
    let hook_hash = stable_hex_hash(definition.as_bytes());

    Some(EffectiveHook {
        name,
        events,
        command,
        timeout: Duration::from_millis(timeout_ms),
        timeout_ms,
        source: source.to_string(),
        definition,
        hook_hash,
        target,
    })
}

fn hook_command_source(config: &ResolvedConfig) -> String {
    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("unknown config");
    };

    if let Some(local) = loaded.local_override.as_ref()
        && local
            .config
            .hooks
            .as_ref()
            .and_then(|hooks| hooks.commands.as_ref())
            .is_some()
    {
        return format!("local override config `{}`", local.path.display());
    }

    format!(
        "{} config `{}`",
        loaded.source.as_str(),
        loaded.path.display()
    )
}

fn hook_definition(
    name: &str,
    events: &[HookEvent],
    command: &[String],
    timeout_ms: u64,
    _source: &str,
) -> String {
    let mut definition = String::from("schema=v1\n");
    append_fingerprinted_field(&mut definition, "name", name);
    definition.push_str(&format!("timeout_ms={timeout_ms}\n"));
    definition.push_str(&format!("events={}\n", events.len()));
    for event in events {
        append_fingerprinted_field(&mut definition, "event", event.as_str());
    }
    definition.push_str(&format!("command={}\n", command.len()));
    for value in command {
        append_fingerprinted_field(&mut definition, "argv", value);
    }

    definition
}

fn append_fingerprinted_field(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push(':');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push('\n');
}

fn hook_target(cwd: &Path, command: &[String]) -> HookTarget {
    let Some(program) = command.first().map(String::as_str) else {
        return opaque_hook_target("<empty>");
    };

    if !path_like_command(program) {
        return opaque_hook_target(program);
    }

    let path = PathBuf::from(program);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let display_path = path.display().to_string();

    match fs::read(&path) {
        Ok(contents) => {
            let resolved = path
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(&path));
            let fingerprint = format!(
                "file:{}:{}:{}",
                program,
                contents.len(),
                stable_hex_hash(&contents)
            );
            HookTarget {
                kind: HookTargetKind::LocalFile,
                display: resolved.display().to_string(),
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
        Err(_) => {
            let fingerprint = format!("missing:{program}");
            HookTarget {
                kind: HookTargetKind::MissingFile,
                display: display_path,
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
    }
}

fn opaque_hook_target(program: &str) -> HookTarget {
    let fingerprint = format!("opaque:{program}");
    HookTarget {
        kind: HookTargetKind::Opaque,
        display: program.to_string(),
        hash: stable_hex_hash(fingerprint.as_bytes()),
        fingerprint,
    }
}

fn path_like_command(program: &str) -> bool {
    program.contains('/') || program.contains('\\') || program.starts_with('.')
}

fn path_clean_display_path(path: &Path) -> PathBuf {
    path.components().collect()
}

fn stable_hex_hash(bytes: &[u8]) -> String {
    format!("{:016x}", stable_hash(bytes))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Clone, Default)]
struct HookTrustStore {
    entries: Vec<HookTrustEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HookTrustEntry {
    subject: String,
    scope: HookTrustScope,
    name: String,
    decision: HookDecision,
    definition: String,
    target: String,
    hook_hash: String,
    target_hash: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct HookTrustSubjects {
    worktree: String,
    repo: Option<String>,
}

impl HookTrustSubjects {
    fn subject(&self, scope: HookTrustScope) -> Option<&str> {
        match scope {
            HookTrustScope::Worktree => Some(&self.worktree),
            HookTrustScope::Repo => self.repo.as_deref(),
        }
    }
}

fn hook_trust_subjects(cwd: &Path) -> HookTrustSubjects {
    match detect_git_identity(cwd) {
        Some(git) => HookTrustSubjects {
            worktree: format!("worktree:{}", git.worktree_path.display()),
            repo: Some(format!("repo:{}", git.git_common_dir.display())),
        },
        None => {
            let path = cwd
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(cwd));
            HookTrustSubjects {
                worktree: format!("path:{}", path.display()),
                repo: None,
            }
        }
    }
}

fn read_hook_trust_store() -> io::Result<HookTrustStore> {
    let path = hook_trust_path()?;
    if !path.is_file() {
        return Ok(HookTrustStore::default());
    }

    let contents = fs::read_to_string(path)?;
    let value = serde_json::from_str::<serde_json::Value>(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let entries = value
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| entries.iter().filter_map(parse_hook_trust_entry).collect())
        .unwrap_or_default();

    Ok(HookTrustStore { entries })
}

fn parse_hook_trust_entry(value: &serde_json::Value) -> Option<HookTrustEntry> {
    Some(HookTrustEntry {
        subject: value.get("subject")?.as_str()?.to_string(),
        scope: HookTrustScope::parse(value.get("scope")?.as_str()?)?,
        name: value.get("name")?.as_str()?.to_string(),
        decision: HookDecision::parse(value.get("decision")?.as_str()?)?,
        definition: value.get("definition")?.as_str()?.to_string(),
        target: value.get("target")?.as_str()?.to_string(),
        hook_hash: value.get("hook_hash")?.as_str()?.to_string(),
        target_hash: value.get("target_hash")?.as_str()?.to_string(),
        updated_at: value.get("updated_at")?.as_str()?.to_string(),
    })
}

fn write_hook_trust_store(store: &HookTrustStore) -> io::Result<()> {
    let path = hook_trust_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let entries = store
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "subject": entry.subject,
                "scope": entry.scope.as_str(),
                "name": entry.name,
                "decision": entry.decision.as_str(),
                "definition": entry.definition,
                "target": entry.target,
                "hook_hash": entry.hook_hash,
                "target_hash": entry.target_hash,
                "updated_at": entry.updated_at,
            })
        })
        .collect::<Vec<_>>();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": HOOK_TRUST_SCHEMA_VERSION,
        "entries": entries,
    }))
    .map_err(io::Error::other)?;

    fs::write(path, format!("{json}\n"))
}

fn hook_trust_status(
    hook: &EffectiveHook,
    store: &HookTrustStore,
    subjects: &HookTrustSubjects,
) -> HookTrustStatus {
    for scope in [HookTrustScope::Worktree, HookTrustScope::Repo] {
        let Some(subject) = subjects.subject(scope) else {
            continue;
        };
        if let Some(entry) = store.entries.iter().find(|entry| {
            entry.scope == scope
                && entry.subject == subject
                && entry.name == hook.name
                && entry.definition == hook.definition
                && entry.target == hook.target.fingerprint
        }) {
            return match entry.decision {
                HookDecision::Approved => HookTrustStatus::Approved { scope },
                HookDecision::Denied => HookTrustStatus::Denied { scope },
            };
        }
    }

    for scope in [HookTrustScope::Worktree, HookTrustScope::Repo] {
        let Some(subject) = subjects.subject(scope) else {
            continue;
        };
        if store.entries.iter().any(|entry| {
            entry.scope == scope && entry.subject == subject && entry.name == hook.name
        }) {
            return HookTrustStatus::Changed;
        }
    }

    HookTrustStatus::Pending
}

fn hook_statuses_for_current_dir(cwd: &Path, config: &ResolvedConfig) -> Vec<HookStatus> {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        return Vec::new();
    };
    let store = read_hook_trust_store().unwrap_or_default();
    let subjects = hook_trust_subjects(cwd);

    plan.hooks
        .into_iter()
        .map(|hook| {
            let trust = hook_trust_status(&hook, &store, &subjects);
            HookStatus { hook, trust }
        })
        .collect()
}

fn upsert_hook_trust_entry(
    store: &mut HookTrustStore,
    subjects: &HookTrustSubjects,
    scope: HookTrustScope,
    hook: &EffectiveHook,
    decision: HookDecision,
) -> Result<(), String> {
    let Some(subject) = subjects.subject(scope) else {
        return Err(String::from(
            "repo scope is only available inside a git repository",
        ));
    };
    store.entries.retain(|entry| {
        !(entry.scope == scope && entry.subject == subject && entry.name == hook.name)
    });
    store.entries.push(HookTrustEntry {
        subject: subject.to_string(),
        scope,
        name: hook.name.clone(),
        decision,
        definition: hook.definition.clone(),
        target: hook.target.fingerprint.clone(),
        hook_hash: hook.hook_hash.clone(),
        target_hash: hook.target.hash.clone(),
        updated_at: unix_timestamp_string(),
    });

    Ok(())
}

fn reset_hook_trust_entries(
    store: &mut HookTrustStore,
    subjects: &HookTrustSubjects,
    scope: HookTrustScope,
    names: &BTreeSet<String>,
) -> usize {
    let Some(subject) = subjects.subject(scope) else {
        return 0;
    };
    let before = store.entries.len();
    store.entries.retain(|entry| {
        !(entry.scope == scope
            && entry.subject == subject
            && (names.is_empty() || names.contains(&entry.name)))
    });

    before - store.entries.len()
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| String::from("0"))
}

fn run_hooks_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    events: &RouteEventCollector,
    output_rendered: bool,
    mode: HookRunMode,
) -> usize {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        return 0;
    };
    let hook_events = events.hook_events(output_rendered);
    if hook_events.is_empty() {
        return 0;
    }
    let matching_hooks = plan
        .hooks
        .iter()
        .filter(|hook| hook_matches_events(hook, &hook_events))
        .collect::<Vec<_>>();

    if matching_hooks.is_empty() {
        return 0;
    }

    let store = match read_hook_trust_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("bindport: warning: hook trust store unavailable: {error}");
            return 0;
        }
    };
    let subjects = hook_trust_subjects(cwd);
    let env = HookEnvironment::new(events, &hook_events);
    let mut ran = 0;
    for hook in &matching_hooks {
        let trust = hook_trust_status(hook, &store, &subjects);
        if !trust.is_approved() {
            print_hook_not_trusted_warning(hook, trust);
            continue;
        }

        match mode {
            HookRunMode::DryRun => print_hook_dry_run(hook),
            HookRunMode::Run => {
                if let Err(error) = execute_hook(cwd, hook, &env) {
                    eprintln!("bindport: warning: {error}");
                }
            }
        }
        ran += 1;
    }

    ran
}

fn hook_matches_events(hook: &EffectiveHook, events: &BTreeSet<HookEvent>) -> bool {
    hook.events.iter().any(|event| events.contains(event))
}

fn print_hook_not_trusted_warning(hook: &EffectiveHook, trust: HookTrustStatus) {
    let reason = match trust {
        HookTrustStatus::Pending => "pending approval",
        HookTrustStatus::Changed => "changed since the last trust decision",
        HookTrustStatus::Denied { .. } => "denied",
        HookTrustStatus::Approved { .. } => return,
    };
    eprintln!(
        "bindport: warning: hook `{}` not run ({reason}); inspect with `bindport hooks status`",
        hook.name
    );
}

#[derive(Debug)]
struct HookEnvironment {
    events: String,
    sources: String,
    context: String,
}

impl HookEnvironment {
    fn new(route_events: &RouteEventCollector, hook_events: &BTreeSet<HookEvent>) -> Self {
        Self {
            events: hook_events
                .iter()
                .map(|event| event.as_str())
                .collect::<Vec<_>>()
                .join(","),
            sources: route_events.hook_sources(),
            context: route_events.warning_context(),
        }
    }
}

fn print_hook_dry_run(hook: &EffectiveHook) {
    println!(
        "would run hook {} ({}): {}",
        hook.name,
        hook.source,
        command_display(&hook.command)
    );
    println!(
        "  env: BINDPORT_HOOK_EVENTS=<redacted> BINDPORT_HOOK_SOURCES=<redacted> BINDPORT_HOOK_CONTEXT=<redacted>"
    );
}

fn execute_hook(
    cwd: &Path,
    hook: &EffectiveHook,
    env: &HookEnvironment,
) -> Result<(), HookExecutionError> {
    let Some((program, args)) = hook.command.split_first() else {
        return Err(HookExecutionError::Spawn {
            command: command_display(&hook.command),
            source: io::Error::new(io::ErrorKind::InvalidInput, "empty hook command"),
        });
    };
    let display = command_display(&hook.command);
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env_clear()
        .env("BINDPORT_HOOK_EVENTS", &env.events)
        .env("BINDPORT_HOOK_SOURCES", &env.sources)
        .env("BINDPORT_HOOK_CONTEXT", &env.context);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    configure_hook_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|source| HookExecutionError::Spawn {
            command: display.clone(),
            source,
        })?;
    let deadline = Instant::now() + hook.timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(HookExecutionError::Failed {
                    command: display,
                    status,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_hook_child(&mut child);
                let _ = child.wait();
                return Err(HookExecutionError::Timeout {
                    command: display,
                    timeout: hook.timeout,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(source) => {
                return Err(HookExecutionError::Wait {
                    command: display,
                    source,
                });
            }
        }
    }
}

#[cfg(unix)]
fn configure_hook_command(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_hook_command(_command: &mut Command) {}

#[cfg(unix)]
fn kill_hook_child(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    if pgid > 0 {
        let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_hook_child(child: &mut Child) {
    let _ = child.kill();
}

fn command_display(command: &[String]) -> String {
    if command.is_empty() {
        String::from("<empty>")
    } else {
        command.join(" ")
    }
}

fn run_render_command(args: &[String]) -> ExitCode {
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

fn run_render_command_result(args: &[String]) -> Result<(), RenderCommandError> {
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

fn auto_render_outputs_for_events(
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

fn collect_stale_reconcile_event(
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

fn has_blocking_auto_outputs(config: &ResolvedConfig) -> Result<bool, RenderCommandError> {
    Ok(configured_outputs(config)?
        .into_iter()
        .any(|output| output.auto_render && output.on_failure == OutputFailurePolicy::Block))
}

fn preflight_blocking_outputs(
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

fn validate_render_outputs(
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

fn parse_render_command(
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

fn configured_outputs(
    config: &ResolvedConfig,
) -> Result<Vec<EffectiveOutputConfig>, OutputConfigError> {
    config
        .loaded
        .as_ref()
        .map(|loaded| loaded.config.effective_outputs())
        .transpose()
        .map(|outputs| outputs.unwrap_or_default())
}

fn selected_outputs(
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

fn render_outputs_for_events(
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

fn render_outputs(
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

struct RenderWriteSummary {
    written: usize,
    external_modified: usize,
}

fn write_repair_render_plan(
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

fn record_written_output_files(
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

fn delete_route_keys(output: &EffectiveOutputConfig, routes: &[RouteRecord]) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| {
            route_delete_state(route).is_some_and(|state| output.delete_on.contains(&state))
        })
        .map(|route| route.key.clone())
        .collect()
}

fn route_delete_state(route: &RouteRecord) -> Option<OutputDeleteState> {
    match route.state.as_str() {
        "stopped" => Some(OutputDeleteState::Stopped),
        "stale" => Some(OutputDeleteState::Stale),
        _ => None,
    }
}

fn remove_output_files_for_lifecycle(
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

fn route_records(services: Vec<StatusService>) -> Vec<RouteRecord> {
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

fn pending_route_record(
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

fn output_base_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .to_path_buf()
}

#[derive(Debug)]
enum RenderCommandError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemplateCommand {
    List,
    Show,
    Export,
    Help,
}

#[derive(Debug, Default)]
struct TemplateCommandOptions {
    source: Option<TemplateSource>,
    name: Option<String>,
}

fn run_template_command(args: &[String]) -> ExitCode {
    match run_template_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(TemplateCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(TemplateCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport templates list|show|export [--source project|global|built-in] [name]"
            );
            ExitCode::FAILURE
        }
        Err(TemplateCommandError::Template(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_template_command_result(args: &[String]) -> Result<(), TemplateCommandError> {
    let (command, options) = parse_template_command(args)?;

    if command == TemplateCommand::Help {
        print_templates_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let resolver = template_resolver(&cwd)?;

    match command {
        TemplateCommand::List => print_template_list(&resolver, options.source)?,
        TemplateCommand::Show => {
            let name = options
                .name
                .as_deref()
                .expect("parser requires name for show");
            print_template_show(&resolver, name, options.source)?;
        }
        TemplateCommand::Export => {
            let name = options
                .name
                .as_deref()
                .expect("parser requires name for export");
            print_template_export(&resolver, name, options.source)?;
        }
        TemplateCommand::Help => unreachable!("handled before resolver setup"),
    }

    Ok(())
}

fn parse_template_command(
    args: &[String],
) -> Result<(TemplateCommand, TemplateCommandOptions), TemplateCommandError> {
    let (command, option_args) = match args.first().map(String::as_str) {
        None | Some("--help" | "-h") => (TemplateCommand::Help, &args[0..0]),
        Some("list") => (TemplateCommand::List, &args[1..]),
        Some("show") => (TemplateCommand::Show, &args[1..]),
        Some("export") => (TemplateCommand::Export, &args[1..]),
        Some(command) => {
            return Err(TemplateCommandError::InvalidArgument(format!(
                "unknown templates command `{command}`"
            )));
        }
    };
    let mut options = TemplateCommandOptions::default();
    let mut index = 0;

    while index < option_args.len() {
        match option_args[index].as_str() {
            "--source" => {
                index += 1;
                let value = option_args.get(index).ok_or_else(|| {
                    TemplateCommandError::InvalidArgument(String::from("--source requires a value"))
                })?;
                options.source = Some(parse_template_source(value)?);
            }
            "--help" | "-h" => {
                return Ok((TemplateCommand::Help, TemplateCommandOptions::default()));
            }
            option if option.starts_with("--") => {
                return Err(TemplateCommandError::InvalidArgument(format!(
                    "unknown templates option `{option}`"
                )));
            }
            name => {
                if options.name.is_some() {
                    return Err(TemplateCommandError::InvalidArgument(String::from(
                        "only one template name can be provided",
                    )));
                }
                options.name = Some(name.to_string());
            }
        }

        index += 1;
    }

    match command {
        TemplateCommand::List if options.name.is_some() => {
            Err(TemplateCommandError::InvalidArgument(String::from(
                "templates list does not take a template name",
            )))
        }
        TemplateCommand::Show | TemplateCommand::Export if options.name.is_none() => Err(
            TemplateCommandError::InvalidArgument(String::from("template name is required")),
        ),
        _ => Ok((command, options)),
    }
}

fn parse_template_source(value: &str) -> Result<TemplateSource, TemplateCommandError> {
    match value {
        "project" => Ok(TemplateSource::Project),
        "global" => Ok(TemplateSource::Global),
        "built-in" | "builtin" => Ok(TemplateSource::BuiltIn),
        _ => Err(TemplateCommandError::InvalidArgument(format!(
            "invalid template source `{value}`"
        ))),
    }
}

fn template_resolver(cwd: &Path) -> Result<TemplateResolver, ConfigError> {
    let config = resolve_config(cwd)?;

    Ok(TemplateResolver::new(
        Some(project_template_dir(cwd, &config)),
        global_template_dir(),
    ))
}

fn project_template_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .join(".bindport")
        .join("templates")
}

fn global_template_dir() -> Option<PathBuf> {
    fallback_config_path()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("templates")))
}

fn print_template_list(
    resolver: &TemplateResolver,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let templates = resolver.list(source)?;

    if templates.is_empty() {
        println!("No BindPort templates found.");
        return Ok(());
    }

    for template in templates {
        match template.path.as_ref() {
            Some(path) => println!("{}\t{}\t{}", template.name, template.source, path.display()),
            None => println!("{}\t{}", template.name, template.source),
        }
    }

    Ok(())
}

fn print_template_show(
    resolver: &TemplateResolver,
    name: &str,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let template = resolver.resolve(name, source)?;

    println!("template: {}", template.name);
    println!("source: {}", template.source);
    if let Some(path) = template.path.as_ref() {
        println!("path: {}", path.display());
    }
    println!();
    print!("{}", template.contents);
    if !template.contents.ends_with('\n') {
        println!();
    }

    Ok(())
}

fn print_template_export(
    resolver: &TemplateResolver,
    name: &str,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let template = resolver.resolve(name, source)?;
    print!("{}", template.contents);

    Ok(())
}

#[derive(Debug)]
enum TemplateCommandError {
    Config(ConfigError),
    InvalidArgument(String),
    Template(AdapterTemplateError),
}

impl From<ConfigError> for TemplateCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<AdapterTemplateError> for TemplateCommandError {
    fn from(error: AdapterTemplateError) -> Self {
        Self::Template(error)
    }
}

#[derive(Debug)]
struct CleanOptions {
    dry_run: bool,
    json: bool,
    stopped: bool,
    stale: bool,
    yes: bool,
    help: bool,
}

impl CleanOptions {
    fn states(&self) -> Vec<CleanState> {
        let mut states = Vec::new();

        if self.stopped {
            states.push(CleanState::Stopped);
        }
        if self.stale {
            states.push(CleanState::Stale);
        }

        states
    }
}

fn clean_registry(args: &[String]) -> ExitCode {
    match clean_registry_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CleanCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport clean [--dry-run] [--stopped] [--stale] [--json] [--yes]");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Json(error)) => {
            eprintln!("bindport: failed to serialize clean JSON: {error}");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Io(error)) => {
            eprintln!("bindport: failed to read cleanup confirmation: {error}");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::ConfirmationRequired(message)) => {
            eprintln!("bindport: {message}");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Aborted) => {
            eprintln!("bindport: cleanup aborted");
            ExitCode::FAILURE
        }
    }
}

fn clean_registry_result(args: &[String]) -> Result<(), CleanCommandError> {
    let options = parse_clean_options(args)?;

    if options.help {
        print_clean_help();
        return Ok(());
    }

    let states = options.states();
    let mut registry = Registry::open_default()?;
    let preview = registry.clean_leases(&states, true)?;
    confirm_stale_cleanup(&options, preview)?;
    let summary = if options.dry_run {
        preview
    } else {
        registry.clean_leases(&states, false)?
    };

    if !options.dry_run && summary.total_leases() > 0 {
        let events =
            RouteEventCollector::single(RouteEventSource::CliClean, RouteEventKind::RoutesRemoved);
        if let Err(error) = auto_render_outputs_for_current_dir(&mut registry, &events) {
            print_auto_render_warning(&events.warning_context(), &error);
        }
    }

    if options.json {
        print_clean_json(summary, options.dry_run)?;
    } else {
        print_clean_summary(summary, options.dry_run);
    }

    Ok(())
}

fn auto_render_outputs_for_current_dir(
    registry: &mut Registry,
    events: &RouteEventCollector,
) -> Result<usize, RenderCommandError> {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;

    auto_render_outputs_for_events(&cwd, &config, registry, events)
}

fn parse_clean_options(args: &[String]) -> Result<CleanOptions, CleanCommandError> {
    let mut options = CleanOptions {
        dry_run: false,
        json: false,
        stopped: false,
        stale: false,
        yes: false,
        help: false,
    };

    for arg in args {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--json" => options.json = true,
            "--stopped" => options.stopped = true,
            "--stale" => options.stale = true,
            "--yes" | "-y" => options.yes = true,
            "--all" => {
                options.stopped = true;
                options.stale = true;
            }
            "--help" | "-h" => options.help = true,
            unknown => {
                return Err(CleanCommandError::InvalidArgument(format!(
                    "unknown clean option `{unknown}`"
                )));
            }
        }
    }

    if !options.stopped && !options.stale {
        options.stopped = true;
        options.stale = true;
    }

    Ok(options)
}

fn confirm_stale_cleanup(
    options: &CleanOptions,
    preview: CleanSummary,
) -> Result<(), CleanCommandError> {
    if options.dry_run || options.yes || preview.stale_leases == 0 {
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err(CleanCommandError::ConfirmationRequired(String::from(
            "stale cleanup requires confirmation; rerun with --yes",
        )));
    }

    eprint!(
        "Remove {} stale registry entries? [y/N] ",
        preview.stale_leases
    );
    io::stderr().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let answer = answer.trim();

    if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(CleanCommandError::Aborted)
    }
}

fn print_clean_json(summary: CleanSummary, dry_run: bool) -> Result<(), CleanCommandError> {
    let report = serde_json::json!({
        "dry_run": dry_run,
        "leases": summary.total_leases(),
        "runs": summary.runs,
        "states": {
            "stopped": summary.stopped_leases,
            "stale": summary.stale_leases,
        },
    });
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");

    Ok(())
}

fn print_clean_summary(summary: CleanSummary, dry_run: bool) {
    let action = if dry_run { "would clean" } else { "cleaned" };

    println!(
        "{action} {} registry entries (stopped {}, stale {}, runs {})",
        summary.total_leases(),
        summary.stopped_leases,
        summary.stale_leases,
        summary.runs
    );
}

#[derive(Debug)]
enum CleanCommandError {
    InvalidArgument(String),
    Registry(RegistryError),
    Json(serde_json::Error),
    Io(io::Error),
    ConfirmationRequired(String),
    Aborted,
}

impl From<RegistryError> for CleanCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<serde_json::Error> for CleanCommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<io::Error> for CleanCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn run_hooks_command(args: &[String]) -> ExitCode {
    match run_hooks_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(HooksCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(HooksCommandError::Io(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(HooksCommandError::InvalidArgument(message)) => {
            eprintln!("bindport: {message}");
            eprintln!("usage: bindport hooks status|trust|deny|reset [options]");
            ExitCode::FAILURE
        }
    }
}

fn run_hooks_command_result(args: &[String]) -> Result<(), HooksCommandError> {
    let options = parse_hooks_command(args)?;
    if options.command == HooksCommand::Help {
        print_hooks_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let Some(plan) = configured_hook_plan(&cwd, &config) else {
        println!("No hooks configured.");
        return Ok(());
    };
    if plan.hooks.is_empty() {
        println!("No enabled hooks configured.");
        return Ok(());
    }

    match options.command {
        HooksCommand::Status => print_hooks_status(&cwd, &config),
        HooksCommand::Trust | HooksCommand::Deny | HooksCommand::Reset => {
            update_hook_trust(&cwd, plan.hooks, &options)
        }
        HooksCommand::Help => Ok(()),
    }
}

fn print_hooks_status(cwd: &Path, config: &ResolvedConfig) -> Result<(), HooksCommandError> {
    let statuses = hook_statuses_for_current_dir(cwd, config);
    if statuses.is_empty() {
        println!("No hooks configured.");
        return Ok(());
    }

    println!("BindPort hooks");
    for status in statuses {
        print_hook_status(&status);
    }

    Ok(())
}

fn print_hook_status(status: &HookStatus) {
    println!(
        "{}\t{}\t{}",
        status.trust.as_str(),
        status.hook.name,
        command_display(&status.hook.command)
    );
    println!("  trust: {}", hook_trust_status_display(status.trust));
    println!("  source: {}", status.hook.source);
    println!("  events: {}", hook_events_display(&status.hook.events));
    println!(
        "  target: {} ({})",
        status.hook.target.display,
        status.hook.target.kind.as_str()
    );
    println!("  hook hash: {}", status.hook.hook_hash);
    println!("  target hash: {}", status.hook.target.hash);
}

fn update_hook_trust(
    cwd: &Path,
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<(), HooksCommandError> {
    let selected = selected_hooks(hooks, options)?;
    let subjects = hook_trust_subjects(cwd);
    let mut store = read_hook_trust_store()?;
    let names = selected
        .iter()
        .map(|hook| hook.name.clone())
        .collect::<BTreeSet<_>>();

    match options.command {
        HooksCommand::Trust | HooksCommand::Deny => {
            let decision = if options.command == HooksCommand::Trust {
                HookDecision::Approved
            } else {
                HookDecision::Denied
            };
            for hook in &selected {
                upsert_hook_trust_entry(&mut store, &subjects, options.scope, hook, decision)
                    .map_err(HooksCommandError::InvalidArgument)?;
            }
            write_hook_trust_store(&store)?;
            println!(
                "{} {} hook(s) for {} scope",
                decision.as_str(),
                selected.len(),
                options.scope.as_str()
            );
        }
        HooksCommand::Reset => {
            let removed = reset_hook_trust_entries(&mut store, &subjects, options.scope, &names);
            write_hook_trust_store(&store)?;
            println!(
                "reset {removed} hook trust entr{} for {} scope",
                if removed == 1 { "y" } else { "ies" },
                options.scope.as_str()
            );
        }
        HooksCommand::Status | HooksCommand::Help => {}
    }

    Ok(())
}

fn selected_hooks(
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<Vec<EffectiveHook>, HooksCommandError> {
    if options.all {
        return Ok(hooks);
    }
    let Some(name) = options.name.as_deref() else {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hook name or --all is required",
        )));
    };
    let selected = hooks
        .into_iter()
        .filter(|hook| hook.name == name)
        .collect::<Vec<_>>();

    if selected.is_empty() {
        Err(HooksCommandError::InvalidArgument(format!(
            "hook `{name}` is not configured or is disabled"
        )))
    } else {
        Ok(selected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HooksCommand {
    Status,
    Trust,
    Deny,
    Reset,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HooksCommandOptions {
    command: HooksCommand,
    scope: HookTrustScope,
    all: bool,
    name: Option<String>,
}

fn parse_hooks_command(args: &[String]) -> Result<HooksCommandOptions, HooksCommandError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(HooksCommandOptions {
            command: HooksCommand::Status,
            scope: HookTrustScope::Worktree,
            all: false,
            name: None,
        });
    };
    let command = match command {
        "status" => HooksCommand::Status,
        "trust" => HooksCommand::Trust,
        "deny" => HooksCommand::Deny,
        "reset" => HooksCommand::Reset,
        "--help" | "-h" | "help" => HooksCommand::Help,
        unknown => {
            return Err(HooksCommandError::InvalidArgument(format!(
                "unknown hooks command `{unknown}`"
            )));
        }
    };

    let mut options = HooksCommandOptions {
        command,
        scope: HookTrustScope::Worktree,
        all: false,
        name: None,
    };
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--scope" => {
                index += 1;
                let Some(scope) = args.get(index).map(String::as_str) else {
                    return Err(HooksCommandError::InvalidArgument(String::from(
                        "--scope requires worktree or repo",
                    )));
                };
                options.scope = HookTrustScope::parse(scope).ok_or_else(|| {
                    HooksCommandError::InvalidArgument(format!(
                        "invalid hook trust scope `{scope}`"
                    ))
                })?;
            }
            "--all" => options.all = true,
            "--help" | "-h" => {
                options.command = HooksCommand::Help;
            }
            value if value.starts_with('-') => {
                return Err(HooksCommandError::InvalidArgument(format!(
                    "unknown hooks option `{value}`"
                )));
            }
            value => {
                if options.name.is_some() {
                    return Err(HooksCommandError::InvalidArgument(String::from(
                        "only one hook name can be provided",
                    )));
                }
                options.name = Some(value.to_string());
            }
        }
        index += 1;
    }

    if options.all && options.name.is_some() {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "use either --all or a hook name, not both",
        )));
    }
    if matches!(
        options.command,
        HooksCommand::Trust | HooksCommand::Deny | HooksCommand::Reset
    ) && !options.all
        && options.name.is_none()
    {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hook name or --all is required",
        )));
    }
    if options.command == HooksCommand::Status && (options.all || options.name.is_some()) {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hooks status does not take a hook selector",
        )));
    }

    Ok(options)
}

#[derive(Debug)]
enum HooksCommandError {
    Config(ConfigError),
    Io(io::Error),
    InvalidArgument(String),
}

impl From<ConfigError> for HooksCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<io::Error> for HooksCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn hooks_status_json_for_current_dir() -> serde_json::Value {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    match resolve_config(&cwd) {
        Ok(config) => hooks_status_json(&cwd, &config),
        Err(error) => serde_json::json!({
            "error": error.to_string(),
            "items": [],
        }),
    }
}

fn hooks_status_json(cwd: &Path, config: &ResolvedConfig) -> serde_json::Value {
    let items = hook_statuses_for_current_dir(cwd, config)
        .into_iter()
        .map(|status| {
            serde_json::json!({
                "name": status.hook.name,
                "status": status.trust.as_str(),
                "trust": hook_trust_status_display(status.trust),
                "source": status.hook.source,
                "events": status
                    .hook
                    .events
                    .iter()
                    .map(|event| event.as_str())
                    .collect::<Vec<_>>(),
                "command": status.hook.command,
                "command_display": command_display(&status.hook.command),
                "timeout_ms": status.hook.timeout_ms,
                "hook_hash": status.hook.hook_hash,
                "target": {
                    "kind": status.hook.target.kind.as_str(),
                    "display": status.hook.target.display,
                    "hash": status.hook.target.hash,
                },
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({ "items": items })
}

fn print_status_json() -> ExitCode {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => match serde_json::to_value(&snapshot).and_then(|mut value| {
            if let Some(object) = value.as_object_mut() {
                object.insert(String::from("hooks"), hooks_status_json_for_current_dir());
            }
            serde_json::to_string_pretty(&value)
        }) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("bindport: failed to serialize status JSON: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn print_status() -> ExitCode {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => {
            if snapshot.services.is_empty() {
                println!("No BindPort runs recorded yet.");
            } else {
                for service in snapshot.services {
                    let pid = service
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| String::from("-"));
                    println!(
                        "{}\t{}\t{}:{}\tpid {}\t{}",
                        service.state,
                        service.service,
                        service.host,
                        service.port,
                        pid,
                        service.command
                    );
                }
            }

            ExitCode::SUCCESS
        }
        Err(error) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OpenOptions {
    service: Option<String>,
    project: Option<String>,
    browser: bool,
    help: bool,
}

fn run_open_command(args: &[String]) -> ExitCode {
    match run_open_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(OpenCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport open [service] [--project PROJECT] [--browser] [--print]");
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

fn run_open_command_result(args: &[String]) -> Result<(), OpenCommandError> {
    let options = parse_open_options(args)?;

    if options.help {
        print_open_help();
        return Ok(());
    }

    let snapshot = Registry::open_default().and_then(|mut registry| registry.status_snapshot())?;
    let service = select_open_service(&snapshot.services, &options)?;
    let url = best_service_url(service);

    if options.browser {
        open_url_in_browser(&url)?;
    }

    println!("{url}");

    Ok(())
}

fn parse_open_options(args: &[String]) -> Result<OpenOptions, OpenCommandError> {
    let mut options = OpenOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--browser" => options.browser = true,
            "--print" => {}
            "--project" => {
                index += 1;
                options.project = Some(
                    args.get(index)
                        .ok_or_else(|| {
                            OpenCommandError::InvalidArgument(String::from(
                                "--project requires a value",
                            ))
                        })?
                        .to_string(),
                );
            }
            "--help" | "-h" => options.help = true,
            value if value.starts_with('-') => {
                return Err(OpenCommandError::InvalidArgument(format!(
                    "unknown open option `{value}`"
                )));
            }
            service => {
                if options.service.is_some() {
                    return Err(OpenCommandError::InvalidArgument(String::from(
                        "bindport open accepts at most one service name",
                    )));
                }
                options.service = Some(service.to_string());
            }
        }

        index += 1;
    }

    Ok(options)
}

fn select_open_service<'a>(
    services: &'a [StatusService],
    options: &OpenOptions,
) -> Result<&'a StatusService, OpenCommandError> {
    let matches = services
        .iter()
        .filter(|service| service.state == "active")
        .filter(|service| {
            options
                .service
                .as_ref()
                .is_none_or(|wanted| service.service == *wanted)
        })
        .filter(|service| {
            options
                .project
                .as_ref()
                .is_none_or(|wanted| service.project == *wanted)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [service] => Ok(service),
        [] => Err(OpenCommandError::Selection(open_not_found_message(options))),
        _ => Err(OpenCommandError::Selection(open_ambiguous_message(
            options, &matches,
        ))),
    }
}

fn open_not_found_message(options: &OpenOptions) -> String {
    match (&options.project, &options.service) {
        (Some(project), Some(service)) => {
            format!("no active BindPort service matched `{project}/{service}`")
        }
        (None, Some(service)) => format!("no active BindPort service matched `{service}`"),
        (Some(project), None) => format!("no active BindPort service matched project `{project}`"),
        (None, None) => String::from("no active BindPort services recorded"),
    }
}

fn open_ambiguous_message(options: &OpenOptions, services: &[&StatusService]) -> String {
    let matches = services
        .iter()
        .map(|service| format!("{}/{}", service.project, service.service))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");

    match &options.service {
        Some(service) => {
            format!(
                "multiple active services matched `{service}`; pass --project. matches: {matches}"
            )
        }
        None => {
            format!("multiple active services recorded; pass a service name. matches: {matches}")
        }
    }
}

fn best_service_url(service: &StatusService) -> String {
    service
        .route_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(&service.url)
        .to_string()
}

fn open_url_in_browser(url: &str) -> io::Result<()> {
    let url = validate_browser_url(url)?;

    #[cfg(not(any(unix, windows)))]
    {
        let _ = url;
        return Err(io::Error::other(
            "browser launch is not supported on this platform",
        ));
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.args(["--", url]);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.args(["--", url]);
        command
    };

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "browser launcher exited with {status}"
        )))
    }
}

fn validate_browser_url(url: &str) -> io::Result<&str> {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(invalid_browser_url());
    };

    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return Err(invalid_browser_url());
    }

    let Some(authority_and_path) = rest.strip_prefix("//") else {
        return Err(invalid_browser_url());
    };

    let authority = authority_and_path
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.is_empty() {
        return Err(invalid_browser_url());
    }

    Ok(url)
}

fn invalid_browser_url() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "browser launch only supports http:// and https:// URLs",
    )
}

#[derive(Debug)]
enum OpenCommandError {
    InvalidArgument(String),
    Registry(RegistryError),
    Browser(io::Error),
    Selection(String),
}

impl From<RegistryError> for OpenCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<io::Error> for OpenCommandError {
    fn from(error: io::Error) -> Self {
        Self::Browser(error)
    }
}

fn run_doctor_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        None => print_doctor(),
        Some("--help" | "-h") => {
            print_doctor_help();
            ExitCode::SUCCESS
        }
        Some("outputs") if args.len() == 1 => print_doctor_outputs(),
        Some("outputs") => {
            eprintln!("bindport: doctor outputs does not take arguments");
            eprintln!("usage: bindport doctor [outputs]");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("bindport: unknown doctor command `{command}`");
            eprintln!("usage: bindport doctor [outputs]");
            ExitCode::FAILURE
        }
    }
}

fn run_config_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("explain") if args.len() == 1 => print_config_explain(),
        Some("validate") if args.len() == 1 => print_config_validate(),
        None | Some("--help" | "-h") => {
            print_config_help();
            ExitCode::SUCCESS
        }
        Some("explain") => {
            eprintln!("bindport: config explain does not take arguments");
            eprintln!("usage: bindport config explain");
            ExitCode::FAILURE
        }
        Some("validate") => {
            eprintln!("bindport: config validate does not take arguments");
            eprintln!("usage: bindport config validate");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("bindport: unknown config command `{command}`");
            eprintln!("usage: bindport config explain|validate");
            ExitCode::FAILURE
        }
    }
}

fn print_config_validate() -> ExitCode {
    println!("BindPort config validate");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    println!("cwd: {}", cwd.display());

    let config = match resolve_config(&cwd) {
        Ok(config) => config,
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };

    print_config_source_explanation(&config);

    let issues = config
        .loaded
        .as_ref()
        .map(|loaded| loaded.config.validate())
        .unwrap_or_default();

    if issues.is_empty() {
        println!("validation: ok");
        ExitCode::SUCCESS
    } else {
        println!(
            "validation: {} {}",
            issues.len(),
            plural(issues.len(), "error")
        );
        for issue in issues {
            println!("  error: {issue}");
        }
        ExitCode::FAILURE
    }
}

fn print_config_explain() -> ExitCode {
    println!("BindPort config explain");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    println!("cwd: {}", cwd.display());

    let config = match resolve_config(&cwd) {
        Ok(config) => config,
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };

    print_config_source_explanation(&config);
    print_config_field_explanations(&config);

    let explained = explain_run_identity(&cwd, &[], &RunOptions::default(), &config);
    println!("identity:");
    println!(
        "  project: {} ({})",
        explained.identity.project, explained.project_source
    );
    println!(
        "  service: {} ({})",
        explained.identity.service, explained.service_source
    );
    println!("  key: {}", explained.identity.identity_key);

    ExitCode::SUCCESS
}

fn print_config_source_explanation(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            if let Some(local) = loaded.local_override.as_ref() {
                println!(
                    "config local override: {} ({} {})",
                    local.path.display(),
                    loaded.source.as_str(),
                    local.format.as_str()
                );
            }
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }

    if let Some(loaded) = config.loaded.as_ref()
        && !loaded.unknown_keys.is_empty()
    {
        println!(
            "config warning: ignored unknown top-level keys: {}",
            loaded.unknown_keys.join(", ")
        );
        println!("config applied keys: {}", APPLIED_CONFIG_KEYS.join(", "));
    }
}

fn print_config_field_explanations(config: &ResolvedConfig) {
    println!("fields:");

    match config.loaded.as_ref() {
        Some(loaded) => {
            print_config_field(
                "project",
                optional_config_value(loaded.config.project.as_deref()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.project.as_deref()),
                    loaded.config.project.as_deref(),
                ),
            );
            print_config_field(
                "service",
                optional_config_value(loaded.config.service.as_deref()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.service.as_deref()),
                    loaded.config.service.as_deref(),
                ),
            );
            print_config_field(
                "default_range",
                format!("{}-{}", config.port_range.start, config.port_range.end),
                defaulted_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.default_range.as_deref()),
                    loaded.config.default_range.as_deref(),
                ),
            );
            print_config_field(
                "skip_ports",
                format!("{} ports", config.skip_ports.len()),
                defaulted_vec_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.skip_ports.as_ref()),
                    loaded.config.skip_ports.as_ref(),
                ),
            );
            print_config_field(
                "services",
                list_config_value(loaded.config.services.as_ref().map(Vec::len), "entry"),
                optional_vec_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.services.as_ref()),
                    loaded.config.services.as_ref(),
                ),
            );
            print_config_field(
                "dashboard",
                configured_value(loaded.config.dashboard.is_some()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.dashboard.as_ref()),
                    loaded.config.dashboard.as_ref(),
                ),
            );
            print_config_field(
                "output_defaults",
                configured_value(loaded.config.output_defaults.is_some()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.output_defaults.as_ref()),
                    loaded.config.output_defaults.as_ref(),
                ),
            );
            print_config_field(
                "outputs",
                list_config_value(loaded.config.outputs.as_ref().map(Vec::len), "entry"),
                optional_vec_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.outputs.as_ref()),
                    loaded.config.outputs.as_ref(),
                ),
            );
            print_config_field(
                "hooks",
                list_config_value(
                    loaded
                        .config
                        .hooks
                        .as_ref()
                        .and_then(|hooks| hooks.commands.as_ref())
                        .map(Vec::len),
                    "entry",
                ),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.hooks.as_ref()),
                    loaded.config.hooks.as_ref(),
                ),
            );
        }
        None => {
            print_config_field("project", "<unset>", "not configured");
            print_config_field("service", "<unset>", "not configured");
            print_config_field(
                "default_range",
                format!("{}-{}", config.port_range.start, config.port_range.end),
                "built-in default",
            );
            print_config_field(
                "skip_ports",
                format!("{} ports", config.skip_ports.len()),
                "built-in default",
            );
            print_config_field("services", "<unset>", "not configured");
            print_config_field("dashboard", "<unset>", "not configured");
            print_config_field("output_defaults", "<unset>", "not configured");
            print_config_field("outputs", "<unset>", "not configured");
            print_config_field("hooks", "<unset>", "not configured");
        }
    }
}

fn print_config_field(name: &str, value: impl AsRef<str>, source: impl AsRef<str>) {
    println!("  {name}: {} ({})", value.as_ref(), source.as_ref());
}

fn optional_config_value(value: Option<&str>) -> String {
    non_empty_value(value)
        .map(str::to_string)
        .unwrap_or_else(|| String::from("<unset>"))
}

fn configured_value(configured: bool) -> &'static str {
    if configured { "configured" } else { "<unset>" }
}

fn list_config_value(count: Option<usize>, unit: &str) -> String {
    match count {
        Some(1) => format!("1 {unit}"),
        Some(count) if unit == "entry" => format!("{count} entries"),
        Some(count) => format!("{count} {unit}s"),
        None => String::from("<unset>"),
    }
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

fn local_config(loaded: &LoadedConfig) -> Option<&BindPortConfig> {
    loaded.local_override.as_ref().map(|local| &local.config)
}

fn optional_field_source<T: ?Sized>(
    loaded: &LoadedConfig,
    local_value: Option<&T>,
    effective_value: Option<&T>,
) -> String {
    if local_value.is_some() {
        String::from("local override config")
    } else if effective_value.is_some() {
        source_config_label(loaded.source).to_string()
    } else {
        String::from("not configured")
    }
}

fn optional_vec_field_source<T>(
    loaded: &LoadedConfig,
    local_value: Option<&Vec<T>>,
    effective_value: Option<&Vec<T>>,
) -> String {
    optional_field_source(loaded, local_value, effective_value)
}

fn defaulted_field_source<T: ?Sized>(
    loaded: &LoadedConfig,
    local_value: Option<&T>,
    effective_value: Option<&T>,
) -> String {
    if local_value.is_some() {
        String::from("local override config")
    } else if effective_value.is_some() {
        source_config_label(loaded.source).to_string()
    } else {
        String::from("built-in default")
    }
}

fn defaulted_vec_field_source<T>(
    loaded: &LoadedConfig,
    local_value: Option<&Vec<T>>,
    effective_value: Option<&Vec<T>>,
) -> String {
    defaulted_field_source(loaded, local_value, effective_value)
}

#[derive(Debug)]
struct IdentityExplanation {
    identity: ServiceIdentity,
    project_source: String,
    service_source: String,
}

fn explain_run_identity(
    cwd: &Path,
    command: &[String],
    options: &RunOptions,
    config: &ResolvedConfig,
) -> IdentityExplanation {
    let identity = resolve_run_identity(cwd, command, options, config);
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let env_service = env::var(BINDPORT_SERVICE_ENV).ok();

    IdentityExplanation {
        project_source: identity_project_source(config, env_project.as_deref()),
        service_source: identity_service_source(cwd, config, options, env_service.as_deref()),
        identity,
    }
}

fn identity_project_source(config: &ResolvedConfig, env_project: Option<&str>) -> String {
    if non_empty_value(env_project).is_some() {
        return format!("environment {BINDPORT_PROJECT_ENV}");
    }

    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("inference");
    };

    if non_empty_value(local_config(loaded).and_then(|local| local.project.as_deref())).is_some() {
        String::from("local override config `project`")
    } else if non_empty_value(loaded.config.project.as_deref()).is_some() {
        format!("{} `project`", source_config_label(loaded.source))
    } else {
        String::from("inference")
    }
}

fn identity_service_source(
    cwd: &Path,
    config: &ResolvedConfig,
    options: &RunOptions,
    env_service: Option<&str>,
) -> String {
    if non_empty_value(options.service.as_deref()).is_some() {
        return String::from("CLI service argument");
    }

    if non_empty_value(env_service).is_some() {
        return format!("environment {BINDPORT_SERVICE_ENV}");
    }

    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("inference");
    };

    if let Some((_, source)) = config_service_source_for_cwd(loaded, cwd) {
        source
    } else {
        String::from("inference")
    }
}

fn config_service_source_for_cwd(loaded: &LoadedConfig, cwd: &Path) -> Option<(String, String)> {
    let service = loaded.configured_service_for_cwd(cwd)?;
    let name = non_empty_value(Some(service.name))?;

    Some((
        name.to_string(),
        configured_service_source_label(loaded, service.source),
    ))
}

fn configured_service_source_label(
    loaded: &LoadedConfig,
    source: ConfiguredServiceSource,
) -> String {
    match source {
        ConfiguredServiceSource::ServiceField => {
            if non_empty_value(local_config(loaded).and_then(|local| local.service.as_deref()))
                .is_some()
            {
                String::from("local override config `service`")
            } else {
                format!("{} `service`", source_config_label(loaded.source))
            }
        }
        ConfiguredServiceSource::PathMatch => {
            format!("{} `[[services]].path`", services_config_label(loaded))
        }
        ConfiguredServiceSource::SingleService => {
            format!("{} single `[[services]]`", services_config_label(loaded))
        }
    }
}

fn services_config_label(loaded: &LoadedConfig) -> &'static str {
    if local_config(loaded)
        .and_then(|local| local.services.as_ref())
        .is_some()
    {
        "local override config"
    } else {
        source_config_label(loaded.source)
    }
}

fn source_config_label(source: ConfigSource) -> &'static str {
    match source {
        ConfigSource::Project => "project config",
        ConfigSource::Fallback => "fallback config",
    }
}

fn non_empty_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn print_doctor() -> ExitCode {
    println!("BindPort bootstrap doctor");

    let mut registry = print_doctor_registry_path();

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = match resolve_config(&cwd) {
        Ok(config) => {
            print_config_diagnostics(&config);
            config
        }
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    let identity = resolve_run_identity(&cwd, &[], &RunOptions::default(), &config);

    print_identity_diagnostics(&identity);
    print_git_diagnostics(&cwd);
    let allocation_ok = print_allocation_diagnostics(&config, &identity, registry.as_mut());

    println!("first proxy adapter: {}", AdapterKind::Traefik.as_str());
    if allocation_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_doctor_outputs() -> ExitCode {
    println!("BindPort output doctor");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = match resolve_config(&cwd) {
        Ok(config) => {
            print_doctor_output_config(&config);
            config
        }
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    let outputs = match configured_outputs(&config) {
        Ok(outputs) => outputs,
        Err(error) => {
            println!("outputs: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    print_doctor_hooks(&cwd, &config);

    if outputs.is_empty() {
        println!("outputs: none configured");
        return ExitCode::SUCCESS;
    }

    let mut registry = match Registry::open_default() {
        Ok(registry) => registry,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let snapshot = match registry.status_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let routes = route_records(snapshot.services);
    let base_dir = output_base_dir(&cwd, &config);
    let resolver = TemplateResolver::new(
        Some(project_template_dir(&cwd, &config)),
        global_template_dir(),
    );
    let mut ok = true;

    println!("routes: {}", routes.len());
    println!("base dir: {}", base_dir.display());

    for output in &outputs {
        if !print_doctor_output(output, &resolver, &routes, &base_dir) {
            ok = false;
        }
    }

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_doctor_hooks(cwd: &Path, config: &ResolvedConfig) {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        println!("hooks: none configured");
        return;
    };

    if plan.hooks.is_empty() {
        println!("hooks: none enabled");
        return;
    }

    let store = read_hook_trust_store().unwrap_or_default();
    let subjects = hook_trust_subjects(cwd);

    println!("hooks: {} configured", plan.hooks.len());
    for hook in plan.hooks {
        let trust = hook_trust_status(&hook, &store, &subjects);
        println!("  hook {}:", hook.name);
        println!("    trust: {}", hook_trust_status_display(trust));
        println!("    source: {}", hook.source);
        println!("    events: {}", hook_events_display(&hook.events));
        println!("    command: {}", command_display(&hook.command));
        println!("    timeout: {}ms", hook.timeout.as_millis());
        println!(
            "    target: {} ({})",
            hook.target.display,
            hook.target.kind.as_str()
        );
        println!("    hook hash: {}", hook.hook_hash);
        println!("    target hash: {}", hook.target.hash);
        println!(
            "    env: BINDPORT_HOOK_EVENTS=<redacted> BINDPORT_HOOK_SOURCES=<redacted> BINDPORT_HOOK_CONTEXT=<redacted>"
        );
    }
}

fn hook_trust_status_display(status: HookTrustStatus) -> String {
    match status {
        HookTrustStatus::Approved { scope } => format!("approved ({})", scope.as_str()),
        HookTrustStatus::Denied { scope } => format!("denied ({})", scope.as_str()),
        HookTrustStatus::Changed => String::from("changed"),
        HookTrustStatus::Pending => String::from("pending"),
    }
}

fn hook_events_display(events: &[HookEvent]) -> String {
    events
        .iter()
        .map(|event| event.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_doctor_output_config(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            if let Some(local) = loaded.local_override.as_ref() {
                println!(
                    "config local override: {} ({} {})",
                    local.path.display(),
                    loaded.source.as_str(),
                    local.format.as_str()
                );
            }
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }
}

fn print_doctor_output(
    output: &EffectiveOutputConfig,
    resolver: &TemplateResolver,
    routes: &[RouteRecord],
    base_dir: &Path,
) -> bool {
    println!("output {}:", output.name);
    println!("  target: {}", output.target);
    println!(
        "  root: {}",
        output.root.as_deref().unwrap_or("<derived from target>")
    );
    println!("  auto-render: {}", output.auto_render);

    let template = match resolver.resolve(&output.template, None) {
        Ok(template) => {
            println!("  template: {} ({})", output.template, template.source);
            if let Some(path) = template.path.as_ref() {
                println!("  template path: {}", path.display());
            }
            if template.wildcard_matches.len() > 1 {
                println!(
                    "  template warning: multiple wildcard matches; using {}",
                    template
                        .wildcard_matches
                        .first()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| String::from("<unknown>"))
                );
            }
            template
        }
        Err(error) => {
            println!("  template: {} (invalid: {error})", output.template);
            return false;
        }
    };

    let render_config = OutputRenderConfig::from(output);
    let plan = match render_output_routes(&render_config, &template.contents, routes) {
        Ok(plan) => plan,
        Err(error) => {
            println!("  plan: invalid ({error})");
            return false;
        }
    };
    let planned_files = match render_plan_paths(&plan, base_dir) {
        Ok(planned_files) => planned_files,
        Err(error) => {
            println!("  paths: invalid ({error})");
            return false;
        }
    };

    println!("  planned files: {}", planned_files.len());
    for file in planned_files.iter().take(5) {
        println!("    {} -> {}", file.route_key, file.path.display());
    }
    if planned_files.len() > 5 {
        println!("    ... {} more", planned_files.len() - 5);
    }

    true
}

fn print_doctor_registry_path() -> Option<Registry> {
    match default_registry_path() {
        Ok(path) => match Registry::open(&path) {
            Ok(registry) => {
                println!("registry: {} (ok)", path.display());
                Some(registry)
            }
            Err(error) => {
                println!("registry: {} (unavailable: {error})", path.display());
                None
            }
        },
        Err(error) => {
            println!("registry: unavailable ({error})");
            None
        }
    }
}

fn print_identity_diagnostics(identity: &ServiceIdentity) {
    println!(
        "effective identity: project={} service={}",
        identity.project, identity.service
    );
    println!("identity key: {}", identity.identity_key);
}

fn print_git_diagnostics(cwd: &Path) {
    match detect_git_identity(cwd) {
        Some(git) => {
            println!("git worktree: {}", git.worktree_path.display());
            println!("git branch: {}", git.branch);
            println!("git branch label: {}", git.branch_label);
            println!("git commit: {}", git.commit);
        }
        None => println!("git worktree: none"),
    }
}

fn print_config_diagnostics(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            if let Some(local) = loaded.local_override.as_ref() {
                println!(
                    "config local override: {} ({} {})",
                    local.path.display(),
                    loaded.source.as_str(),
                    local.format.as_str()
                );
            }
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }

    if let Some(loaded) = config.loaded.as_ref()
        && !loaded.unknown_keys.is_empty()
    {
        println!(
            "config warning: ignored unknown top-level keys: {}",
            loaded.unknown_keys.join(", ")
        );
        println!("config applied keys: {}", APPLIED_CONFIG_KEYS.join(", "));
    }

    println!(
        "effective port range: {}-{}",
        config.port_range.start, config.port_range.end
    );
    println!("skip ports: {}", config.skip_ports.len());
}

fn print_allocation_diagnostics(
    config: &ResolvedConfig,
    identity: &ServiceIdentity,
    registry: Option<&mut Registry>,
) -> bool {
    let mut active_ports = Vec::new();
    let mut previous_port = None;
    let registry_available = registry.is_some();
    let mut active_ports_available = registry_available;
    let mut previous_port_available = registry_available;

    match registry {
        Some(registry) => {
            match registry.active_ports() {
                Ok(ports) => active_ports = ports,
                Err(error) => {
                    println!("registry active ports in range: unavailable ({error})");
                    active_ports_available = false;
                }
            }

            match registry.previous_identity_port(&identity.identity_key) {
                Ok(port) => previous_port = port,
                Err(error) => {
                    println!("previous identity port: unavailable ({error})");
                    previous_port_available = false;
                }
            }
        }
        None => {
            println!("registry active ports in range: unavailable");
            active_ports_available = false;
            previous_port_available = false;
        }
    }

    if active_ports_available {
        let active_in_range = ports_in_range(&active_ports, config.port_range);
        println!(
            "registry active ports in range: {}",
            format_limited_ports(&active_in_range)
        );
    }

    if previous_port_available {
        print_previous_port_diagnostics(previous_port, config, &active_ports);
    }
    let listener_conflicts = listener_conflicts(config.port_range, &active_ports);
    println!(
        "known registry listener conflicts in range: {}",
        format_limited_ports(&listener_conflicts.known_registry)
    );
    println!(
        "unknown os listener conflicts in range: {}",
        format_listener_conflict_scan(&listener_conflicts)
    );

    let scan_start = identity.port_scan_start(config.port_range);
    match scan_start {
        Some(port) => println!("allocation scan start: {port}"),
        None => println!("allocation scan start: unavailable"),
    }

    let mut skip_ports = config.skip_ports.clone();
    skip_ports.extend(active_ports);
    let allocation_hints = AllocationHints {
        preferred_port: previous_port,
        scan_start,
    };

    match allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints) {
        Ok(port) => {
            let source = if Some(port) == previous_port {
                "sticky"
            } else {
                "scan"
            };
            println!("next candidate port: {port} ({source})");
            true
        }
        Err(error) => {
            println!("next candidate port: unavailable ({error})");
            false
        }
    }
}

fn print_previous_port_diagnostics(
    previous_port: Option<u16>,
    config: &ResolvedConfig,
    active_ports: &[u16],
) {
    let Some(port) = previous_port else {
        println!("previous identity port: none");
        return;
    };

    let status = if !config.port_range.contains(port) {
        "outside range"
    } else if config.skip_ports.contains(&port) {
        "configured skip"
    } else if active_ports.contains(&port) {
        "active registry conflict"
    } else if is_port_available(port) {
        "free"
    } else {
        "os listener conflict"
    };

    println!("previous identity port: {port} ({status})");
}

fn ports_in_range(ports: &[u16], range: PortRange) -> Vec<u16> {
    let mut ports = ports
        .iter()
        .copied()
        .filter(|port| range.contains(*port))
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

struct ListenerConflictScan {
    known_registry: Vec<u16>,
    unknown: Vec<u16>,
    scanned_ports: u32,
    total_ports: u32,
}

fn listener_conflicts(range: PortRange, known_registry_ports: &[u16]) -> ListenerConflictScan {
    let total_ports = range.len();
    let scanned_ports = total_ports.min(DOCTOR_MAX_LISTENER_PROBES);
    let known_registry_ports = ports_in_range(known_registry_ports, range);
    let mut known_registry = Vec::new();
    let mut unknown = Vec::new();

    for offset in 0..scanned_ports {
        let port = range.start as u32 + offset;
        let port = u16::try_from(port).expect("port remains within configured range");

        if is_port_available(port) {
            continue;
        }

        if known_registry_ports.contains(&port) {
            known_registry.push(port);
        } else {
            unknown.push(port);
        }
    }

    ListenerConflictScan {
        known_registry,
        unknown,
        scanned_ports,
        total_ports,
    }
}

fn format_listener_conflict_scan(scan: &ListenerConflictScan) -> String {
    let mut summary = format_limited_ports(&scan.unknown);

    if scan.scanned_ports < scan.total_ports {
        summary.push_str(&format!(
            " (scanned first {} of {} ports)",
            scan.scanned_ports, scan.total_ports
        ));
    }

    summary
}

fn format_limited_ports(ports: &[u16]) -> String {
    if ports.is_empty() {
        return String::from("none");
    }

    let mut summary = ports
        .iter()
        .take(DOCTOR_PORT_DISPLAY_LIMIT)
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    if ports.len() > DOCTOR_PORT_DISPLAY_LIMIT {
        summary.push_str(&format!(
            " (+{} more)",
            ports.len() - DOCTOR_PORT_DISPLAY_LIMIT
        ));
    }

    summary
}

fn init_fallback_config() -> ExitCode {
    match write_fallback_config() {
        Ok(InitConfigResult::Created(path)) => {
            println!("created config: {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(InitConfigResult::AlreadyExists(path)) => {
            println!("config already exists: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bindport: failed to initialize fallback config: {error}");
            ExitCode::FAILURE
        }
    }
}

enum InitConfigResult {
    Created(PathBuf),
    AlreadyExists(PathBuf),
}

fn write_fallback_config() -> io::Result<InitConfigResult> {
    let path = fallback_config_path()?;

    if path.is_file() {
        return Ok(InitConfigResult::AlreadyExists(path));
    }

    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("`{}` exists but is not a file", path.display()),
        ));
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, default_fallback_config())?;

    Ok(InitConfigResult::Created(path))
}

fn status_to_exit_code(status: &ExitStatus) -> ExitCode {
    match status_registry_exit_code(status) {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

fn status_registry_exit_code(status: &ExitStatus) -> Option<i32> {
    status.code().or_else(|| signal_exit_code(status))
}

fn should_retry_allocation(status: &ExitStatus, elapsed: Duration, port: u16) -> bool {
    matches!(status.code(), Some(code) if code != 0)
        && elapsed <= ALLOCATION_RETRY_WINDOW
        && !is_port_available(port)
}

#[cfg(unix)]
fn signal_exit_code(status: &ExitStatus) -> Option<i32> {
    status.signal().map(|signal| 128 + signal)
}

#[cfg(not(unix))]
fn signal_exit_code(_status: &ExitStatus) -> Option<i32> {
    None
}

fn print_runner_error(error: &RunnerError) {
    eprintln!("bindport: {error}");
}

fn print_config_error(error: &ConfigError) {
    eprintln!("bindport: {error}");
}

fn print_registry_error(error: &RegistryError) {
    eprintln!("bindport: {error}");
    eprintln!("bindport: set {REGISTRY_PATH_ENV} to override the registry path");
}

fn print_registry_warning(context: &str, error: &RegistryError) {
    eprintln!("bindport: warning: {context}: {error}");
}

fn print_auto_render_warning(context: &str, error: &RenderCommandError) {
    eprintln!("bindport: warning: output auto-render failed after {context}: {error}");
}

fn registry_disabled_warning() {
    eprintln!(
        "bindport: warning: running without registry recording; set {REGISTRY_PATH_ENV} to restore it"
    );
}

fn print_help() {
    println!("BindPort - proxy-neutral local development port registry");
    println!();
    println!("Usage:");
    println!("  bindport -- <command>        Run a command with an assigned PORT");
    println!("  bindport run [service] [options] [-- <command>]");
    println!("                                  Run a command or configured service command");
    println!("  bindport status [--json]     Show registry status");
    println!("  bindport open [service]      Print or open the best service URL");
    println!("  bindport clean [--dry-run]   Remove stopped and stale registry entries");
    println!("  bindport config explain      Explain resolved config and identity sources");
    println!("  bindport config validate     Validate config structure");
    println!("  bindport hooks status        Inspect configured hook trust");
    println!("  bindport doctor              Show bootstrap diagnostics");
    println!("  bindport doctor outputs      Validate output rendering setup");
    println!("  bindport dashboard [serve]   Serve the local dashboard");
    println!("  bindport dashboard start     Start the dashboard in the background");
    println!("  bindport dashboard status    Show background dashboard status");
    println!("  bindport dashboard stop      Stop the background dashboard");
    println!("  bindport render [output]     Render configured output files");
    println!("  bindport templates list      List resolved output templates");
    println!("  bindport templates show      Show a resolved output template");
    println!("  bindport templates export    Export a resolved output template");
    println!("  bindport init                Create optional fallback config");
    println!("  bindport --version           Print version");
    println!();
    println!("Run options:");
    println!("  --env NAME=VALUE             Add a templated child environment variable");
    println!("  --hostname <template>        Set route hostname metadata");
    println!("  --route-url <template>       Set route URL metadata");
    println!("  --health-url <template>      Set service health check URL metadata");
}

fn print_open_help() {
    println!("BindPort service URL lookup");
    println!();
    println!("Usage:");
    println!("  bindport open [service] [--project PROJECT] [--browser] [--print]");
    println!();
    println!("Options:");
    println!("  --project <project>    Disambiguate services with the same name");
    println!("  --browser              Open the URL with the system browser and print it");
    println!("  --print                Print the URL without launching a browser (default)");
}

fn print_config_help() {
    println!("BindPort config");
    println!();
    println!("Usage:");
    println!("  bindport config explain");
    println!("  bindport config validate");
    println!();
    println!("Commands:");
    println!("  explain    Show resolved config fields and identity sources");
    println!("  validate   Validate config structure and output actionable errors");
}

fn print_doctor_help() {
    println!("BindPort diagnostics");
    println!();
    println!("Usage:");
    println!("  bindport doctor");
    println!("  bindport doctor outputs");
    println!();
    println!("Commands:");
    println!("  outputs    Validate output config, templates, and planned file paths");
}

fn print_render_help() {
    println!("BindPort output rendering");
    println!();
    println!("Usage:");
    println!("  bindport render [output] [options]");
    println!();
    println!("Options:");
    println!("  --all        Render every enabled output (default)");
    println!("  --dry-run    Render templates and print targets without writing files");
    println!("  --repair     Re-render current routes and reconcile DB-owned files");
}

fn print_templates_help() {
    println!("BindPort output templates");
    println!();
    println!("Usage:");
    println!("  bindport templates list [--source project|global|built-in]");
    println!("  bindport templates show [--source project|global|built-in] <name>");
    println!("  bindport templates export [--source project|global|built-in] <name>");
    println!();
    println!("Options:");
    println!("  --source <source>    Resolve only project, global, or built-in templates");
}

fn print_clean_help() {
    println!("BindPort registry cleanup");
    println!();
    println!("Usage:");
    println!("  bindport clean [options]");
    println!();
    println!("Options:");
    println!("  --dry-run     Show what would be removed without deleting entries");
    println!("  --stopped     Remove stopped entries only");
    println!("  --stale       Remove stale entries only");
    println!("  --all         Remove stopped and stale entries (default)");
    println!("  --json        Print machine-readable cleanup counts");
    println!("  --yes, -y     Confirm stale entry deletion without prompting");
}

fn print_hooks_help() {
    println!("BindPort hooks");
    println!();
    println!("Usage:");
    println!("  bindport hooks status");
    println!("  bindport hooks trust [--scope worktree|repo] <hook|--all>");
    println!("  bindport hooks deny [--scope worktree|repo] <hook|--all>");
    println!("  bindport hooks reset [--scope worktree|repo] <hook|--all>");
    println!();
    println!("Options:");
    println!("  --scope <scope>    Trust scope, either worktree (default) or repo");
    println!("  --all              Select every configured hook");
}

fn print_dashboard_help() {
    println!("BindPort dashboard");
    println!();
    println!("Usage:");
    println!("  bindport dashboard [serve] [options]");
    println!("  bindport dashboard start [options]");
    println!("  bindport dashboard status");
    println!("  bindport dashboard stop");
    println!();
    println!("Options:");
    println!("  --host <ip>              Bind IP address (default 127.0.0.1)");
    println!("  --port <port>            Preferred dashboard port (default 27080)");
    println!("  --auth <mode>            required or disabled");
    println!("  --auth-required          Require bearer token access to dashboard data");
    println!("  --no-auth                Disable dashboard bearer token checks");
    println!("  --register-service       Record the dashboard in BindPort status");
    println!("  --no-register-service    Do not record the dashboard in BindPort status");
    println!("  --token <token>          Bearer token value (visible in process lists)");
    println!("  --token-env <name>       Environment variable containing the token");
    println!("  --allowed-host <host>    Additional accepted HTTP Host header");
    println!("  --static-dir <path>      Read dashboard assets from a local directory");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bindport_core::HooksConfig;
    use std::collections::BTreeMap;

    #[test]
    fn empty_args_print_help_successfully() {
        assert_eq!(run([]), ExitCode::SUCCESS);
        assert_eq!(run([String::from("--help")]), ExitCode::SUCCESS);
    }

    #[test]
    fn version_arg_succeeds() {
        assert_eq!(run([String::from("--version")]), ExitCode::SUCCESS);
    }

    #[test]
    fn subcommand_help_surfaces_succeed() {
        assert_eq!(run(strings(["config", "--help"])), ExitCode::SUCCESS);
        assert_eq!(run(strings(["doctor", "--help"])), ExitCode::SUCCESS);
        assert_eq!(run(strings(["render", "--help"])), ExitCode::SUCCESS);
        assert_eq!(run(strings(["templates", "--help"])), ExitCode::SUCCESS);
        assert_eq!(run(strings(["clean", "--help"])), ExitCode::SUCCESS);
        assert_eq!(run(strings(["dashboard", "--help"])), ExitCode::SUCCESS);
    }

    #[test]
    fn invalid_command_surfaces_fail_without_panicking() {
        for args in [
            strings(["unknown"]),
            strings(["run", "--bad"]),
            strings(["config", "unknown"]),
            strings(["config", "explain", "extra"]),
            strings(["doctor", "unknown"]),
            strings(["doctor", "outputs", "extra"]),
            strings(["render", "--bad"]),
            strings(["templates", "unknown"]),
            strings(["templates", "show"]),
            strings(["clean", "--bad"]),
            strings(["dashboard", "--bad"]),
            strings(["dashboard", "serve", "--host", "0.0.0.0"]),
            strings([
                "dashboard",
                "serve",
                "--auth-required",
                "--token-env",
                "BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST",
            ]),
        ] {
            assert_eq!(run(args), ExitCode::FAILURE);
        }
    }

    #[test]
    fn empty_runner_command_fails() {
        assert_eq!(run([String::from("--")]), ExitCode::FAILURE);
    }

    #[test]
    fn route_event_collector_retains_source_and_kind() {
        let empty = RouteEventCollector::default();
        assert!(empty.is_empty());
        assert_eq!(empty.warning_context(), "route event");

        let mut collector =
            RouteEventCollector::single(RouteEventSource::CliRunner, RouteEventKind::RouteStarted);
        collector.record(
            RouteEventSource::StaleReconcile,
            RouteEventKind::RoutesMarkedStale,
        );

        assert_eq!(
            collector.events(),
            &[
                RouteEvent::new(RouteEventSource::CliRunner, RouteEventKind::RouteStarted),
                RouteEvent::new(
                    RouteEventSource::StaleReconcile,
                    RouteEventKind::RoutesMarkedStale,
                )
            ]
        );
        assert_eq!(
            collector.warning_context(),
            "route events from cli_runner,stale_reconcile"
        );
        assert_eq!(collector.hook_sources(), "cli_runner,stale_reconcile");
        assert_eq!(
            collector.hook_events(false),
            BTreeSet::from([HookEvent::RouteStarted, HookEvent::RoutesMarkedStale,])
        );
        assert_eq!(
            collector.hook_events(true),
            BTreeSet::from([
                HookEvent::RouteStarted,
                HookEvent::RoutesMarkedStale,
                HookEvent::OutputRendered,
            ])
        );

        let single = RouteEventCollector::single(
            RouteEventSource::DashboardClean,
            RouteEventKind::RoutesRemoved,
        );
        assert_eq!(single.warning_context(), "dashboard_clean routes_removed");
        assert_eq!(RouteEventSource::CliClean.as_str(), "cli_clean");
        assert_eq!(RouteEventSource::ManualRender.as_str(), "manual_render");
        assert_eq!(RouteEventKind::RouteFinished.as_str(), "route_finished");
        assert_eq!(RouteEventKind::RenderRequested.as_str(), "render_requested");
    }

    #[test]
    fn hook_plan_reports_source_without_granting_trust() {
        let project_hooks = HooksConfig {
            commands: Some(vec![hook_command("reload")]),
            ..HooksConfig::default()
        };
        let cwd = Path::new("/workspace/demo");
        let project_plan = configured_hook_plan(
            cwd,
            &hook_resolved_config(ConfigSource::Project, project_hooks.clone(), None),
        )
        .expect("project hook plan");
        assert_eq!(
            project_plan.hooks[0].source,
            "project config `/workspace/demo/bindport.toml`"
        );

        let local_command = hook_command("local-reload");
        let local_commands = configured_hook_plan(
            cwd,
            &hook_resolved_config(
                ConfigSource::Project,
                HooksConfig {
                    commands: Some(vec![hook_command("project-reload")]),
                    ..HooksConfig::default()
                },
                Some(HooksConfig {
                    commands: Some(vec![local_command]),
                    ..HooksConfig::default()
                }),
            ),
        )
        .expect("local hook plan");
        assert_eq!(local_commands.hooks[0].name, "local-reload");
        assert_eq!(
            local_commands.hooks[0].source,
            "local override config `/workspace/demo/.bindport.local.toml`"
        );

        let fallback = configured_hook_plan(
            cwd,
            &hook_resolved_config(
                ConfigSource::Fallback,
                HooksConfig {
                    commands: Some(vec![hook_command("fallback-reload")]),
                    ..HooksConfig::default()
                },
                None,
            ),
        )
        .expect("fallback hook plan");
        assert_eq!(
            fallback.hooks[0].source,
            "fallback config `/home/user/.config/bindport/config.toml`"
        );
    }

    #[test]
    fn hook_trust_status_requires_exact_user_scoped_match() {
        let cwd = Path::new("/workspace/demo");
        let plan = configured_hook_plan(
            cwd,
            &hook_resolved_config(
                ConfigSource::Project,
                HooksConfig {
                    commands: Some(vec![hook_command("reload")]),
                    ..HooksConfig::default()
                },
                None,
            ),
        )
        .expect("hook plan");
        let hook = &plan.hooks[0];
        let subjects = hook_trust_subjects(cwd);
        let mut store = HookTrustStore::default();

        assert_eq!(
            hook_trust_status(hook, &store, &subjects),
            HookTrustStatus::Pending
        );

        upsert_hook_trust_entry(
            &mut store,
            &subjects,
            HookTrustScope::Worktree,
            hook,
            HookDecision::Approved,
        )
        .expect("approve hook");
        assert_eq!(
            hook_trust_status(hook, &store, &subjects),
            HookTrustStatus::Approved {
                scope: HookTrustScope::Worktree
            }
        );

        let mut changed_hook = hook.clone();
        changed_hook.definition.push_str("changed\n");
        assert_eq!(
            hook_trust_status(&changed_hook, &store, &subjects),
            HookTrustStatus::Changed
        );

        upsert_hook_trust_entry(
            &mut store,
            &subjects,
            HookTrustScope::Worktree,
            hook,
            HookDecision::Denied,
        )
        .expect("deny hook");
        assert_eq!(
            hook_trust_status(hook, &store, &subjects),
            HookTrustStatus::Denied {
                scope: HookTrustScope::Worktree
            }
        );
    }

    #[test]
    fn run_option_parser_accepts_valid_options_and_rejects_bad_env_names() {
        let args = strings([
            "web",
            "--env",
            "NEXT_PUBLIC_URL={route_url}",
            "--hostname",
            "{branch}.{project}.localhost",
            "--route-url",
            "https://{hostname}",
            "--health-url",
            "{route_url}/health",
            "--",
            "next",
            "dev",
        ]);
        let (options, command) = parse_run_options(&args).expect("run options");

        assert_eq!(options.service.as_deref(), Some("web"));
        assert_eq!(
            options.hostname.as_deref(),
            Some("{branch}.{project}.localhost")
        );
        assert_eq!(options.route_url.as_deref(), Some("https://{hostname}"));
        assert_eq!(options.health_url.as_deref(), Some("{route_url}/health"));
        assert_eq!(
            options.env,
            vec![(String::from("NEXT_PUBLIC_URL"), String::from("{route_url}"))]
        );
        assert_eq!(command, strings(["next", "dev"]).as_slice());
        let service_only = strings(["web"]);
        let (options, command) = parse_run_options(&service_only).expect("service-only options");
        assert_eq!(options.service.as_deref(), Some("web"));
        assert!(command.is_empty());

        assert_eq!(
            parse_env_assignment("PORT").expect_err("missing assignment"),
            "invalid env assignment `PORT`; expected NAME=VALUE"
        );
        assert_eq!(
            parse_env_assignment("1PORT=3000").expect_err("bad name"),
            "invalid env variable name `1PORT`"
        );
        assert!(valid_env_name("_PORT"));
        assert!(valid_env_name("NEXT_PUBLIC_URL"));
        assert!(!valid_env_name(""));
        assert!(!valid_env_name("PORT-NAME"));
    }

    #[test]
    fn run_metadata_expands_route_and_env_templates() {
        let identity = ServiceIdentity {
            project: String::from("hoststamp"),
            service: String::from("web"),
            git: None,
            identity_key: String::from("v1:test"),
        };
        let templates = RunTemplates {
            command: Some(vec![
                String::from("storybook"),
                String::from("--port"),
                String::from("{port}"),
            ]),
            hostname: Some(String::from("{service}.{project}.localhost")),
            route_url: Some(String::from("https://{hostname}")),
            health_url: Some(String::from("{route_url}/health")),
            env: vec![
                (String::from("URL"), String::from("{route_url}")),
                (String::from("HEALTH"), String::from("{health_url}")),
                (String::from("JSON"), String::from(r#"{{"port":{port}}}"#)),
            ],
        };

        let metadata = resolve_run_metadata(&identity, 29100, &templates).expect("metadata");

        assert_eq!(
            metadata.hostname.as_deref(),
            Some("web.hoststamp.localhost")
        );
        assert_eq!(
            metadata.route_url.as_deref(),
            Some("https://web.hoststamp.localhost")
        );
        assert_eq!(
            metadata.health_url.as_deref(),
            Some("https://web.hoststamp.localhost/health")
        );
        assert_eq!(
            metadata.env,
            vec![
                (
                    String::from("URL"),
                    String::from("https://web.hoststamp.localhost")
                ),
                (
                    String::from("HEALTH"),
                    String::from("https://web.hoststamp.localhost/health")
                ),
                (String::from("JSON"), String::from(r#"{"port":29100}"#)),
            ]
        );
        assert_eq!(
            metadata.command,
            Some(vec![
                String::from("storybook"),
                String::from("--port"),
                String::from("29100"),
            ])
        );
    }

    #[test]
    fn template_expansion_reports_syntax_errors() {
        let identity = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("web"),
            git: None,
            identity_key: String::from("v1:test"),
        };
        let values = TemplateValues::new(&identity, 29100, None, None, None);

        assert!(matches!(
            expand_template("{project", &values),
            Err(TemplateError::Unclosed { .. })
        ));
        assert_eq!(
            expand_template("{project", &values)
                .expect_err("unclosed")
                .to_string(),
            "unclosed template placeholder in `{project`"
        );
        assert!(matches!(
            expand_template("project}", &values),
            Err(TemplateError::Unopened { .. })
        ));
        assert_eq!(
            expand_template("project}", &values)
                .expect_err("unopened")
                .to_string(),
            "unmatched `}` in template `project}`"
        );
        assert!(matches!(
            expand_template("{missing}", &values),
            Err(TemplateError::UnknownPlaceholder { .. })
        ));
        assert_eq!(
            expand_template("{missing}", &values)
                .expect_err("unknown placeholder")
                .to_string(),
            "unknown or unavailable template placeholder `missing` in `{missing}`"
        );
    }

    #[test]
    fn template_values_include_git_and_fallback_context() {
        let identity = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("web"),
            git: Some(bindport_core::GitIdentity {
                worktree_path: PathBuf::from("/workspace/demo-feature-tree"),
                worktree_hash: String::from("abc123456789"),
                git_common_dir: PathBuf::from("/workspace/demo/.git"),
                branch: String::from("feature/tree"),
                branch_label: String::from("feature-tree"),
                commit: String::from("0123456789abcdef"),
            }),
            identity_key: String::from("v1:test"),
        };
        let values = TemplateValues::new(
            &identity,
            29_100,
            Some("feature-tree.demo.localhost"),
            Some("https://feature-tree.demo.localhost"),
            Some("https://feature-tree.demo.localhost/health"),
        );

        assert_eq!(values.value("port").as_deref(), Some("29100"));
        assert_eq!(values.value("host").as_deref(), Some("127.0.0.1"));
        assert_eq!(
            values.value("url").as_deref(),
            Some("http://127.0.0.1:29100")
        );
        assert_eq!(values.value("project").as_deref(), Some("demo"));
        assert_eq!(values.value("service").as_deref(), Some("web"));
        assert_eq!(
            values.value("hostname").as_deref(),
            Some("feature-tree.demo.localhost")
        );
        assert_eq!(
            values.value("route_url").as_deref(),
            Some("https://feature-tree.demo.localhost")
        );
        assert_eq!(
            values.value("health_url").as_deref(),
            Some("https://feature-tree.demo.localhost/health")
        );
        assert_eq!(values.value("branch").as_deref(), Some("feature-tree"));
        assert_eq!(
            values.value("branch_label").as_deref(),
            Some("feature-tree")
        );
        assert_eq!(values.value("git_branch").as_deref(), Some("feature/tree"));
        assert_eq!(
            values.value("worktree").as_deref(),
            Some("demo-feature-tree")
        );
        assert_eq!(
            values.value("worktree_label").as_deref(),
            Some("demo-feature-tree")
        );
        assert_eq!(
            values.value("worktree_hash").as_deref(),
            Some("abc123456789")
        );
        assert_eq!(values.value("missing"), None);

        let no_git = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("api"),
            git: None,
            identity_key: String::from("v1:no-git"),
        };
        let values = TemplateValues::new(&no_git, 29_101, None, None, None);
        assert_eq!(
            values.value("route_url").as_deref(),
            Some("http://127.0.0.1:29101")
        );
        assert_eq!(values.value("branch").as_deref(), Some("no-branch"));
        assert_eq!(values.value("git_branch").as_deref(), Some("no-branch"));
        assert_eq!(values.value("worktree").as_deref(), Some("demo"));
        assert_eq!(values.value("worktree_hash").as_deref(), Some("no-git"));
    }

    #[test]
    fn dashboard_command_parser_preserves_serve_args_and_modes() {
        let args = strings([
            "start",
            "--host",
            "0.0.0.0",
            "--port",
            "27081",
            "--auth",
            "required",
            "--register-service",
            "--token",
            "secret",
            "--token-env",
            "CUSTOM_TOKEN",
            "--allowed-host",
            "devbox.test",
            "--static-dir",
            "static",
        ]);
        let (command, options) = parse_dashboard_command(&args).expect("dashboard command");

        assert_eq!(command, DashboardCommand::Start);
        assert_eq!(options.host, Some(Ipv4Addr::UNSPECIFIED));
        assert_eq!(options.port, Some(27081));
        assert_eq!(options.auth_required, Some(true));
        assert_eq!(options.register_service, Some(true));
        assert_eq!(options.token.as_deref(), Some("secret"));
        assert_eq!(options.token_env.as_deref(), Some("CUSTOM_TOKEN"));
        assert_eq!(options.allowed_hosts, vec![String::from("devbox.test")]);
        assert_eq!(options.static_dir, Some(PathBuf::from("static")));
        assert_eq!(
            options.serve_args,
            strings([
                "--host",
                "0.0.0.0",
                "--port",
                "27081",
                "--auth",
                "required",
                "--register-service",
                "--token-env",
                "CUSTOM_TOKEN",
                "--allowed-host",
                "devbox.test",
                "--static-dir",
                "static",
            ])
        );

        let (command, options) =
            parse_dashboard_command(&strings(["--help"])).expect("dashboard help");
        assert_eq!(command, DashboardCommand::Help);
        assert_eq!(options.token_env_name(), DASHBOARD_TOKEN_ENV);
        assert!(parse_dashboard_command(&strings(["--port", "bad"])).is_err());
        assert!(parse_dashboard_command(&strings(["--host", "bad"])).is_err());
        assert!(parse_dashboard_command(&strings(["--auth", "maybe"])).is_err());
        assert!(parse_dashboard_command(&strings(["--missing"])).is_err());
        assert!(parse_dashboard_command(&strings(["--token"])).is_err());

        let (command, _) = parse_dashboard_command(&[]).expect("default serve");
        assert_eq!(command, DashboardCommand::Serve);
        let (command, _) = parse_dashboard_command(&strings(["serve"])).expect("serve");
        assert_eq!(command, DashboardCommand::Serve);
        let (command, _) = parse_dashboard_command(&strings(["status"])).expect("status");
        assert_eq!(command, DashboardCommand::Status);
        let (command, _) = parse_dashboard_command(&strings(["stop"])).expect("stop");
        assert_eq!(command, DashboardCommand::Stop);

        let (_, options) =
            parse_dashboard_command(&strings(["--auth-required", "--no-register-service"]))
                .expect("boolean dashboard flags");
        assert_eq!(options.auth_required, Some(true));
        assert_eq!(options.register_service, Some(false));
        assert_eq!(
            options.serve_args,
            strings(["--auth-required", "--no-register-service"])
        );

        assert!(parse_dashboard_auth_mode("enabled").expect("enabled"));
        assert!(!parse_dashboard_auth_mode("disabled").expect("disabled"));
        assert!(parse_dashboard_bool("yes", "setting").expect("yes"));
        assert!(!parse_dashboard_bool("no", "setting").expect("no"));
    }

    #[test]
    fn dashboard_option_resolution_enforces_auth_and_precedence() {
        let config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: PathBuf::from("/workspace/demo/bindport.toml"),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Project,
                local_override: None,
                config: BindPortConfig {
                    dashboard: Some(bindport_core::DashboardConfig {
                        host: Some(String::from("127.0.0.2")),
                        port: Some(27_081),
                        register_service: Some(true),
                        allowed_hosts: Some(vec![
                            String::from("config.test"),
                            String::from("localhost"),
                        ]),
                        auth: Some(bindport_core::DashboardAuthConfig {
                            required: Some(true),
                            token: Some(String::from("config-token")),
                            token_env: Some(String::from("CONFIG_DASHBOARD_TOKEN")),
                        }),
                    }),
                    ..BindPortConfig::default()
                },
                unknown_keys: Vec::new(),
            }),
            fallback_path: None,
            port_range: PortRange {
                start: 29_100,
                end: 29_110,
            },
            skip_ports: vec![29_101],
        };
        let cli = DashboardCliOptions {
            host: Some(Ipv4Addr::LOCALHOST),
            port: Some(27_082),
            auth_required: Some(true),
            register_service: Some(false),
            token: Some(String::from("cli-token")),
            allowed_hosts: vec![String::from("cli.test")],
            static_dir: Some(PathBuf::from("dashboard/static")),
            ..DashboardCliOptions::default()
        };

        let options =
            resolve_dashboard_options(&config, &cli, vec![29_102]).expect("dashboard options");

        assert_eq!(options.host, Ipv4Addr::LOCALHOST);
        assert_eq!(options.preferred_port, 27_082);
        assert_eq!(options.fallback_range.start, 29_100);
        assert_eq!(options.skip_ports, vec![29_102]);
        assert!(options.allowed_hosts.contains(&String::from("127.0.0.1")));
        assert!(options.allowed_hosts.contains(&String::from("cli.test")));
        assert!(options.allowed_hosts.contains(&String::from("config.test")));
        assert!(options.auth.required);
        assert_eq!(options.auth.token.as_deref(), Some("cli-token"));
        assert_eq!(options.static_dir, Some(PathBuf::from("dashboard/static")));
        assert!(!resolve_dashboard_registration(&config, &cli).expect("registration"));

        let non_loopback_without_auth = DashboardCliOptions {
            host: Some(Ipv4Addr::UNSPECIFIED),
            auth_required: Some(false),
            ..DashboardCliOptions::default()
        };
        assert!(matches!(
            resolve_dashboard_options(&config, &non_loopback_without_auth, Vec::new()),
            Err(DashboardCommandError::InvalidArgument(message))
                if message.contains("requires auth")
        ));

        let missing_token = DashboardCliOptions {
            auth_required: Some(true),
            token_env: Some(String::from("BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST")),
            ..DashboardCliOptions::default()
        };
        let config_without_token = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };
        assert!(matches!(
            resolve_dashboard_options(&config_without_token, &missing_token, Vec::new()),
            Err(DashboardCommandError::MissingToken { source_name })
                if source_name == "BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST"
        ));
    }

    #[test]
    fn render_command_parser_and_output_selection_validate_combinations() {
        let (command, options) =
            parse_render_command(&strings(["traefik", "--dry-run"])).expect("render command");
        assert_eq!(command, RenderCommand::Render);
        assert_eq!(options.output.as_deref(), Some("traefik"));
        assert!(options.dry_run);

        let (command, _) = parse_render_command(&strings(["--help"])).expect("render help");
        assert_eq!(command, RenderCommand::Help);
        assert!(parse_render_command(&strings(["--all", "traefik"])).is_err());
        assert!(parse_render_command(&strings(["--dry-run", "--repair"])).is_err());
        assert!(parse_render_command(&strings(["traefik", "debug"])).is_err());

        let outputs = vec![EffectiveOutputConfig {
            name: String::from("traefik"),
            template: String::from("bindport-traefik"),
            root: None,
            target: String::from("{{ route.slug }}.yml"),
            target_host: String::from("127.0.0.1"),
            target_scheme: String::from("http"),
            auto_render: true,
            delete_on: Vec::new(),
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 0,
            vars: BTreeMap::new(),
        }];
        let selected = selected_outputs(outputs.clone(), Some("traefik")).expect("selected");
        assert_eq!(selected.len(), 1);
        assert!(selected_outputs(outputs, Some("missing")).is_err());
    }

    #[test]
    fn template_command_parser_validates_sources_and_names() {
        let (command, options) = parse_template_command(&strings([
            "show",
            "--source",
            "built-in",
            "bindport-traefik",
        ]))
        .expect("template command");
        assert_eq!(command, TemplateCommand::Show);
        assert_eq!(options.source, Some(TemplateSource::BuiltIn));
        assert_eq!(options.name.as_deref(), Some("bindport-traefik"));

        let (command, _) = parse_template_command(&strings(["-h"])).expect("template help");
        assert_eq!(command, TemplateCommand::Help);
        assert_eq!(
            parse_template_source("builtin").expect("builtin alias"),
            TemplateSource::BuiltIn
        );
        assert!(parse_template_command(&strings(["list", "extra"])).is_err());
        assert!(parse_template_command(&strings(["show"])).is_err());
        assert!(parse_template_command(&strings(["show", "a", "b"])).is_err());
        assert!(parse_template_command(&strings(["show", "--source"])).is_err());
        assert!(parse_template_command(&strings(["show", "--source", "bad", "name"])).is_err());
        assert!(parse_template_command(&strings(["bad"])).is_err());
        assert!(parse_template_command(&strings(["show", "--bad", "name"])).is_err());
    }

    #[test]
    fn clean_option_parser_defaults_and_validates_states() {
        let options = parse_clean_options(&[]).expect("default clean options");
        assert_eq!(
            options.states(),
            vec![CleanState::Stopped, CleanState::Stale]
        );
        assert!(!options.dry_run);
        assert!(!options.json);

        let options =
            parse_clean_options(&strings(["--dry-run", "--json", "--stopped"])).expect("clean");
        assert_eq!(options.states(), vec![CleanState::Stopped]);
        assert!(options.dry_run);
        assert!(options.json);
        assert!(!options.yes);

        let options = parse_clean_options(&strings(["--stale", "--yes"])).expect("stale clean");
        assert_eq!(options.states(), vec![CleanState::Stale]);
        assert!(options.yes);

        let options = parse_clean_options(&strings(["--help"])).expect("help clean");
        assert!(options.help);
        assert_eq!(
            options.states(),
            vec![CleanState::Stopped, CleanState::Stale]
        );
        assert!(parse_clean_options(&strings(["--bad"])).is_err());
    }

    #[test]
    fn open_option_parser_and_selection_handle_agent_url_lookup() {
        let options = parse_open_options(&strings(["web", "--project", "demo", "--print"]))
            .expect("open options");
        assert_eq!(options.service.as_deref(), Some("web"));
        assert_eq!(options.project.as_deref(), Some("demo"));
        assert!(!options.browser);

        let options = parse_open_options(&strings(["api", "--browser"])).expect("browser open");
        assert_eq!(options.service.as_deref(), Some("api"));
        assert!(options.browser);

        assert!(parse_open_options(&strings(["web", "api"])).is_err());
        assert!(parse_open_options(&strings(["--project"])).is_err());

        let web = status_service("open-web", "active", None);
        let mut api = status_service("open-api", "active", None);
        api.service = String::from("api");
        api.route_url = None;

        assert_eq!(
            best_service_url(&web),
            "https://feature-tree.demo.localhost"
        );
        assert_eq!(best_service_url(&api), "http://127.0.0.1:29100");
        assert_eq!(
            validate_browser_url(" https://feature-tree.demo.localhost/path ").expect("https"),
            "https://feature-tree.demo.localhost/path"
        );
        assert_eq!(
            validate_browser_url("HTTP://127.0.0.1:29100").expect("http"),
            "HTTP://127.0.0.1:29100"
        );
        assert!(validate_browser_url("file:///tmp/bindport").is_err());
        assert!(validate_browser_url("-psn_0_123").is_err());
        assert!(validate_browser_url("http:example.com").is_err());
        assert!(validate_browser_url("https:///missing-host").is_err());

        let services = vec![web, api];
        let selected = select_open_service(
            &services,
            &OpenOptions {
                service: Some(String::from("api")),
                ..OpenOptions::default()
            },
        )
        .expect("select api");
        assert_eq!(selected.service, "api");

        assert!(select_open_service(&services, &OpenOptions::default()).is_err());

        let stopped_web = status_service("open-stopped", "stopped", Some("2026-06-29T00:01:00Z"));
        assert!(
            select_open_service(
                &[stopped_web],
                &OpenOptions {
                    service: Some(String::from("web")),
                    ..OpenOptions::default()
                },
            )
            .is_err()
        );
    }

    #[test]
    fn config_and_doctor_formatting_helpers_are_stable() {
        assert_eq!(optional_config_value(Some(" value ")), "value");
        assert_eq!(optional_config_value(Some("  ")), "<unset>");
        assert_eq!(optional_config_value(None), "<unset>");
        assert_eq!(configured_value(true), "configured");
        assert_eq!(configured_value(false), "<unset>");
        assert_eq!(list_config_value(Some(1), "entry"), "1 entry");
        assert_eq!(list_config_value(Some(2), "entry"), "2 entries");
        assert_eq!(list_config_value(Some(3), "port"), "3 ports");
        assert_eq!(list_config_value(None, "port"), "<unset>");
        assert_eq!(plural(1, "route"), "route");
        assert_eq!(plural(2, "route"), "routes");
        assert_eq!(source_config_label(ConfigSource::Project), "project config");
        assert_eq!(
            source_config_label(ConfigSource::Fallback),
            "fallback config"
        );
        assert_eq!(non_empty_value(Some("  web  ")), Some("web"));
        assert_eq!(non_empty_value(Some("  ")), None);

        let range = PortRange {
            start: 29_100,
            end: 29_105,
        };
        assert_eq!(
            ports_in_range(&[29_102, 29_102, 29_000, 29_101], range),
            vec![29_101, 29_102]
        );
        assert_eq!(format_limited_ports(&[]), "none");
        assert_eq!(format_limited_ports(&[1, 2, 3]), "1, 2, 3");
        let many_ports = (1..=12).collect::<Vec<u16>>();
        assert_eq!(
            format_limited_ports(&many_ports),
            "1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (+2 more)"
        );
        assert_eq!(
            stale_lease_prune_limit(
                PortRange {
                    start: 29_100,
                    end: 29_110,
                },
                &[29_100, 29_101, 30_000],
            ),
            4
        );
        let scan = ListenerConflictScan {
            known_registry: vec![29_100],
            unknown: vec![29_101],
            scanned_ports: 2,
            total_ports: 10,
        };
        assert_eq!(
            format_listener_conflict_scan(&scan),
            "29101 (scanned first 2 of 10 ports)"
        );
    }

    #[test]
    fn config_and_doctor_diagnostics_render_constructed_state() {
        let cwd = Path::new("/workspace/demo/apps/web");
        let local_override = bindport_core::LoadedLocalConfig {
            path: PathBuf::from("/workspace/demo/.bindport.local.toml"),
            format: bindport_core::ConfigFormat::Toml,
            config: BindPortConfig {
                project: Some(String::from("local-demo")),
                service: Some(String::from("web")),
                services: Some(vec![ServiceConfig {
                    name: Some(String::from("web")),
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                }]),
                outputs: Some(vec![bindport_core::OutputConfig {
                    name: Some(String::from("local-output")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("routes/{{ route.slug }}.yml")),
                    ..bindport_core::OutputConfig::default()
                }]),
                ..BindPortConfig::default()
            },
            unknown_keys: vec![String::from("local_unknown")],
        };
        let config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: PathBuf::from("/workspace/demo/bindport.toml"),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Project,
                local_override: Some(local_override),
                config: BindPortConfig {
                    project: Some(String::from("demo")),
                    service: Some(String::from("web")),
                    default_range: Some(String::from("29100-29110")),
                    skip_ports: Some(vec![29_101, 29_102]),
                    services: Some(vec![ServiceConfig {
                        name: Some(String::from("web")),
                        path: Some(String::from("apps/web")),
                        command: Some(vec![String::from("next"), String::from("dev")]),
                        ..ServiceConfig::default()
                    }]),
                    dashboard: Some(bindport_core::DashboardConfig::default()),
                    output_defaults: Some(bindport_core::OutputDefaultsConfig {
                        root: Some(String::from(".bindport/out")),
                        ..bindport_core::OutputDefaultsConfig::default()
                    }),
                    outputs: Some(vec![bindport_core::OutputConfig {
                        name: Some(String::from("traefik")),
                        template: Some(String::from("bindport-traefik")),
                        target: Some(String::from("routes/{{ route.slug }}.yml")),
                        auto_render: Some(true),
                        delete_on: Some(vec![OutputDeleteState::Removed]),
                        ..bindport_core::OutputConfig::default()
                    }]),
                    ..BindPortConfig::default()
                },
                unknown_keys: vec![String::from("mystery")],
            }),
            fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
            port_range: PortRange {
                start: 29_100,
                end: 29_110,
            },
            skip_ports: vec![29_101, 29_102],
        };
        let no_config = ResolvedConfig {
            loaded: None,
            fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };

        print_config_source_explanation(&config);
        print_config_source_explanation(&no_config);
        print_config_field_explanations(&config);
        print_config_field_explanations(&no_config);
        print_doctor_output_config(&config);
        print_doctor_output_config(&no_config);
        print_config_diagnostics(&config);
        print_config_diagnostics(&no_config);

        let identity = explain_run_identity(
            cwd,
            &strings(["next", "dev"]),
            &RunOptions::default(),
            &config,
        );
        print_identity_diagnostics(&identity.identity);
        assert_eq!(identity.project_source, "local override config `project`");
        assert_eq!(identity.service_source, "local override config `service`");

        let resolver = TemplateResolver::new(None, None);
        let routes = route_records(vec![
            status_service("route-1", "active", None),
            status_service("route-2", "active", None),
            status_service("route-3", "active", None),
            status_service("route-4", "active", None),
            status_service("route-5", "active", None),
            status_service("route-6", "active", None),
        ]);
        let mut output = test_output_config("traefik");
        output.target = String::from("routes/{{ route.key }}.yml");
        assert!(print_doctor_output(
            &output,
            &resolver,
            &routes,
            &temp_test_dir("doctor-output")
        ));

        let empty_range_config = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: PortRange { start: 1, end: 0 },
            skip_ports: Vec::new(),
        };
        assert!(!print_allocation_diagnostics(
            &empty_range_config,
            &identity.identity,
            None
        ));

        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let held_port = listener.local_addr().expect("listener address").port();
        let allocation_config = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: PortRange {
                start: held_port,
                end: held_port,
            },
            skip_ports: Vec::new(),
        };
        print_previous_port_diagnostics(None, &allocation_config, &[]);
        print_previous_port_diagnostics(Some(held_port.saturating_sub(1)), &allocation_config, &[]);
        print_previous_port_diagnostics(Some(held_port), &allocation_config, &[held_port]);
        let skipped_allocation_config = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: PortRange {
                start: held_port,
                end: held_port,
            },
            skip_ports: vec![held_port],
        };
        print_previous_port_diagnostics(Some(held_port), &skipped_allocation_config, &[]);
        print_previous_port_diagnostics(Some(held_port), &allocation_config, &[]);

        print_git_diagnostics(Path::new("/definitely/not-a-bindport-git-worktree"));
        print_git_diagnostics(Path::new(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn command_error_conversions_format_underlying_errors() {
        let render_errors = vec![
            RenderCommandError::from(ConfigError::UnknownFormat {
                path: PathBuf::from("bindport.txt"),
            }),
            RenderCommandError::from(OutputConfigError::MissingName { index: 0 }),
            RenderCommandError::InvalidArgument(String::from("bad render arg")),
            RenderCommandError::from(RegistryError::MissingStateDirectory),
            RenderCommandError::from(AdapterTemplateError::InvalidName(String::from("../bad"))),
            RenderCommandError::from(RenderError::TargetCollision {
                target: String::from("routes/demo.yml"),
                route_keys: vec![String::from("a"), String::from("b")],
            }),
            RenderCommandError::from(OutputFileError::UnsafeRoot {
                root: String::from("../out"),
            }),
        ];

        for error in render_errors {
            assert!(!error.to_string().is_empty());
        }

        let template_config_error = TemplateCommandError::from(ConfigError::UnknownFormat {
            path: PathBuf::from("bindport.txt"),
        });
        assert!(matches!(
            template_config_error,
            TemplateCommandError::Config(_)
        ));
        let template_error =
            TemplateCommandError::from(AdapterTemplateError::InvalidName(String::from("../bad")));
        assert!(matches!(template_error, TemplateCommandError::Template(_)));
        let template_invalid = TemplateCommandError::InvalidArgument(String::from("bad template"));
        assert!(matches!(
            template_invalid,
            TemplateCommandError::InvalidArgument(_)
        ));

        let clean_registry_error = CleanCommandError::from(RegistryError::MissingStateDirectory);
        assert!(matches!(
            clean_registry_error,
            CleanCommandError::Registry(_)
        ));
        let json_error = serde_json::from_str::<serde_json::Value>("{").expect_err("json error");
        let clean_json_error = CleanCommandError::from(json_error);
        assert!(matches!(clean_json_error, CleanCommandError::Json(_)));
    }

    #[test]
    fn render_route_helpers_preserve_status_and_pending_metadata() {
        let services = vec![
            status_service("route-1", "active", Some("2026-06-29T00:05:00Z")),
            status_service("route-2", "stopped", None),
            status_service("route-3", "stale", None),
        ];

        let routes = route_records(services);

        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].key, "route-1");
        assert_eq!(routes[0].updated_at, "2026-06-29T00:05:00Z");
        assert_eq!(routes[1].state, "stopped");
        assert_eq!(routes[1].updated_at, "2026-06-29T00:00:00Z");
        assert_eq!(routes[2].state, "stale");
        assert_eq!(
            route_delete_state(&routes[0]),
            None,
            "active routes are not lifecycle delete candidates"
        );
        assert_eq!(
            route_delete_state(&routes[1]),
            Some(OutputDeleteState::Stopped)
        );
        assert_eq!(
            route_delete_state(&routes[2]),
            Some(OutputDeleteState::Stale)
        );

        let mut output = test_output_config("debug");
        output.delete_on = vec![OutputDeleteState::Stopped];
        assert_eq!(
            delete_route_keys(&output, &routes),
            BTreeSet::from([String::from("route-2")])
        );
        output.delete_on = vec![OutputDeleteState::Stopped, OutputDeleteState::Stale];
        assert_eq!(
            delete_route_keys(&output, &routes),
            BTreeSet::from([String::from("route-2"), String::from("route-3")])
        );

        let identity = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("web"),
            git: Some(bindport_core::GitIdentity {
                worktree_path: PathBuf::from("/workspace/demo-feature-tree"),
                worktree_hash: String::from("abc123456789"),
                git_common_dir: PathBuf::from("/workspace/demo/.git"),
                branch: String::from("feature/tree"),
                branch_label: String::from("feature-tree"),
                commit: String::from("0123456789abcdef"),
            }),
            identity_key: String::from("v1:demo:web"),
        };
        let metadata = RunMetadata {
            command: None,
            hostname: Some(String::from("feature-tree.demo.localhost")),
            route_url: Some(String::from("https://feature-tree.demo.localhost")),
            health_url: Some(String::from("https://feature-tree.demo.localhost/health")),
            env: Vec::new(),
        };
        let pending = pending_route_record(
            &identity,
            29_100,
            &metadata,
            "next dev",
            Path::new("/workspace/demo"),
        );

        assert_eq!(pending.key, "v1:demo:web");
        assert_eq!(pending.state, "active");
        assert_eq!(pending.url, "http://127.0.0.1:29100");
        assert_eq!(
            pending.hostname.as_deref(),
            Some("feature-tree.demo.localhost")
        );
        assert_eq!(pending.branch.as_deref(), Some("feature/tree"));
        assert_eq!(pending.branch_label.as_deref(), Some("feature-tree"));
        assert_eq!(
            pending.worktree_path.as_deref(),
            Some("/workspace/demo-feature-tree")
        );
        assert_eq!(pending.pid, None);
        assert_eq!(pending.started_at, "pending");

        let cwd = Path::new("/workspace/demo/apps/web");
        let config_path = PathBuf::from("/workspace/demo/bindport.toml");
        let project_config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: config_path.clone(),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Project,
                local_override: None,
                config: BindPortConfig::default(),
                unknown_keys: Vec::new(),
            }),
            fallback_path: None,
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };
        assert_eq!(
            output_base_dir(cwd, &project_config),
            Path::new("/workspace/demo")
        );

        let fallback_config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: PathBuf::from("/home/user/.config/bindport/config.toml"),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Fallback,
                local_override: None,
                config: BindPortConfig::default(),
                unknown_keys: Vec::new(),
            }),
            fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };
        assert_eq!(output_base_dir(cwd, &fallback_config), cwd);
    }

    #[cfg(unix)]
    #[test]
    fn exit_status_helpers_preserve_process_codes_and_retry_conditions() {
        let success = ExitStatus::from_raw(0);
        let failure = ExitStatus::from_raw(7 << 8);

        assert_eq!(status_registry_exit_code(&success), Some(0));
        assert_eq!(status_to_exit_code(&success), ExitCode::SUCCESS);
        assert_eq!(status_registry_exit_code(&failure), Some(7));
        assert_eq!(status_to_exit_code(&failure), ExitCode::from(7));

        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let port = listener.local_addr().expect("listener address").port();
        assert!(should_retry_allocation(
            &ExitStatus::from_raw(1 << 8),
            Duration::from_millis(1),
            port
        ));
        assert!(!should_retry_allocation(
            &ExitStatus::from_raw(0),
            Duration::from_millis(1),
            port
        ));
        assert!(!should_retry_allocation(
            &ExitStatus::from_raw(1 << 8),
            ALLOCATION_RETRY_WINDOW + Duration::from_millis(1),
            port
        ));
    }

    fn hook_command(name: &str) -> HookCommandConfig {
        HookCommandConfig {
            name: Some(name.to_string()),
            events: Some(vec![HookEvent::RouteStarted]),
            command: Some(vec![String::from("true")]),
            ..HookCommandConfig::default()
        }
    }

    fn hook_resolved_config(
        source: ConfigSource,
        hooks: HooksConfig,
        local_hooks: Option<HooksConfig>,
    ) -> ResolvedConfig {
        let mut config = BindPortConfig {
            hooks: Some(hooks),
            ..BindPortConfig::default()
        };
        let local_override = local_hooks.map(|hooks| {
            let local_config = BindPortConfig {
                hooks: Some(hooks),
                ..BindPortConfig::default()
            };
            config.merge_local_override(local_config.clone());

            bindport_core::LoadedLocalConfig {
                path: PathBuf::from("/workspace/demo/.bindport.local.toml"),
                format: bindport_core::ConfigFormat::Toml,
                config: local_config,
                unknown_keys: Vec::new(),
            }
        });

        ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: match source {
                    ConfigSource::Project => PathBuf::from("/workspace/demo/bindport.toml"),
                    ConfigSource::Fallback => {
                        PathBuf::from("/home/user/.config/bindport/config.toml")
                    }
                },
                format: bindport_core::ConfigFormat::Toml,
                source,
                local_override,
                config,
                unknown_keys: Vec::new(),
            }),
            fallback_path: None,
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        }
    }

    fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
        values.into_iter().map(String::from).collect()
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bindport-cli-{name}-{}-{now}", std::process::id()));
        fs::create_dir_all(&path).expect("temp test dir");
        path
    }

    fn test_output_config(name: &str) -> EffectiveOutputConfig {
        EffectiveOutputConfig {
            name: name.to_string(),
            template: String::from("bindport-traefik"),
            root: None,
            target: String::from("{{ route.slug }}.yml"),
            target_host: String::from("127.0.0.1"),
            target_scheme: String::from("http"),
            auto_render: true,
            delete_on: Vec::new(),
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 0,
            vars: BTreeMap::new(),
        }
    }

    fn status_service(identity_key: &str, state: &str, exited_at: Option<&str>) -> StatusService {
        StatusService {
            project: String::from("demo"),
            service: String::from("web"),
            state: state.to_string(),
            port: 29_100,
            host: String::from("127.0.0.1"),
            url: String::from("http://127.0.0.1:29100"),
            hostname: Some(String::from("feature-tree.demo.localhost")),
            route_url: Some(String::from("https://feature-tree.demo.localhost")),
            health_url: Some(String::from("https://feature-tree.demo.localhost/health")),
            worktree_path: Some(String::from("/workspace/demo-feature-tree")),
            worktree_hash: Some(String::from("abc123456789")),
            git_common_dir: Some(String::from("/workspace/demo/.git")),
            branch: Some(String::from("feature/tree")),
            branch_label: Some(String::from("feature-tree")),
            commit: Some(String::from("0123456789abcdef")),
            identity_key: Some(identity_key.to_string()),
            pid: Some(12_345),
            command: String::from("next dev"),
            cwd: String::from("/workspace/demo"),
            started_at: String::from("2026-06-29T00:00:00Z"),
            exited_at: exited_at.map(str::to_string),
            exit_code: None,
            health: String::from("unknown"),
            outputs: Vec::new(),
            proxy: None,
        }
    }
}
