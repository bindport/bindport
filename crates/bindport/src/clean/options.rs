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
