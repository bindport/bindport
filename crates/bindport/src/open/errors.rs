use super::*;

#[derive(Debug)]
pub(crate) enum OpenCommandError {
    InvalidArgument(String),
    Config(ConfigError),
    Registry(RegistryError),
    Browser(io::Error),
    Selection(String),
}

impl From<ConfigError> for OpenCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
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
