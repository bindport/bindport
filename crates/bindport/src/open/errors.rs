use super::*;

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
