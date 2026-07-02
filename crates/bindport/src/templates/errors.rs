use super::*;

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
