use super::*;

pub(crate) fn run_template_command(args: &[String]) -> ExitCode {
    match run_template_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(TemplateCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(TemplateCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!(
                "usage: bindport templates list|show|export [--source project|global|built-in] [name]"
            );
            ExitCode::FAILURE
        }
        Err(TemplateCommandError::Template(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_template_command_result(args: &[String]) -> Result<(), TemplateCommandError> {
    let (command, options) = parse_template_command(args)?;

    if command == TemplateCommand::Help {
        print_templates_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let resolver = template_resolver(&cwd)?;

    match command {
        TemplateCommand::List => print_template_list(&resolver, options.source)?,
        TemplateCommand::Show => {
            let name = options
                .name
                .as_deref()
                .expect("parser requires name for show");
            print_template_show(&resolver, name, options.source)?;
        }
        TemplateCommand::Export => {
            let name = options
                .name
                .as_deref()
                .expect("parser requires name for export");
            print_template_export(&resolver, name, options.source)?;
        }
        TemplateCommand::Help => unreachable!("handled before resolver setup"),
    }

    Ok(())
}
