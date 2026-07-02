use super::*;

pub(crate) fn prune_stale_leases_for_range(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
) -> Result<CleanSummary, RegistryError> {
    let limit = stale_lease_prune_limit(config.port_range, &config.skip_ports);
    let summary = registry.prune_oldest_stale_leases(
        config.port_range.start,
        config.port_range.end,
        limit,
        false,
    )?;

    if summary.total_leases() > 0 {
        let events = RouteEventCollector::single(
            RouteEventSource::StaleReconcile,
            RouteEventKind::RoutesRemoved,
        );
        if let Err(error) = auto_render_outputs_for_events(cwd, config, registry, &events) {
            print_auto_render_warning(&events.warning_context(), &error);
        }
    }

    Ok(summary)
}

pub(crate) fn stale_lease_prune_limit(range: PortRange, skip_ports: &[u16]) -> usize {
    let skipped_in_range = ports_in_range(skip_ports, range).len() as u32;
    let usable_ports = range.len().saturating_sub(skipped_in_range);

    (usable_ports / 2) as usize
}
