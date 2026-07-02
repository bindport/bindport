use super::*;

#[derive(Debug)]
pub(crate) enum HooksCommandError {
    Config(ConfigError),
    Io(io::Error),
    InvalidArgument(String),
}

impl From<ConfigError> for HooksCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<io::Error> for HooksCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
