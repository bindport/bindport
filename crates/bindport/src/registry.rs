use super::*;

#[derive(Debug)]
pub(crate) enum RegistryCommandError {
    InvalidArgument(String),
    Registry(RegistryError),
    Serialize(serde_json::Error),
}

impl From<RegistryError> for RegistryCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<serde_json::Error> for RegistryCommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error)
    }
}

pub(crate) fn run_registry_command(args: &[String]) -> ExitCode {
    match run_registry_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(RegistryCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport registry export");
            ExitCode::FAILURE
        }
        Err(RegistryCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(RegistryCommandError::Serialize(error)) => {
            eprintln!("bindport: failed to serialize registry export JSON: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_registry_command_result(args: &[String]) -> Result<(), RegistryCommandError> {
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h") => {
            print_registry_help();
            Ok(())
        }
        Some("export") => run_registry_export(&args[1..]),
        Some(command) => Err(RegistryCommandError::InvalidArgument(format!(
            "unknown registry command `{command}`"
        ))),
    }
}

fn run_registry_export(args: &[String]) -> Result<(), RegistryCommandError> {
    if let Some(arg) = args.first() {
        if matches!(arg.as_str(), "--help" | "-h") && args.len() == 1 {
            print_registry_help();
            return Ok(());
        }

        return Err(RegistryCommandError::InvalidArgument(format!(
            "unexpected registry export argument `{arg}`"
        )));
    }

    let registry = Registry::open_default()?;
    let snapshot = registry.export_snapshot()?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);

    Ok(())
}
