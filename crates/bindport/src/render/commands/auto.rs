use super::*;

pub(crate) fn auto_render_outputs_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    events: &RouteEventCollector,
) -> Result<usize, RenderCommandError> {
    if events.is_empty() {
        return Ok(0);
    }

    let log = DiagnosticLog::from_env();
    log.debug(format_args!(
        "auto-render requested by {}",
        events.diagnostic_summary()
    ));

    let mut outputs = Vec::new();
    for output in configured_outputs(config)?
        .into_iter()
        .filter(|output| output.auto_render)
    {
        let delay = registry.reserve_auto_render(&output.name, output.debounce_ms)?;
        if !delay.is_zero() {
            log.debug(format_args!(
                "auto-render debounce output={} delay_ms={}",
                output.name,
                delay.as_millis()
            ));
            std::thread::sleep(delay);
        }
        log.debug(format_args!("auto-render queued output={}", output.name));
        outputs.push(output);
    }
    if outputs.is_empty() {
        log.debug(format_args!(
            "auto-render skipped: no enabled auto-render outputs"
        ));
    }

    render_outputs_for_events(
        cwd,
        config,
        registry,
        RenderInvocation {
            outputs,
            dry_run: false,
            mode: RenderMode::Normal,
            report: RenderReport::Quiet,
            log,
            events,
        },
    )
}

pub(crate) fn collect_stale_reconcile_event(
    registry: &mut Registry,
    events: &mut RouteEventCollector,
) -> Result<(), RegistryError> {
    if registry.reconcile_stale_active_leases()? > 0 {
        events.record(
            RouteEventSource::StaleReconcile,
            RouteEventKind::RoutesMarkedStale,
        );
    }

    Ok(())
}

pub(crate) fn has_blocking_auto_outputs(
    config: &ResolvedConfig,
) -> Result<bool, RenderCommandError> {
    Ok(configured_outputs(config)?
        .into_iter()
        .any(|output| output.auto_render && output.on_failure == OutputFailurePolicy::Block))
}
