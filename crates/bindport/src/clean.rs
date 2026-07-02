use super::*;

#[derive(Debug)]
pub(crate) struct CleanOptions {
    pub(crate) dry_run: bool,
    pub(crate) json: bool,
    pub(crate) stopped: bool,
    pub(crate) stale: bool,
    pub(crate) yes: bool,
    pub(crate) help: bool,
}

impl CleanOptions {
    pub(crate) fn states(&self) -> Vec<CleanState> {
        let mut states = Vec::new();

        if self.stopped {
            states.push(CleanState::Stopped);
        }
        if self.stale {
            states.push(CleanState::Stale);
        }

        states
    }
}

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

pub(crate) fn parse_clean_options(args: &[String]) -> Result<CleanOptions, CleanCommandError> {
    let mut options = CleanOptions {
        dry_run: false,
        json: false,
        stopped: false,
        stale: false,
        yes: false,
        help: false,
    };

    for arg in args {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--json" => options.json = true,
            "--stopped" => options.stopped = true,
            "--stale" => options.stale = true,
            "--yes" | "-y" => options.yes = true,
            "--all" => {
                options.stopped = true;
                options.stale = true;
            }
            "--help" | "-h" => options.help = true,
            unknown => {
                return Err(CleanCommandError::InvalidArgument(format!(
                    "unknown clean option `{unknown}`"
                )));
            }
        }
    }

    if !options.stopped && !options.stale {
        options.stopped = true;
        options.stale = true;
    }

    Ok(options)
}

pub(crate) fn confirm_stale_cleanup(
    options: &CleanOptions,
    preview: CleanSummary,
) -> Result<(), CleanCommandError> {
    if options.dry_run || options.yes || preview.stale_leases == 0 {
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err(CleanCommandError::ConfirmationRequired(String::from(
            "stale cleanup requires confirmation; rerun with --yes",
        )));
    }

    eprint!(
        "Remove {} stale registry entries? [y/N] ",
        preview.stale_leases
    );
    io::stderr().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let answer = answer.trim();

    if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(CleanCommandError::Aborted)
    }
}

pub(crate) fn print_clean_json(
    summary: CleanSummary,
    dry_run: bool,
) -> Result<(), CleanCommandError> {
    let report = serde_json::json!({
        "dry_run": dry_run,
        "leases": summary.total_leases(),
        "runs": summary.runs,
        "states": {
            "stopped": summary.stopped_leases,
            "stale": summary.stale_leases,
        },
    });
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");

    Ok(())
}

pub(crate) fn print_clean_summary(summary: CleanSummary, dry_run: bool) {
    let action = if dry_run { "would clean" } else { "cleaned" };

    println!(
        "{action} {} registry entries (stopped {}, stale {}, runs {})",
        summary.total_leases(),
        summary.stopped_leases,
        summary.stale_leases,
        summary.runs
    );
}

#[derive(Debug)]
pub(crate) enum CleanCommandError {
    InvalidArgument(String),
    Registry(RegistryError),
    Json(serde_json::Error),
    Io(io::Error),
    ConfirmationRequired(String),
    Aborted,
}

impl From<RegistryError> for CleanCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<serde_json::Error> for CleanCommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<io::Error> for CleanCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
