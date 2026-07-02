use super::*;

pub(crate) fn run_doctor_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        None => print_doctor(),
        Some("--help" | "-h") => {
            print_doctor_help();
            ExitCode::SUCCESS
        }
        Some("outputs") if args.len() == 1 => print_doctor_outputs(),
        Some("outputs") => {
            eprintln!("bindport: doctor outputs does not take arguments");
            eprintln!("usage: bindport doctor [outputs]");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("bindport: unknown doctor command `{command}`");
            eprintln!("usage: bindport doctor [outputs]");
            ExitCode::FAILURE
        }
    }
}
pub(crate) fn print_doctor() -> ExitCode {
    println!("BindPort bootstrap doctor");

    let mut registry = print_doctor_registry_path();

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = match resolve_config(&cwd) {
        Ok(config) => {
            print_config_diagnostics(&config);
            config
        }
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    let identity = resolve_run_identity(&cwd, &[], &RunOptions::default(), &config);

    print_identity_diagnostics(&identity);
    print_git_diagnostics(&cwd);
    let allocation_ok = print_allocation_diagnostics(&config, &identity, registry.as_mut());

    println!("first proxy adapter: {}", AdapterKind::Traefik.as_str());
    if allocation_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
