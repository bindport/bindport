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
