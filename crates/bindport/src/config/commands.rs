use super::*;

pub(crate) fn run_config_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("explain") if args.len() == 1 => print_config_explain(),
        Some("validate") if args.len() == 1 => print_config_validate(),
        None | Some("--help" | "-h") => {
            print_config_help();
            ExitCode::SUCCESS
        }
        Some("explain") => {
            eprintln!("bindport: config explain does not take arguments");
            eprintln!("usage: bindport config explain");
            ExitCode::FAILURE
        }
        Some("validate") => {
            eprintln!("bindport: config validate does not take arguments");
            eprintln!("usage: bindport config validate");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("bindport: unknown config command `{command}`");
            eprintln!("usage: bindport config explain|validate");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn print_config_validate() -> ExitCode {
    println!("BindPort config validate");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    println!("cwd: {}", cwd.display());

    let config = match resolve_config(&cwd) {
        Ok(config) => config,
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };

    print_config_source_explanation(&config);

    let issues = config
        .loaded
        .as_ref()
        .map(|loaded| loaded.config.validate())
        .unwrap_or_default();

    if issues.is_empty() {
        println!("validation: ok");
        ExitCode::SUCCESS
    } else {
        println!(
            "validation: {} {}",
            issues.len(),
            plural(issues.len(), "error")
        );
        for issue in issues {
            println!("  error: {issue}");
        }
        ExitCode::FAILURE
    }
}

pub(crate) fn print_config_explain() -> ExitCode {
    println!("BindPort config explain");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    println!("cwd: {}", cwd.display());

    let config = match resolve_config(&cwd) {
        Ok(config) => config,
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };

    print_config_source_explanation(&config);
    print_config_field_explanations(&config);

    let explained = explain_run_identity(&cwd, &[], &RunOptions::default(), &config);
    println!("identity:");
    println!(
        "  project: {} ({})",
        explained.identity.project, explained.project_source
    );
    println!(
        "  service: {} ({})",
        explained.identity.service, explained.service_source
    );
    println!("  key: {}", explained.identity.identity_key);

    ExitCode::SUCCESS
}
