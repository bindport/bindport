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
    UnsupportedRegistryVersion {
        path: PathBuf,
        found: i64,
        supported: i64,
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
    ReservationRestoreConflict {
        lease_id: i64,
        port: u16,
    },
    ConcurrentReservation {
        project: String,
        service: String,
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
            Self::UnsupportedRegistryVersion {
                path,
                found,
                supported,
            } => write!(
                f,
                "registry `{}` uses unsupported user_version {found}; this BindPort supports through {supported}",
                path.display()
            ),
            Self::PortConflict { port } => {
                write!(f, "port {port} is already active in the registry")
            }
            Self::ServiceNotFound { project, service } => write!(
                f,
                "no active or reserved service matched `{project}/{service}` in the current project scope (Git worktree and branch when available)"
            ),
            Self::AmbiguousService { project, service } => write!(
                f,
                "multiple active or reserved services matched `{project}/{service}` in the current project scope (Git worktree and branch when available)"
            ),
            Self::ReservationNotFound { lease_id } => {
                write!(f, "reserved lease {lease_id} is no longer available")
            }
            Self::ReservationRestoreConflict { lease_id, port } => write!(
                f,
                "reserved lease {lease_id} could not be restored because port {port} is now owned by another active or reserved service; the failed lease was stopped"
            ),
            Self::ConcurrentReservation { project, service } => write!(
                f,
                "reservation for `{project}/{service}` changed repeatedly during allocation; retry the command"
            ),
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
            | Self::UnsupportedRegistryVersion { .. }
            | Self::PortConflict { .. }
            | Self::ServiceNotFound { .. }
            | Self::AmbiguousService { .. }
            | Self::ReservationNotFound { .. }
            | Self::ReservationRestoreConflict { .. }
            | Self::ConcurrentReservation { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for RegistryError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}
