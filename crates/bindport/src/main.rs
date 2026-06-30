// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, BufRead, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

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
    ConfigSource, ConfiguredServiceSource, DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS,
    EffectiveOutputConfig, FALLBACK_CONFIG_FILE, IdentitySources, LoadedConfig, OutputConfigError,
    OutputDeleteState, OutputFailurePolicy, PortRange, SERVICE_NAME, ServiceConfig,
    ServiceIdentity, default_fallback_config, detect_git_identity, discover_config,
    normalize_branch_label, resolve_identity,
};
use bindport_dashboard::{DashboardCleanCallback, DashboardOptions, DashboardServer};
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
const DASHBOARD_STATE_FILE: &str = "dashboard.state";
const DASHBOARD_LOG_FILE: &str = "dashboard.log";

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
        Some("clean") => clean_registry(&args[1..]),
        Some("config") => run_config_command(&args[1..]),
        Some("doctor") => run_doctor_command(&args[1..]),
        Some("dashboard") => run_dashboard(&args[1..]),
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
    env: Vec<(String, String)>,
}

fn run_subcommand(args: &[String]) -> ExitCode {
    match parse_run_options(args) {
        Ok((options, command)) => run_wrapped_command(command, options),
        Err(error) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport run [service] [--env NAME=VALUE] [--hostname TEMPLATE] [--route-url TEMPLATE] -- <command>"
            );
            ExitCode::FAILURE
        }
    }
}

fn parse_run_options(args: &[String]) -> Result<(RunOptions, &[String]), String> {
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| String::from("missing `--` before wrapped command"))?;
    let (option_args, command) = args.split_at(separator);
    let command = &command[1..];
    if command.is_empty() {
        return Err(String::from("no command provided after `--`"));
    }

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
    if command.is_empty() {
        return Err(RunnerError::NoCommand.into());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let identity = resolve_run_identity(&cwd, command, options, &config);
    let service_config = configured_service(&config, &identity);
    let run_templates = resolve_run_templates(options, service_config);
    let requires_output_preflight = has_blocking_auto_outputs(&config)?;
    let mut registry = open_optional_registry();
    let mut skip_ports = config.skip_ports.clone();
    let mut previous_port = None;

    let mut disable_registry = false;
    if let Some(registry) = registry.as_mut() {
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
    let command_display = command.join(" ");

    loop {
        let allocation_hints = AllocationHints {
            preferred_port: previous_port,
            scan_start: identity.port_scan_start(config.port_range),
        };
        let port = allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints)?;
        let run_metadata = resolve_run_metadata(&identity, port, &run_templates)?;
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
        let mut child = spawn_child_on_port(command, port, &run_metadata.env)?;
        let attempt_started_at = Instant::now();
        let run = RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port,
            hostname: run_metadata.hostname.clone(),
            route_url: run_metadata.route_url.clone(),
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
    hostname: Option<String>,
    route_url: Option<String>,
    env: Vec<(String, String)>,
}

#[derive(Debug)]
struct RunMetadata {
    hostname: Option<String>,
    route_url: Option<String>,
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
    options: &RunOptions,
    service_config: Option<&ServiceConfig>,
) -> RunTemplates {
    let mut templates = RunTemplates::default();

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
    let base_values = TemplateValues::new(identity, port, None, None);
    let hostname = templates
        .hostname
        .as_deref()
        .map(|template| expand_template(template, &base_values))
        .transpose()?;
    let route_values = TemplateValues::new(identity, port, hostname.as_deref(), None);
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
    let env_values = TemplateValues::new(identity, port, hostname.as_deref(), route_url.as_deref());
    let env = templates
        .env
        .iter()
        .map(|(name, template)| {
            expand_template(template, &env_values).map(|value| (name.clone(), value))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RunMetadata {
        hostname,
        route_url,
        env,
    })
}

struct TemplateValues<'a> {
    identity: &'a ServiceIdentity,
    port: u16,
    hostname: Option<&'a str>,
    route_url: Option<&'a str>,
    host: &'static str,
    url: String,
}

impl<'a> TemplateValues<'a> {
    fn new(
        identity: &'a ServiceIdentity,
        port: u16,
        hostname: Option<&'a str>,
        route_url: Option<&'a str>,
    ) -> Self {
        let host = "127.0.0.1";

        Self {
            identity,
            port,
            hostname,
            route_url,
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

    if outputs.is_empty() {
        return Ok(0);
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

    render_outputs(
        cwd,
        config,
        registry,
        invocation.outputs,
        invocation.dry_run,
        invocation.mode,
        invocation.report,
    )
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
            eprintln!("usage: bindport clean [--dry-run] [--stopped] [--stale] [--json]");
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
    let summary = registry.clean_leases(&states, options.dry_run)?;

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
        help: false,
    };

    for arg in args {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--json" => options.json = true,
            "--stopped" => options.stopped = true,
            "--stale" => options.stale = true,
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

fn print_status_json() -> ExitCode {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => match serde_json::to_string_pretty(&snapshot) {
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
        None | Some("--help" | "-h") => {
            print_config_help();
            ExitCode::SUCCESS
        }
        Some("explain") => {
            eprintln!("bindport: config explain does not take arguments");
            eprintln!("usage: bindport config explain");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("bindport: unknown config command `{command}`");
            eprintln!("usage: bindport config explain");
            ExitCode::FAILURE
        }
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
    let listener_conflicts = listener_conflicts(config.port_range);
    println!(
        "os listener conflicts in range: {}",
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
    conflicts: Vec<u16>,
    scanned_ports: u32,
    total_ports: u32,
}

fn listener_conflicts(range: PortRange) -> ListenerConflictScan {
    let total_ports = range.len();
    let scanned_ports = total_ports.min(DOCTOR_MAX_LISTENER_PROBES);
    let conflicts = (0..scanned_ports)
        .filter_map(|offset| {
            let port = range.start as u32 + offset;
            let port = u16::try_from(port).expect("port remains within configured range");

            if is_port_available(port) {
                None
            } else {
                Some(port)
            }
        })
        .collect();

    ListenerConflictScan {
        conflicts,
        scanned_ports,
        total_ports,
    }
}

fn format_listener_conflict_scan(scan: &ListenerConflictScan) -> String {
    let mut summary = format_limited_ports(&scan.conflicts);

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
    println!("  bindport run [service] [options] -- <command>");
    println!("                                  Run a command with service env templates");
    println!("  bindport status [--json]     Show registry status");
    println!("  bindport clean [--dry-run]   Remove stopped and stale registry entries");
    println!("  bindport config explain      Explain resolved config and identity sources");
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
}

fn print_config_help() {
    println!("BindPort config");
    println!();
    println!("Usage:");
    println!("  bindport config explain");
    println!();
    println!("Commands:");
    println!("  explain    Show resolved config fields and identity sources");
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

    #[test]
    fn empty_args_print_help_successfully() {
        assert_eq!(run([]), ExitCode::SUCCESS);
    }

    #[test]
    fn version_arg_succeeds() {
        assert_eq!(run([String::from("--version")]), ExitCode::SUCCESS);
    }

    #[test]
    fn empty_runner_command_fails() {
        assert_eq!(run([String::from("--")]), ExitCode::FAILURE);
    }

    #[test]
    fn route_event_collector_retains_source_and_kind() {
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
    }
}
