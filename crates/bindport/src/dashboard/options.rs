use super::*;

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
