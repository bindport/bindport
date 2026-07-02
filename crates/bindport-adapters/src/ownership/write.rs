use super::*;

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
