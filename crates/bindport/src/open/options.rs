use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpenOptions {
    pub(crate) service: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) browser: bool,
    pub(crate) help: bool,
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
