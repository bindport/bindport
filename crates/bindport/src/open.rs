use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpenOptions {
    pub(crate) service: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) browser: bool,
    pub(crate) help: bool,
}

pub(crate) fn run_open_command(args: &[String]) -> ExitCode {
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

pub(crate) fn run_open_command_result(args: &[String]) -> Result<(), OpenCommandError> {
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

pub(crate) fn parse_open_options(args: &[String]) -> Result<OpenOptions, OpenCommandError> {
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

pub(crate) fn select_open_service<'a>(
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

pub(crate) fn open_not_found_message(options: &OpenOptions) -> String {
    match (&options.project, &options.service) {
        (Some(project), Some(service)) => {
            format!("no active BindPort service matched `{project}/{service}`")
        }
        (None, Some(service)) => format!("no active BindPort service matched `{service}`"),
        (Some(project), None) => format!("no active BindPort service matched project `{project}`"),
        (None, None) => String::from("no active BindPort services recorded"),
    }
}

pub(crate) fn open_ambiguous_message(options: &OpenOptions, services: &[&StatusService]) -> String {
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

pub(crate) fn best_service_url(service: &StatusService) -> String {
    service
        .route_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(&service.url)
        .to_string()
}

pub(crate) fn open_url_in_browser(url: &str) -> io::Result<()> {
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

pub(crate) fn validate_browser_url(url: &str) -> io::Result<&str> {
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

pub(crate) fn invalid_browser_url() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "browser launch only supports http:// and https:// URLs",
    )
}

#[derive(Debug)]
pub(crate) enum OpenCommandError {
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
