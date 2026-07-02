use super::*;

#[derive(Debug)]
pub(crate) enum DashboardCommandError {
    Config(ConfigError),
    Dashboard(bindport_dashboard::DashboardError),
    InvalidArgument(String),
    Io(io::Error),
    MissingToken { source_name: String },
}

impl From<ConfigError> for DashboardCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<bindport_dashboard::DashboardError> for DashboardCommandError {
    fn from(error: bindport_dashboard::DashboardError) -> Self {
        Self::Dashboard(error)
    }
}

impl From<io::Error> for DashboardCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
