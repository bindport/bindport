use super::*;

pub(crate) fn clean_registry(args: &[String]) -> ExitCode {
    match clean_registry_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CleanCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport clean [--dry-run] [--stopped] [--stale] [--json] [--yes]");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Json(error)) => {
            eprintln!("bindport: failed to serialize clean JSON: {error}");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Io(error)) => {
            eprintln!("bindport: failed to read cleanup confirmation: {error}");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::ConfirmationRequired(message)) => {
            eprintln!("bindport: {message}");
            ExitCode::FAILURE
        }
        Err(CleanCommandError::Aborted) => {
            eprintln!("bindport: cleanup aborted");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn clean_registry_result(args: &[String]) -> Result<(), CleanCommandError> {
    let options = parse_clean_options(args)?;

    if options.help {
        print_clean_help();
        return Ok(());
    }

    let states = options.states();
    let mut registry = Registry::open_default()?;
    let preview = registry.clean_leases(&states, true)?;
    confirm_stale_cleanup(&options, preview)?;
    let summary = if options.dry_run {
        preview
    } else {
        registry.clean_leases(&states, false)?
    };

    if !options.dry_run && summary.total_leases() > 0 {
        let events =
            RouteEventCollector::single(RouteEventSource::CliClean, RouteEventKind::RoutesRemoved);
        if let Err(error) = auto_render_outputs_for_current_dir(&mut registry, &events) {
            print_auto_render_warning(&events.warning_context(), &error);
        }
    }

    if options.json {
        print_clean_json(summary, options.dry_run)?;
    } else {
        print_clean_summary(summary, options.dry_run);
    }

    Ok(())
}

pub(crate) fn auto_render_outputs_for_current_dir(
    registry: &mut Registry,
    events: &RouteEventCollector,
) -> Result<usize, RenderCommandError> {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;

    auto_render_outputs_for_events(&cwd, &config, registry, events)
}
