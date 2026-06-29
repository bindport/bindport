// SPDX-License-Identifier: MIT

use std::{
    env, fs,
    io::{self, BufRead, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use bindport_adapters::{
    AdapterKind, TemplateError as AdapterTemplateError, TemplateResolver, TemplateSource,
};
use bindport_core::{
    APPLIED_CONFIG_KEYS, BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, ConfigError, ConfigSource,
    DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, FALLBACK_CONFIG_FILE, IdentitySources, LoadedConfig,
    PortRange, SERVICE_NAME, ServiceConfig, ServiceIdentity, default_fallback_config,
    detect_git_identity, discover_config, normalize_branch_label, resolve_identity,
};
use bindport_dashboard::{DashboardOptions, DashboardServer};
use bindport_registry::{
    CleanState, CleanSummary, REGISTRY_PATH_ENV, Registry, RegistryError, RunStart, StartedRun,
    default_registry_path,
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
        Some("doctor") => print_doctor(),
        Some("dashboard") => run_dashboard(&args[1..]),
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
                Ok(started) => Some(started),
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

        if let (Some(registry), Some(started)) = (registry.as_mut(), started)
            && let Err(error) = registry.record_run_finished(started, exit_code)
        {
            print_registry_warning("failed to record run finish", &error);
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
        .and_then(|loaded| loaded.config.configured_service_name());

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

    let dashboard = resolve_dashboard_options(&config, options, skip_ports)?;
    let register_service = resolve_dashboard_registration(&config, options)?;
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
    let summary = Registry::open_default()
        .and_then(|mut registry| registry.clean_leases(&states, options.dry_run))?;

    if options.json {
        print_clean_json(summary, options.dry_run)?;
    } else {
        print_clean_summary(summary, options.dry_run);
    }

    Ok(())
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
    println!("  bindport doctor              Show bootstrap diagnostics");
    println!("  bindport dashboard [serve]   Serve the local dashboard");
    println!("  bindport dashboard start     Start the dashboard in the background");
    println!("  bindport dashboard status    Show background dashboard status");
    println!("  bindport dashboard stop      Stop the background dashboard");
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
}
