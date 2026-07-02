use super::*;

#[derive(Debug)]
pub(crate) enum CleanCommandError {
    InvalidArgument(String),
    Registry(RegistryError),
    Json(serde_json::Error),
    Io(io::Error),
    ConfirmationRequired(String),
    Aborted,
}

impl From<RegistryError> for CleanCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<serde_json::Error> for CleanCommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<io::Error> for CleanCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
