use super::*;

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
            removed.push(RemovedOutputFile {
                route_key: file.route_key.clone(),
                path: file.path.clone(),
                status: OutputFileRemovalStatus::OutsideRoot,
            });
            continue;
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
