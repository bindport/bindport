use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFileOwnership {
    pub path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovableOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub status: OutputFileRemovalStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileRemovalStatus {
    Removed,
    Missing,
    OutsideRoot,
    ExternalModified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub content_hash: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutputFile {
    pub route_key: String,
    pub target: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileDiffStatus {
    Added,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffedOutputFile {
    pub route_key: String,
    pub target: String,
    pub path: PathBuf,
    pub status: OutputFileDiffStatus,
    pub old_contents: Option<String>,
    pub new_contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffedRemovalOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub status: OutputFileRemovalStatus,
    pub old_contents: Option<String>,
}
#[derive(Debug)]
pub enum OutputFileError {
    UnsafeRoot { root: String },
    UnsafeTarget { target: String },
    TargetEscapesRoot { target: String, root: PathBuf },
    SymlinkInPath { path: PathBuf },
    UnownedTarget { path: PathBuf },
    ExternalModified { path: PathBuf },
    Io { path: PathBuf, source: io::Error },
}
impl fmt::Display for OutputFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeRoot { root } => write!(f, "unsafe output root `{root}`"),
            Self::UnsafeTarget { target } => write!(f, "unsafe output target `{target}`"),
            Self::TargetEscapesRoot { target, root } => write!(
                f,
                "output target `{target}` escapes output root `{}`",
                root.display()
            ),
            Self::SymlinkInPath { path } => {
                write!(f, "output path contains a symlink: {}", path.display())
            }
            Self::UnownedTarget { path } => write!(
                f,
                "refusing to overwrite unowned output file `{}`",
                path.display()
            ),
            Self::ExternalModified { path } => write!(
                f,
                "refusing to overwrite externally modified output file `{}`",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for OutputFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
