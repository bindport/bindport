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
    pub(crate) planned_paths: &'a BTreeMap<String, PathBuf>,
    pub(crate) delete_route_keys: &'a BTreeSet<String>,
    pub(crate) base_dir: &'a Path,
    pub(crate) render_config: &'a OutputRenderConfig,
}

pub(crate) struct LifecycleRemovalSummary {
    pub(crate) removed: usize,
    pub(crate) preserved: Vec<AdapterRemovedOutputFile>,
}

pub(crate) fn remove_output_files_for_lifecycle(
    registry: &mut Registry,
    removal: LifecycleRemoval<'_>,
) -> Result<LifecycleRemovalSummary, RenderCommandError> {
    let output = removal.output;
    let candidates = lifecycle_removal_candidates(&removal);
    let mut summary = LifecycleRemovalSummary {
        removed: 0,
        preserved: Vec::new(),
    };

    if candidates.is_empty() {
        return Ok(summary);
    }

    let removed = remove_owned_output_files(
        &candidates,
        removal.base_dir,
        &removal.render_config.context,
    )?;

    for file in removed {
        let expected_hash = candidates
            .iter()
            .find(|candidate| candidate.route_key == file.route_key && candidate.path == file.path)
            .map(|candidate| candidate.content_hash.clone());
        let (status, reason, content_hash) = match file.status {
            AdapterOutputFileRemovalStatus::Removed => {
                summary.removed += 1;
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
            AdapterOutputFileRemovalStatus::ExternalModified => {
                if removal.planned_paths.contains_key(&file.route_key) {
                    summary.preserved.push(file.clone());
                }
                (
                    OutputFileStatus::Error,
                    Some(String::from("external_modified")),
                    expected_hash,
                )
            }
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

    Ok(summary)
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
                || is_superseded_output_file(owned, removal.planned_paths)
                || (delete_removed
                    && !removal.current_route_keys.contains(&owned.route_key)
                    && !removal.planned_paths.contains_key(&owned.route_key))
        })
        .map(removable_output_file)
        .collect()
}

pub(crate) fn is_superseded_output_file(
    owned: &bindport_registry::OutputFileOwnership,
    planned_paths: &BTreeMap<String, PathBuf>,
) -> bool {
    planned_paths
        .get(&owned.route_key)
        .is_some_and(|path| path != &owned.path)
}

pub(crate) fn removable_output_file(
    owned: &bindport_registry::OutputFileOwnership,
) -> AdapterRemovableOutputFile {
    AdapterRemovableOutputFile {
        route_key: owned.route_key.clone(),
        path: owned.path.clone(),
        content_hash: owned.content_hash.clone(),
    }
}

pub(crate) fn lifecycle_diff_candidates(
    removal: &LifecycleRemoval<'_>,
) -> Vec<AdapterRemovableOutputFile> {
    let planned = removal.planned_paths.values().collect::<BTreeSet<_>>();

    lifecycle_removal_candidates(removal)
        .into_iter()
        .filter(|candidate| !planned.contains(&candidate.path))
        .collect()
}
