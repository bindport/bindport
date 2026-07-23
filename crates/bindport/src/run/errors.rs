use super::*;

#[derive(Debug)]
pub(crate) enum RunCommandError {
    Runner(RunnerError),
    Config(ConfigError),
    ExecutionContext(ServiceExecutionContextError),
    Template(TemplateError),
    SiblingResolution(RegistryError),
    OutputRender(RenderCommandError),
    ReservedPortUnavailable { port: u16 },
    ReservedPromotion { port: u16, source: RegistryError },
}

#[derive(Debug)]
pub(crate) enum ServiceExecutionContextError {
    InvalidPath {
        service: String,
        path: PathBuf,
        source: io::Error,
    },
    NotDirectory {
        service: String,
        path: PathBuf,
    },
    OutsideProject {
        service: String,
        path: PathBuf,
        project_root: PathBuf,
    },
    InvalidPathEnvironment {
        source: env::JoinPathsError,
    },
}

impl fmt::Display for ServiceExecutionContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath {
                service,
                path,
                source,
            } => write!(
                formatter,
                "configured service `{service}` path `{}` is unavailable: {source}",
                path.display()
            ),
            Self::NotDirectory { service, path } => write!(
                formatter,
                "configured service `{service}` path `{}` is not a directory",
                path.display()
            ),
            Self::OutsideProject {
                service,
                path,
                project_root,
            } => write!(
                formatter,
                "configured service `{service}` path `{}` resolves outside project root `{}`",
                path.display(),
                project_root.display()
            ),
            Self::InvalidPathEnvironment { source } => {
                write!(formatter, "failed to construct child PATH: {source}")
            }
        }
    }
}

impl From<ServiceExecutionContextError> for RunCommandError {
    fn from(error: ServiceExecutionContextError) -> Self {
        Self::ExecutionContext(error)
    }
}

impl From<RunnerError> for RunCommandError {
    fn from(error: RunnerError) -> Self {
        Self::Runner(error)
    }
}

impl From<ConfigError> for RunCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<TemplateError> for RunCommandError {
    fn from(error: TemplateError) -> Self {
        Self::Template(error)
    }
}

impl From<RenderCommandError> for RunCommandError {
    fn from(error: RenderCommandError) -> Self {
        Self::OutputRender(error)
    }
}
