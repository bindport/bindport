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
pub fn write_render_plan(
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[OutputFileOwnership],
) -> Result<Vec<WrittenOutputFile>, OutputFileError> {
    let root = output_root(base_dir, &plan.output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, &plan.output);
    let planned_files = render_plan_paths_with_anchor(plan, base_dir, &root, &symlink_anchor)?;
    let owned_hashes = ownership
        .iter()
        .map(|owned| (owned.path.clone(), owned.content_hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut written = Vec::with_capacity(plan.files.len());

    for (file, planned) in plan.files.iter().zip(planned_files) {
        let path = planned.path;
        verify_existing_target(&path, &owned_hashes)?;
        atomic_write(&symlink_anchor, &path, &file.contents)?;

        written.push(WrittenOutputFile {
            route_key: file.route_key.clone(),
            path,
            content_hash: content_hash(&file.contents),
            bytes: file.contents.len(),
        });
    }

    Ok(written)
}

pub fn verify_render_plan_targets(
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[OutputFileOwnership],
) -> Result<Vec<PlannedOutputFile>, OutputFileError> {
    let root = output_root(base_dir, &plan.output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, &plan.output);
    let planned_files = render_plan_paths_with_anchor(plan, base_dir, &root, &symlink_anchor)?;
    let owned_hashes = ownership
        .iter()
        .map(|owned| (owned.path.clone(), owned.content_hash.clone()))
        .collect::<BTreeMap<_, _>>();

    for planned in &planned_files {
        verify_existing_target(&planned.path, &owned_hashes)?;
    }

    Ok(planned_files)
}

pub fn remove_owned_output_files(
    files: &[RemovableOutputFile],
    base_dir: &Path,
    output: &OutputContext,
) -> Result<Vec<RemovedOutputFile>, OutputFileError> {
    let root = output_root(base_dir, output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, output);
    let mut removed = Vec::with_capacity(files.len());

    for file in files {
        if !file.path.starts_with(&root) {
            return Err(OutputFileError::TargetEscapesRoot {
                target: file.path.display().to_string(),
                root: root.clone(),
            });
        }
        if file.path.file_name().is_none() {
            return Err(OutputFileError::UnsafeTarget {
                target: file.path.display().to_string(),
            });
        }
        reject_symlink_components(&symlink_anchor, &file.path)?;

        let status = match owned_file_state(&file.path, &file.content_hash)? {
            OwnedFileState::Missing => OutputFileRemovalStatus::Missing,
            OwnedFileState::ExternalModified => OutputFileRemovalStatus::ExternalModified,
            OwnedFileState::Matches => match fs::remove_file(&file.path) {
                Ok(()) => OutputFileRemovalStatus::Removed,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    OutputFileRemovalStatus::Missing
                }
                Err(source) => {
                    return Err(OutputFileError::Io {
                        path: file.path.clone(),
                        source,
                    });
                }
            },
        };
        removed.push(RemovedOutputFile {
            route_key: file.route_key.clone(),
            path: file.path.clone(),
            status,
        });
    }

    Ok(removed)
}

pub fn render_plan_paths(
    plan: &RenderPlan,
    base_dir: &Path,
) -> Result<Vec<PlannedOutputFile>, OutputFileError> {
    let root = output_root(base_dir, &plan.output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, &plan.output);

    render_plan_paths_with_anchor(plan, base_dir, &root, &symlink_anchor)
}

pub(crate) fn render_plan_paths_with_anchor(
    plan: &RenderPlan,
    base_dir: &Path,
    root: &Path,
    symlink_anchor: &Path,
) -> Result<Vec<PlannedOutputFile>, OutputFileError> {
    let mut planned_files = Vec::with_capacity(plan.files.len());

    for file in &plan.files {
        let path = output_file_path(base_dir, root, &plan.output, &file.target)?;
        reject_symlink_components(symlink_anchor, &path)?;
        planned_files.push(PlannedOutputFile {
            route_key: file.route_key.clone(),
            target: file.target.clone(),
            path,
        });
    }

    Ok(planned_files)
}

pub(crate) fn output_root(
    base_dir: &Path,
    output: &OutputContext,
) -> Result<PathBuf, OutputFileError> {
    if let Some(root) = output.root.as_deref() {
        return clean_root_path(base_dir, root);
    }

    let prefix = output.target.split("{{").next().unwrap_or_default();
    let directory_prefix = literal_directory_prefix(prefix);

    safe_join(base_dir, directory_prefix).map_err(|_| OutputFileError::UnsafeRoot {
        root: directory_prefix.display().to_string(),
    })
}

pub(crate) fn literal_directory_prefix(prefix: &str) -> &Path {
    let trimmed = prefix.trim_end_matches(['/', '\\']);

    if prefix.ends_with(['/', '\\']) {
        return Path::new(trimmed);
    }

    Path::new(trimmed).parent().unwrap_or_else(|| Path::new(""))
}

pub(crate) fn clean_root_path(base_dir: &Path, root: &str) -> Result<PathBuf, OutputFileError> {
    let root_path = Path::new(root);

    if root_path.is_absolute() {
        Err(OutputFileError::UnsafeRoot {
            root: root.to_string(),
        })
    } else {
        safe_join(base_dir, root_path).map_err(|_| OutputFileError::UnsafeRoot {
            root: root.to_string(),
        })
    }
}

pub(crate) fn output_file_path(
    base_dir: &Path,
    root: &Path,
    output: &OutputContext,
    target: &str,
) -> Result<PathBuf, OutputFileError> {
    let target_path = Path::new(target);
    let path = if output.root.is_some() {
        safe_join(root, target_path)
    } else {
        safe_join(base_dir, target_path)
    }
    .map_err(|_| OutputFileError::UnsafeTarget {
        target: target.to_string(),
    })?;

    if !path.starts_with(root) {
        return Err(OutputFileError::TargetEscapesRoot {
            target: target.to_string(),
            root: root.to_path_buf(),
        });
    }
    if path.file_name().is_none() {
        return Err(OutputFileError::UnsafeTarget {
            target: target.to_string(),
        });
    }

    Ok(path)
}

pub(crate) fn symlink_check_anchor(
    base_dir: &Path,
    root: &Path,
    output: &OutputContext,
) -> PathBuf {
    let _ = (root, output);
    base_dir.to_path_buf()
}

pub(crate) fn safe_join(base: &Path, relative: &Path) -> Result<PathBuf, ()> {
    if relative.is_absolute() {
        return Err(());
    }

    let mut path = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => path.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(()),
        }
    }

    Ok(path)
}

