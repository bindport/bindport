use super::*;

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
