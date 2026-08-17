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
    let (hostname_warnings, hostname_errors) = configured_hostname_diagnostics(&cwd, &config);

    let error_count = issues.len() + hostname_errors.len();
    if error_count == 0 {
        if hostname_warnings.is_empty() {
            println!("validation: ok");
        } else {
            println!(
                "validation: ok with {} {}",
                hostname_warnings.len(),
                plural(hostname_warnings.len(), "warning")
            );
        }
        for warning in hostname_warnings {
            println!("  warning: {warning}");
        }
        ExitCode::SUCCESS
    } else {
        println!("validation: {error_count} {}", plural(error_count, "error"));
        for issue in issues {
            println!("  error: {issue}");
        }
        for error in hostname_errors {
            println!("  error: {error}");
        }
        for warning in hostname_warnings {
            println!("  warning: {warning}");
        }
        ExitCode::FAILURE
    }
}

fn configured_hostname_diagnostics(
    cwd: &Path,
    config: &ResolvedConfig,
) -> (Vec<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let Some(services) = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.services.as_deref())
    else {
        return (warnings, errors);
    };

    for (index, service) in services.iter().enumerate() {
        let Some(name) = service
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let Some(hostname) = service.hostname.as_ref() else {
            continue;
        };
        let config_project = config
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.config.project.as_deref());
        let identity = resolve_identity_in_scope(
            IdentitySources {
                cwd,
                command: &[],
                cli_project: None,
                cli_service: Some(name),
                env_project: None,
                env_service: None,
                config_project,
                config_service: None,
            },
            project_identity_scope(cwd, config),
        );
        let templates = RunTemplates {
            hostname: Some(hostname.clone()),
            ..RunTemplates::default()
        };
        let field = format!("services[{index}].hostname");

        match resolve_run_route_metadata(&identity, config.port_range.start, &templates) {
            Ok(metadata) => {
                for change in metadata.hostname_changes {
                    warnings.push(format!(
                        "{field}: service `{name}` resolved label `{}` is shortened to `{}` to satisfy the DNS 63-byte label limit",
                        change.original, change.replacement
                    ));
                }
            }
            Err(error) => errors.push(format!("{field}: service `{name}` {error}")),
        }
    }

    (warnings, errors)
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