pub(crate) fn reject_symlink_components(anchor: &Path, path: &Path) -> Result<(), OutputFileError> {
    let relative = path.strip_prefix(anchor).unwrap_or(path);
    let mut current = anchor.to_path_buf();

    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(OutputFileError::SymlinkInPath { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(OutputFileError::Io {
                    path: current,
                    source,
                });
            }
        }
    }

    Ok(())
}

pub(crate) fn verify_existing_target(
    path: &Path,
    owned_hashes: &BTreeMap<PathBuf, String>,
) -> Result<(), OutputFileError> {
    if !path.exists() {
        return Ok(());
    }
    let Some(expected_hash) = owned_hashes.get(path) else {
        return Err(OutputFileError::UnownedTarget {
            path: path.to_path_buf(),
        });
    };
    let contents = fs::read_to_string(path).map_err(|source| OutputFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    if !content_hash_matches(&contents, expected_hash) {
        return Err(OutputFileError::ExternalModified {
            path: path.to_path_buf(),
        });
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnedFileState {
    Matches,
    Missing,
    ExternalModified,
}

pub(crate) fn owned_file_state(
    path: &Path,
    expected_hash: &str,
) -> Result<OwnedFileState, OutputFileError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(OwnedFileState::Missing);
        }
        Err(source) => {
            return Err(OutputFileError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if !content_hash_matches(&contents, expected_hash) {
        return Ok(OwnedFileState::ExternalModified);
    }

    Ok(OwnedFileState::Matches)
}

pub(crate) fn atomic_write(
    anchor: &Path,
    path: &Path,
    contents: &str,
) -> Result<(), OutputFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| OutputFileError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        reject_symlink_components(anchor, parent)?;
    }

    let temp_path = temp_sibling(path);
    if let Err(source) = write_output_file(&temp_path, contents) {
        let _ = fs::remove_file(&temp_path);
        return Err(OutputFileError::Io {
            path: temp_path,
            source,
        });
    }
    fs::rename(&temp_path, path).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        OutputFileError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

pub(crate) fn write_output_file(path: &Path, contents: &str) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())
}

pub(crate) fn temp_sibling(path: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");

    path.with_file_name(format!(
        ".{filename}.bindport-tmp-{}-{now}",
        std::process::id()
    ))
}
