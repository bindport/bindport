use super::*;

#[derive(Debug)]
pub enum DashboardError {
    NoAvailablePort { range: PortRange },
    Bind { port: u16, source: io::Error },
    LocalAddress(io::Error),
}

impl fmt::Display for DashboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAvailablePort { range } => write!(
                f,
                "no dashboard port available in range {}-{}",
                range.start, range.end
            ),
            Self::Bind { port, source } => {
                write!(f, "failed to bind dashboard port {port}: {source}")
            }
            Self::LocalAddress(source) => write!(f, "failed to read dashboard address: {source}"),
        }
    }
}

impl std::error::Error for DashboardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bind { source, .. } | Self::LocalAddress(source) => Some(source),
            Self::NoAvailablePort { .. } => None,
        }
    }
}
