use super::*;

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
