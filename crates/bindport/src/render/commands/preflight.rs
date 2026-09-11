use super::*;

pub(crate) fn preflight_blocking_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    pending_route: RouteRecord,
) -> Result<(), RenderCommandError> {
    preflight_blocking_outputs_for_routes(cwd, config, registry, vec![pending_route])
}

pub(crate) fn preflight_blocking_outputs_for_routes(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    pending_routes: Vec<RouteRecord>,
) -> Result<(), RenderCommandError> {
    let outputs = configured_outputs(config)?
        .into_iter()
        .filter(|output| output.auto_render && output.on_failure == OutputFailurePolicy::Block)
        .collect::<Vec<_>>();

    if outputs.is_empty() {
        return Ok(());
    }

    let mut snapshot = output_route_snapshot(registry.status_snapshot()?);
    for pending_route in pending_routes {
        snapshot.retain_routes(|route| route.key != pending_route.key);
        snapshot.push_route(pending_route);
    }

    validate_render_outputs(cwd, config, registry, outputs, &snapshot)
}
