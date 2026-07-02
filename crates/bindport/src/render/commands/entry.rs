use super::*;

pub(crate) fn run_render_command(args: &[String]) -> ExitCode {
    match run_render_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(RenderCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(RenderCommandError::OutputConfig(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport render [output] [--all] [--dry-run] [--repair]");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(RenderCommandError::Template(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::Render(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(RenderCommandError::File(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_render_command_result(args: &[String]) -> Result<(), RenderCommandError> {
    let (command, options) = parse_render_command(args)?;

    if command == RenderCommand::Help {
        print_render_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let outputs = configured_outputs(&config)?;
    let outputs = selected_outputs(outputs, options.output.as_deref())?;

    if outputs.is_empty() {
        println!("No enabled outputs configured.");
        return Ok(());
    }

    let mut registry = Registry::open_default()?;
    let mode = if options.repair {
        RenderMode::Repair
    } else {
        RenderMode::Normal
    };
    let events = RouteEventCollector::single(
        RouteEventSource::ManualRender,
        RouteEventKind::RenderRequested,
    );
    render_outputs_for_events(
        &cwd,
        &config,
        &mut registry,
        RenderInvocation {
            outputs,
            dry_run: options.dry_run,
            mode,
            report: RenderReport::Print,
            events: &events,
        },
    )?;

    Ok(())
}
