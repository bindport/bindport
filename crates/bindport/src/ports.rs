use super::*;

pub(crate) fn ports_in_range(ports: &[u16], range: PortRange) -> Vec<u16> {
    let mut ports = ports
        .iter()
        .copied()
        .filter(|port| range.contains(*port))
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[derive(Debug, Default)]
struct PortOptions {
    service: Option<String>,
    project: Option<String>,
    help: bool,
}

#[derive(Debug)]
enum PortCommandError {
    InvalidArgument(String),
    Config(ConfigError),
    Registry(RegistryError),
}

pub(crate) fn run_port_command(args: &[String]) -> ExitCode {
    match run_port_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(PortCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport port <service> [--project PROJECT]");
            ExitCode::FAILURE
        }
        Err(PortCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(PortCommandError::Registry(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_port_command_result(args: &[String]) -> Result<(), PortCommandError> {
    let options = parse_port_options(args)?;
    if options.help {
        print_port_help();
        return Ok(());
    }
    let service = options.service.as_deref().ok_or_else(|| {
        PortCommandError::InvalidArgument(String::from("bindport port requires a service name"))
    })?;
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let config_project = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.project.as_deref());
    let identity = resolve_identity(IdentitySources {
        cwd: &cwd,
        command: &[],
        cli_project: options.project.as_deref(),
        cli_service: Some(service),
        env_project: env_project.as_deref(),
        env_service: None,
        config_project,
        config_service: None,
    });
    let selected = Registry::open_default()?.select_service(&identity)?;

    println!("{}", selected.port);

    Ok(())
}

fn parse_port_options(args: &[String]) -> Result<PortOptions, PortCommandError> {
    let mut options = PortOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--project" => {
                index += 1;
                options.project = Some(
                    args.get(index)
                        .ok_or_else(|| {
                            PortCommandError::InvalidArgument(String::from(
                                "--project requires a value",
                            ))
                        })?
                        .clone(),
                );
            }
            "--help" | "-h" => options.help = true,
            value if value.starts_with('-') => {
                return Err(PortCommandError::InvalidArgument(format!(
                    "unknown port option `{value}`"
                )));
            }
            service => {
                if options.service.is_some() {
                    return Err(PortCommandError::InvalidArgument(String::from(
                        "bindport port accepts exactly one service name",
                    )));
                }
                options.service = Some(service.to_string());
            }
        }
        index += 1;
    }

    Ok(options)
}

impl From<ConfigError> for PortCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<RegistryError> for PortCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}
