use super::*;

#[derive(Debug, Default)]
struct HostnameOptions {
    service: Option<String>,
    project: Option<String>,
    help: bool,
}

#[derive(Debug)]
enum HostnameCommandError {
    InvalidArgument(String),
    Config(ConfigError),
    Registry(RegistryError),
    MissingHostname { project: String, service: String },
}

pub(crate) fn run_hostname_command(args: &[String]) -> ExitCode {
    match run_hostname_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(HostnameCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport hostname <service> [--project PROJECT]");
            ExitCode::FAILURE
        }
        Err(HostnameCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(HostnameCommandError::Registry(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(HostnameCommandError::MissingHostname { project, service }) => {
            eprintln!(
                "bindport: active or reserved service `{project}/{service}` has no hostname metadata"
            );
            ExitCode::FAILURE
        }
    }
}

fn run_hostname_command_result(args: &[String]) -> Result<(), HostnameCommandError> {
    let options = parse_hostname_options(args)?;
    if options.help {
        print_hostname_help();
        return Ok(());
    }
    let service = options.service.as_deref().ok_or_else(|| {
        HostnameCommandError::InvalidArgument(String::from(
            "bindport hostname requires a service name",
        ))
    })?;
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let identity =
        resolve_service_identity_in_current_scope(&cwd, options.project.as_deref(), service)?;
    let selected = Registry::open_default()?.select_service(&identity)?;
    let hostname = selected
        .hostname
        .as_deref()
        .filter(|hostname| !hostname.trim().is_empty())
        .ok_or_else(|| HostnameCommandError::MissingHostname {
            project: selected.project.clone(),
            service: selected.service.clone(),
        })?;

    println!("{hostname}");

    Ok(())
}

fn parse_hostname_options(args: &[String]) -> Result<HostnameOptions, HostnameCommandError> {
    let mut options = HostnameOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--project" => {
                index += 1;
                options.project = Some(
                    args.get(index)
                        .ok_or_else(|| {
                            HostnameCommandError::InvalidArgument(String::from(
                                "--project requires a value",
                            ))
                        })?
                        .clone(),
                );
            }
            "--help" | "-h" => options.help = true,
            value if value.starts_with('-') => {
                return Err(HostnameCommandError::InvalidArgument(format!(
                    "unknown hostname option `{value}`"
                )));
            }
            service => {
                if options.service.is_some() {
                    return Err(HostnameCommandError::InvalidArgument(String::from(
                        "bindport hostname accepts exactly one service name",
                    )));
                }
                options.service = Some(service.to_string());
            }
        }
        index += 1;
    }

    Ok(options)
}

impl From<ConfigError> for HostnameCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<RegistryError> for HostnameCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}
