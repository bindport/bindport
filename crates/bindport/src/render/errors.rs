use super::*;

#[derive(Debug)]
pub(crate) enum RenderCommandError {
    Config(ConfigError),
    OutputConfig(OutputConfigError),
    InvalidArgument(String),
    Registry(RegistryError),
    Template(AdapterTemplateError),
    Render(RenderError),
    File(OutputFileError),
}

impl std::fmt::Display for RenderCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::OutputConfig(error) => write!(f, "{error}"),
            Self::InvalidArgument(error) => write!(f, "{error}"),
            Self::Registry(error) => write!(f, "{error}"),
            Self::Template(error) => write!(f, "{error}"),
            Self::Render(error) => write!(f, "{error}"),
            Self::File(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RenderCommandError {}

impl From<ConfigError> for RenderCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<OutputConfigError> for RenderCommandError {
    fn from(error: OutputConfigError) -> Self {
        Self::OutputConfig(error)
    }
}

impl From<RegistryError> for RenderCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<AdapterTemplateError> for RenderCommandError {
    fn from(error: AdapterTemplateError) -> Self {
        Self::Template(error)
    }
}

impl From<RenderError> for RenderCommandError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<OutputFileError> for RenderCommandError {
    fn from(error: OutputFileError) -> Self {
        Self::File(error)
    }
}
