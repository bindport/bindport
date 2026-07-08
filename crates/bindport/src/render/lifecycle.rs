use super::*;

pub(crate) fn delete_route_keys(
    output: &EffectiveOutputConfig,
    routes: &[RouteRecord],
) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| {
            route_delete_state(route).is_some_and(|state| output.delete_on.contains(&state))
        })
        .map(|route| route.key.clone())
        .collect()
}

pub(crate) fn route_delete_state(route: &RouteRecord) -> Option<OutputDeleteState> {
    match route.state.as_str() {
        "stopped" => Some(OutputDeleteState::Stopped),
        "stale" => Some(OutputDeleteState::Stale),
        _ => None,
    }
}

pub(crate) fn remove_output_files_for_lifecycle(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    ownership: &[bindport_registry::OutputFileOwnership],
    current_route_keys: &BTreeSet<String>,
    delete_route_keys: &BTreeSet<String>,
    base_dir: &Path,
    render_config: &OutputRenderConfig,
) -> Result<usize, RenderCommandError> {
    let delete_removed = output.delete_on.contains(&OutputDeleteState::Removed);
    let candidates = ownership
        .iter()
        .filter(|owned| {
            delete_route_keys.contains(&owned.route_key)
                || (delete_removed && !current_route_keys.contains(&owned.route_key))
        })
        .map(|owned| AdapterRemovableOutputFile {
            route_key: owned.route_key.clone(),
            path: owned.path.clone(),
            content_hash: owned.content_hash.clone(),
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Ok(0);
    }

    let removed = remove_owned_output_files(&candidates, base_dir, &render_config.context)?;
    let mut removed_count = 0;

    for file in removed {
        let expected_hash = candidates
            .iter()
            .find(|candidate| candidate.route_key == file.route_key && candidate.path == file.path)
            .map(|candidate| candidate.content_hash.clone());
        let (status, reason, content_hash) = match file.status {
            AdapterOutputFileRemovalStatus::Removed => {
                removed_count += 1;
                (OutputFileStatus::Removed, None, None)
            }
            AdapterOutputFileRemovalStatus::Missing => (
                OutputFileStatus::Removed,
                Some(String::from("missing")),
                None,
            ),
            AdapterOutputFileRemovalStatus::OutsideRoot => (
                OutputFileStatus::Removed,
                Some(String::from("outside_output_root")),
                None,
            ),
            AdapterOutputFileRemovalStatus::ExternalModified => (
                OutputFileStatus::Error,
                Some(String::from("external_modified")),
                expected_hash,
            ),
        };

        registry.record_output_file(&OutputFileRecord {
            output_name: output.name.clone(),
            route_key: file.route_key,
            rendered_path: file.path,
            status,
            reason,
            content_hash,
            template_hash: None,
            lease_id: None,
            run_id: None,
        })?;
    }

    Ok(removed_count)
}
