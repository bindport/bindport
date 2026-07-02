use super::*;

pub(crate) fn run_dashboard(args: &[String]) -> ExitCode {
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

pub(crate) fn run_dashboard_result(args: &[String]) -> Result<(), DashboardCommandError> {
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

pub(crate) fn serve_dashboard(options: &DashboardCliOptions) -> Result<(), DashboardCommandError> {
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
pub(crate) enum DashboardCommand {
    Serve,
    Start,
    Status,
    Stop,
    Help,
}

#[derive(Debug, Default)]
pub(crate) struct DashboardCliOptions {
    pub(crate) host: Option<Ipv4Addr>,
    pub(crate) port: Option<u16>,
    pub(crate) auth_required: Option<bool>,
    pub(crate) register_service: Option<bool>,
    pub(crate) token: Option<String>,
    pub(crate) token_env: Option<String>,
    pub(crate) allowed_hosts: Vec<String>,
    pub(crate) static_dir: Option<PathBuf>,
    pub(crate) serve_args: Vec<String>,
}

impl DashboardCliOptions {
    pub(crate) fn token_env_name(&self) -> &str {
        self.token_env.as_deref().unwrap_or(DASHBOARD_TOKEN_ENV)
    }
}

pub(crate) fn parse_dashboard_command(
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

pub(crate) fn parse_dashboard_options(
    args: &[String],
) -> Result<DashboardCliOptions, DashboardCommandError> {
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

pub(crate) fn dashboard_option_value(
    args: &[String],
    index: &mut usize,
    option: &'static str,
) -> Result<String, DashboardCommandError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| DashboardCommandError::InvalidArgument(format!("{option} requires a value")))
}

pub(crate) fn parse_dashboard_auth_mode(value: &str) -> Result<bool, DashboardCommandError> {
    parse_dashboard_bool(value, "dashboard auth mode")
}

pub(crate) fn parse_dashboard_bool(
    value: &str,
    setting: &str,
) -> Result<bool, DashboardCommandError> {
    match value {
        "required" | "require" | "enabled" | "true" | "1" | "yes" => Ok(true),
        "disabled" | "disable" | "false" | "0" | "no" => Ok(false),
        _ => Err(DashboardCommandError::InvalidArgument(format!(
            "invalid {setting} `{value}`"
        ))),
    }
}

pub(crate) fn resolve_dashboard_options(
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

pub(crate) fn resolve_dashboard_registration(
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

pub(crate) fn env_dashboard_host() -> Result<Option<Ipv4Addr>, DashboardCommandError> {
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

pub(crate) fn env_dashboard_port() -> Result<Option<u16>, DashboardCommandError> {
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

pub(crate) fn env_dashboard_auth_required() -> Result<Option<bool>, DashboardCommandError> {
    env::var(DASHBOARD_AUTH_REQUIRED_ENV)
        .ok()
        .map(|value| parse_dashboard_auth_mode(&value))
        .transpose()
}

pub(crate) fn env_dashboard_register_service() -> Result<Option<bool>, DashboardCommandError> {
    env::var(DASHBOARD_REGISTER_SERVICE_ENV)
        .ok()
        .map(|value| parse_dashboard_bool(&value, DASHBOARD_REGISTER_SERVICE_ENV))
        .transpose()
}

pub(crate) fn start_dashboard_service(
    options: &DashboardCliOptions,
) -> Result<(), DashboardCommandError> {
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

pub(crate) fn dashboard_start_error() -> io::Error {
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

pub(crate) fn print_dashboard_service_status() -> Result<(), DashboardCommandError> {
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

pub(crate) fn stop_dashboard_service() -> Result<(), DashboardCommandError> {
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

pub(crate) fn redacted_dashboard_command() -> String {
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
pub(crate) struct DashboardServiceState {
    pub(crate) pid: u32,
    pub(crate) url: String,
    pub(crate) process_start_time: Option<u64>,
}

pub(crate) fn read_dashboard_state() -> Result<Option<DashboardServiceState>, DashboardCommandError>
{
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

pub(crate) fn write_dashboard_state(
    state: &DashboardServiceState,
) -> Result<(), DashboardCommandError> {
    let path = dashboard_state_path()?;
    create_dashboard_state_dir()?;
    let mut contents = format!("pid={}\nurl={}\n", state.pid, state.url);
    if let Some(process_start_time) = state.process_start_time {
        contents.push_str(&format!("process_start_time={process_start_time}\n"));
    }
    fs::write(path, contents)?;
    Ok(())
}

pub(crate) fn remove_dashboard_state() -> io::Result<()> {
    match fs::remove_file(dashboard_state_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn dashboard_process_is_running(state: &DashboardServiceState) -> bool {
    process_is_running(state.pid) && dashboard_process_matches_state(state)
}

#[cfg(target_os = "linux")]
pub(crate) fn dashboard_process_matches_state(state: &DashboardServiceState) -> bool {
    match state.process_start_time {
        Some(expected) => {
            process_start_time(state.pid) == Some(expected)
                && process_cmdline_is_dashboard(state.pid)
        }
        None => process_cmdline_is_dashboard(state.pid),
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn dashboard_process_matches_state(_state: &DashboardServiceState) -> bool {
    // Non-Linux targets do not have the /proc fields used above. Dashboard stop
    // falls back to PID liveness there, which can be fooled by PID reuse.
    true
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(19)?.parse::<u64>().ok()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn process_start_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
pub(crate) fn process_cmdline_is_dashboard(pid: u32) -> bool {
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
pub(crate) fn process_is_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
pub(crate) fn process_is_running(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn terminate_process(pid: u32) -> io::Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn dashboard_clean_callback(
    cwd: PathBuf,
    config: ResolvedConfig,
) -> DashboardCleanCallback {
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

pub(crate) fn dashboard_status_callback(cwd: PathBuf) -> DashboardStatusCallback {
    Arc::new(move || match resolve_config(&cwd) {
        Ok(config) => hooks_status_json(&cwd, &config),
        Err(error) => serde_json::json!({
            "error": error.to_string(),
            "items": [],
        }),
    })
}

#[cfg(not(unix))]
pub(crate) fn terminate_process(_pid: u32) -> io::Result<()> {
    Err(io::Error::other(
        "dashboard stop is not implemented on this platform",
    ))
}

#[derive(Debug)]
pub(crate) enum DashboardCommandError {
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
