use super::*;

pub fn diff_render_plan(
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[OutputFileOwnership],
) -> Result<Vec<DiffedOutputFile>, OutputFileError> {
    let root = output_root(base_dir, &plan.output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, &plan.output);
    let planned_files = render_plan_paths_with_anchor(plan, base_dir, &root, &symlink_anchor)?;
    let owned_hashes = ownership
        .iter()
        .map(|owned| (owned.path.clone(), owned.content_hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut diffed = Vec::with_capacity(plan.files.len());

    for (file, planned) in plan.files.iter().zip(planned_files) {
        let path = planned.path;
        let (status, old_contents) = if path.exists() {
            let Some(expected_hash) = owned_hashes.get(&path) else {
                return Err(OutputFileError::UnownedTarget { path });
            };
            let contents = fs::read_to_string(&path).map_err(|source| OutputFileError::Io {
                path: path.clone(),
                source,
            })?;
            if !content_hash_matches(&contents, expected_hash) {
                return Err(OutputFileError::ExternalModified { path });
            }

            if contents == file.contents {
                (OutputFileDiffStatus::Unchanged, Some(contents))
            } else {
                (OutputFileDiffStatus::Modified, Some(contents))
            }
        } else {
            (OutputFileDiffStatus::Added, None)
        };

        diffed.push(DiffedOutputFile {
            route_key: file.route_key.clone(),
            target: file.target.clone(),
            path,
            status,
            old_contents,
            new_contents: file.contents.clone(),
        });
    }

    Ok(diffed)
}

pub fn diff_removable_output_files(
    files: &[RemovableOutputFile],
    base_dir: &Path,
    output: &OutputContext,
) -> Result<Vec<DiffedRemovalOutputFile>, OutputFileError> {
    let root = output_root(base_dir, output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, output);
    let mut diffed = Vec::with_capacity(files.len());

    for file in files {
        if !file.path.starts_with(&root) {
            diffed.push(DiffedRemovalOutputFile {
                route_key: file.route_key.clone(),
                path: file.path.clone(),
                status: OutputFileRemovalStatus::OutsideRoot,
                old_contents: None,
            });
            continue;
        }
        if file.path.file_name().is_none() {
            return Err(OutputFileError::UnsafeTarget {
                target: file.path.display().to_string(),
            });
        }
        reject_symlink_components(&symlink_anchor, &file.path)?;

        let contents = match fs::read_to_string(&file.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                diffed.push(DiffedRemovalOutputFile {
                    route_key: file.route_key.clone(),
                    path: file.path.clone(),
                    status: OutputFileRemovalStatus::Missing,
                    old_contents: None,
                });
                continue;
            }
            Err(source) => {
                return Err(OutputFileError::Io {
                    path: file.path.clone(),
                    source,
                });
            }
        };

        let (status, old_contents) = if content_hash_matches(&contents, &file.content_hash) {
            (OutputFileRemovalStatus::Removed, Some(contents))
        } else {
            (OutputFileRemovalStatus::ExternalModified, None)
        };
        diffed.push(DiffedRemovalOutputFile {
            route_key: file.route_key.clone(),
            path: file.path.clone(),
            status,
            old_contents,
        });
    }

    Ok(diffed)
}
