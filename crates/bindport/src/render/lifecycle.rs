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

pub(crate) struct LifecycleRemoval<'a> {
    pub(crate) output: &'a EffectiveOutputConfig,
    pub(crate) scope: &'a OutputFileScope,
    pub(crate) ownership: &'a [bindport_registry::OutputFileOwnership],
    pub(crate) current_route_keys: &'a BTreeSet<String>,
    pub(crate) planned_route_keys: &'a BTreeSet<String>,
    pub(crate) delete_route_keys: &'a BTreeSet<String>,
    pub(crate) base_dir: &'a Path,
    pub(crate) render_config: &'a OutputRenderConfig,
}

pub(crate) fn remove_output_files_for_lifecycle(
    registry: &mut Registry,
    removal: LifecycleRemoval<'_>,
) -> Result<usize, RenderCommandError> {
    let output = removal.output;
    let candidates = lifecycle_removal_candidates(&removal);

    if candidates.is_empty() {
        return Ok(0);
    }

    let removed = remove_owned_output_files(
        &candidates,
        removal.base_dir,
        &removal.render_config.context,
    )?;
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
            scope: removal.scope.clone(),
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

pub(crate) fn lifecycle_removal_candidates(
    removal: &LifecycleRemoval<'_>,
) -> Vec<AdapterRemovableOutputFile> {
    let delete_removed = removal
        .output
        .delete_on
        .contains(&OutputDeleteState::Removed);

    removal
        .ownership
        .iter()
        .filter(|owned| {
            removal.delete_route_keys.contains(&owned.route_key)
                || (delete_removed
                    && !removal.current_route_keys.contains(&owned.route_key)
                    && !removal.planned_route_keys.contains(&owned.route_key))
        })
        .map(|owned| AdapterRemovableOutputFile {
            route_key: owned.route_key.clone(),
            path: owned.path.clone(),
            content_hash: owned.content_hash.clone(),
        })
        .collect()
}
