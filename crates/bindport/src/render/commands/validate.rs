use super::*;

pub(crate) fn validate_render_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &Registry,
    outputs: Vec<EffectiveOutputConfig>,
    snapshot: &OutputRouteSnapshot,
) -> Result<(), RenderCommandError> {
    let resolver = TemplateResolver::new(
        Some(project_template_dir(cwd, config)),
        global_template_dir(),
    );
    let base_dir = output_base_dir(cwd, config);

    for output in outputs {
        let template = resolver.resolve(&output.template, None)?;
        let render_config = OutputRenderConfig::from(&output);
        let scope = output_file_scope(&base_dir, &render_config)?;
        let delete_route_keys = delete_route_keys(&output, snapshot.routes());
        let render_snapshot = filtered_output_route_snapshot(snapshot, &delete_route_keys);
        let plan = render_output_plan(&render_config, &template.contents, &render_snapshot)?;
        let ownership = registry.output_file_ownership(&output.name, &scope)?;
        let write_ownership = ownership
            .iter()
            .map(|owned| AdapterOutputFileOwnership {
                path: owned.path.clone(),
                content_hash: owned.content_hash.clone(),
            })
            .collect::<Vec<_>>();

        let planned_paths = verify_render_plan_targets(&plan, &base_dir, &write_ownership)?
            .into_iter()
            .map(|file| (file.route_key, file.path))
            .collect::<BTreeMap<_, _>>();
        let superseded = ownership
            .iter()
            .filter(|owned| is_superseded_output_file(owned, &planned_paths))
            .map(removable_output_file)
            .collect::<Vec<_>>();
        if let Some(modified) =
            diff_removable_output_files(&superseded, &base_dir, &render_config.context)?
                .into_iter()
                .find(|file| file.status == AdapterOutputFileRemovalStatus::ExternalModified)
        {
            return Err(RenderCommandError::SupersededOutputModified {
                path: modified.path,
            });
        }
    }

    Ok(())
}
