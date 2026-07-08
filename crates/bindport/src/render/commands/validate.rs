use super::*;

pub(crate) fn preflight_blocking_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    pending_route: RouteRecord,
) -> Result<(), RenderCommandError> {
    let outputs = configured_outputs(config)?
        .into_iter()
        .filter(|output| output.auto_render && output.on_failure == OutputFailurePolicy::Block)
        .collect::<Vec<_>>();

    if outputs.is_empty() {
        return Ok(());
    }

    let mut snapshot = output_route_snapshot(registry.status_snapshot()?);
    snapshot.retain_routes(|route| route.key != pending_route.key);
    snapshot.push_route(pending_route);

    validate_render_outputs(cwd, config, registry, outputs, &snapshot)
}

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

        verify_render_plan_targets(&plan, &base_dir, &write_ownership)?;
    }

    Ok(())
}
