use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemplateCommand {
    List,
    Show,
    Export,
    Help,
}

#[derive(Debug, Default)]
pub(crate) struct TemplateCommandOptions {
    pub(crate) source: Option<TemplateSource>,
    pub(crate) name: Option<String>,
}

pub(crate) fn run_template_command(args: &[String]) -> ExitCode {
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

pub(crate) fn run_template_command_result(args: &[String]) -> Result<(), TemplateCommandError> {
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

pub(crate) fn parse_template_command(
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

pub(crate) fn parse_template_source(value: &str) -> Result<TemplateSource, TemplateCommandError> {
    match value {
        "project" => Ok(TemplateSource::Project),
        "global" => Ok(TemplateSource::Global),
        "built-in" | "builtin" => Ok(TemplateSource::BuiltIn),
        _ => Err(TemplateCommandError::InvalidArgument(format!(
            "invalid template source `{value}`"
        ))),
    }
}

pub(crate) fn template_resolver(cwd: &Path) -> Result<TemplateResolver, ConfigError> {
    let config = resolve_config(cwd)?;

    Ok(TemplateResolver::new(
        Some(project_template_dir(cwd, &config)),
        global_template_dir(),
    ))
}

pub(crate) fn project_template_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .join(".bindport")
        .join("templates")
}

pub(crate) fn global_template_dir() -> Option<PathBuf> {
    fallback_config_path()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("templates")))
}

pub(crate) fn print_template_list(
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

pub(crate) fn print_template_show(
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

pub(crate) fn print_template_export(
    resolver: &TemplateResolver,
    name: &str,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let template = resolver.resolve(name, source)?;
    print!("{}", template.contents);

    Ok(())
}

#[derive(Debug)]
pub(crate) enum TemplateCommandError {
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
