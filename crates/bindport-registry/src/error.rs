use super::*;

#[derive(Debug)]
pub enum RegistryError {
    MissingStateDirectory,
    CreateDirectory {
        path: PathBuf,
        source: io::Error,
    },
    UnsafePath {
        path: PathBuf,
        message: &'static str,
    },
    PortConflict {
        port: u16,
    },
    ServiceNotFound {
        project: String,
        service: String,
    },
    AmbiguousService {
        project: String,
        service: String,
    },
    ReservationNotFound {
        lease_id: i64,
    },
    Open {
        path: PathBuf,
        source: rusqlite::Error,
    },
    Sqlite(rusqlite::Error),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingStateDirectory => {
                write!(
                    f,
                    "could not determine registry directory; set {REGISTRY_PATH_ENV}"
                )
            }
            Self::CreateDirectory { path, source } => {
                write!(
                    f,
                    "failed to create registry directory `{}`: {source}",
                    path.display()
                )
            }
            Self::UnsafePath { path, message } => {
                write!(f, "unsafe registry path `{}`: {message}", path.display())
            }
            Self::PortConflict { port } => {
                write!(f, "port {port} is already active in the registry")
            }
            Self::ServiceNotFound { project, service } => write!(
                f,
                "no active or reserved service matched `{project}/{service}` in the current worktree"
            ),
            Self::AmbiguousService { project, service } => write!(
                f,
                "multiple active or reserved services matched `{project}/{service}` in the current worktree"
            ),
            Self::ReservationNotFound { lease_id } => {
                write!(f, "reserved lease {lease_id} is no longer available")
            }
            Self::Open { path, source } => {
                write!(f, "failed to open registry `{}`: {source}", path.display())
            }
            Self::Sqlite(source) => write!(f, "registry database error: {source}"),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDirectory { source, .. } => Some(source),
            Self::Open { source, .. } | Self::Sqlite(source) => Some(source),
            Self::MissingStateDirectory
            | Self::UnsafePath { .. }
            | Self::PortConflict { .. }
            | Self::ServiceNotFound { .. }
            | Self::AmbiguousService { .. }
            | Self::ReservationNotFound { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for RegistryError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}
