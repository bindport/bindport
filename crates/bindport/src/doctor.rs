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

pub(crate) fn print_doctor_outputs() -> ExitCode {
    println!("BindPort output doctor");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = match resolve_config(&cwd) {
        Ok(config) => {
            print_doctor_output_config(&config);
            config
        }
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    let outputs = match configured_outputs(&config) {
        Ok(outputs) => outputs,
        Err(error) => {
            println!("outputs: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    print_doctor_hooks(&cwd, &config);

    if outputs.is_empty() {
        println!("outputs: none configured");
        return ExitCode::SUCCESS;
    }

    let mut registry = match Registry::open_default() {
        Ok(registry) => registry,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let snapshot = match registry.status_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let routes = route_records(snapshot.services);
    let base_dir = output_base_dir(&cwd, &config);
    let resolver = TemplateResolver::new(
        Some(project_template_dir(&cwd, &config)),
        global_template_dir(),
    );
    let mut ok = true;

    println!("routes: {}", routes.len());
    println!("base dir: {}", base_dir.display());

    for output in &outputs {
        if !print_doctor_output(output, &resolver, &routes, &base_dir) {
            ok = false;
        }
    }

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
pub(crate) fn print_doctor_output_config(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            print_config_local_override(loaded);
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }
}

pub(crate) fn print_doctor_output(
    output: &EffectiveOutputConfig,
    resolver: &TemplateResolver,
    routes: &[RouteRecord],
    base_dir: &Path,
) -> bool {
    println!("output {}:", output.name);
    println!("  target: {}", output.target);
    println!(
        "  root: {}",
        output.root.as_deref().unwrap_or("<derived from target>")
    );
    println!("  auto-render: {}", output.auto_render);

    let template = match resolver.resolve(&output.template, None) {
        Ok(template) => {
            println!("  template: {} ({})", output.template, template.source);
            if let Some(path) = template.path.as_ref() {
                println!("  template path: {}", path.display());
            }
            if template.wildcard_matches.len() > 1 {
                println!(
                    "  template warning: multiple wildcard matches; using {}",
                    template
                        .wildcard_matches
                        .first()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| String::from("<unknown>"))
                );
            }
            template
        }
        Err(error) => {
            println!("  template: {} (invalid: {error})", output.template);
            return false;
        }
    };

    let render_config = OutputRenderConfig::from(output);
    let plan = match render_output_routes(&render_config, &template.contents, routes) {
        Ok(plan) => plan,
        Err(error) => {
            println!("  plan: invalid ({error})");
            return false;
        }
    };
    let planned_files = match render_plan_paths(&plan, base_dir) {
        Ok(planned_files) => planned_files,
        Err(error) => {
            println!("  paths: invalid ({error})");
            return false;
        }
    };

    println!("  planned files: {}", planned_files.len());
    for file in planned_files.iter().take(5) {
        println!("    {} -> {}", file.route_key, file.path.display());
    }
    if planned_files.len() > 5 {
        println!("    ... {} more", planned_files.len() - 5);
    }

    true
}

pub(crate) fn print_doctor_registry_path() -> Option<Registry> {
    match default_registry_path() {
        Ok(path) => match Registry::open(&path) {
            Ok(registry) => {
                println!("registry: {} (ok)", path.display());
                Some(registry)
            }
            Err(error) => {
                println!("registry: {} (unavailable: {error})", path.display());
                None
            }
        },
        Err(error) => {
            println!("registry: unavailable ({error})");
            None
        }
    }
}

pub(crate) fn print_identity_diagnostics(identity: &ServiceIdentity) {
    println!(
        "effective identity: project={} service={}",
        identity.project, identity.service
    );
    println!("identity key: {}", identity.identity_key);
}

pub(crate) fn print_git_diagnostics(cwd: &Path) {
    match detect_git_identity(cwd) {
        Some(git) => {
            println!("git worktree: {}", git.worktree_path.display());
            println!("git branch: {}", git.branch);
            println!("git branch label: {}", git.branch_label);
            println!("git commit: {}", git.commit);
        }
        None => println!("git worktree: none"),
    }
}

pub(crate) fn print_config_diagnostics(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            print_config_local_override(loaded);
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }

    if let Some(loaded) = config.loaded.as_ref()
        && !loaded.unknown_keys.is_empty()
    {
        println!(
            "config warning: ignored unknown top-level keys: {}",
            loaded.unknown_keys.join(", ")
        );
        println!("config applied keys: {}", APPLIED_CONFIG_KEYS.join(", "));
    }

    println!(
        "effective port range: {}-{}",
        config.port_range.start, config.port_range.end
    );
    println!("skip ports: {}", config.skip_ports.len());
}

pub(crate) fn print_allocation_diagnostics(
    config: &ResolvedConfig,
    identity: &ServiceIdentity,
    registry: Option<&mut Registry>,
) -> bool {
    let mut active_ports = Vec::new();
    let mut previous_port = None;
    let registry_available = registry.is_some();
    let mut active_ports_available = registry_available;
    let mut previous_port_available = registry_available;

    match registry {
        Some(registry) => {
            match registry.active_ports() {
                Ok(ports) => active_ports = ports,
                Err(error) => {
                    println!("registry active ports in range: unavailable ({error})");
                    active_ports_available = false;
                }
            }

            match registry.previous_identity_port(&identity.identity_key) {
                Ok(port) => previous_port = port,
                Err(error) => {
                    println!("previous identity port: unavailable ({error})");
                    previous_port_available = false;
                }
            }
        }
        None => {
            println!("registry active ports in range: unavailable");
            active_ports_available = false;
            previous_port_available = false;
        }
    }

    if active_ports_available {
        let active_in_range = ports_in_range(&active_ports, config.port_range);
        println!(
            "registry active ports in range: {}",
            format_limited_ports(&active_in_range)
        );
    }

    if previous_port_available {
        print_previous_port_diagnostics(previous_port, config, &active_ports);
    }
    let listener_conflicts = listener_conflicts(config.port_range, &active_ports);
    println!(
        "known registry listener conflicts in range: {}",
        format_limited_ports(&listener_conflicts.known_registry)
    );
    println!(
        "unknown os listener conflicts in range: {}",
        format_listener_conflict_scan(&listener_conflicts)
    );

    let scan_start = identity.port_scan_start(config.port_range);
    match scan_start {
        Some(port) => println!("allocation scan start: {port}"),
        None => println!("allocation scan start: unavailable"),
    }

    let mut skip_ports = config.skip_ports.clone();
    skip_ports.extend(active_ports);
    let allocation_hints = AllocationHints {
        preferred_port: previous_port,
        scan_start,
    };

    match allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints) {
        Ok(port) => {
            let source = if Some(port) == previous_port {
                "sticky"
            } else {
                "scan"
            };
            println!("next candidate port: {port} ({source})");
            true
        }
        Err(error) => {
            println!("next candidate port: unavailable ({error})");
            false
        }
    }
}

pub(crate) fn print_previous_port_diagnostics(
    previous_port: Option<u16>,
    config: &ResolvedConfig,
    active_ports: &[u16],
) {
    let Some(port) = previous_port else {
        println!("previous identity port: none");
        return;
    };

    let status = if !config.port_range.contains(port) {
        "outside range"
    } else if config.skip_ports.contains(&port) {
        "configured skip"
    } else if active_ports.contains(&port) {
        "active registry conflict"
    } else if is_port_available(port) {
        "free"
    } else {
        "os listener conflict"
    };

    println!("previous identity port: {port} ({status})");
}

pub(crate) struct ListenerConflictScan {
    pub(crate) known_registry: Vec<u16>,
    pub(crate) unknown: Vec<u16>,
    pub(crate) scanned_ports: u32,
    pub(crate) total_ports: u32,
}

pub(crate) fn listener_conflicts(
    range: PortRange,
    known_registry_ports: &[u16],
) -> ListenerConflictScan {
    let total_ports = range.len();
    let scanned_ports = total_ports.min(DOCTOR_MAX_LISTENER_PROBES);
    let known_registry_ports = ports_in_range(known_registry_ports, range);
    let mut known_registry = Vec::new();
    let mut unknown = Vec::new();

    for offset in 0..scanned_ports {
        let port = range.start as u32 + offset;
        let port = u16::try_from(port).expect("port remains within configured range");

        if is_port_available(port) {
            continue;
        }

        if known_registry_ports.contains(&port) {
            known_registry.push(port);
        } else {
            unknown.push(port);
        }
    }

    ListenerConflictScan {
        known_registry,
        unknown,
        scanned_ports,
        total_ports,
    }
}

pub(crate) fn format_listener_conflict_scan(scan: &ListenerConflictScan) -> String {
    let mut summary = format_limited_ports(&scan.unknown);

    if scan.scanned_ports < scan.total_ports {
        summary.push_str(&format!(
            " (scanned first {} of {} ports)",
            scan.scanned_ports, scan.total_ports
        ));
    }

    summary
}

pub(crate) fn format_limited_ports(ports: &[u16]) -> String {
    if ports.is_empty() {
        return String::from("none");
    }

    let mut summary = ports
        .iter()
        .take(DOCTOR_PORT_DISPLAY_LIMIT)
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    if ports.len() > DOCTOR_PORT_DISPLAY_LIMIT {
        summary.push_str(&format!(
            " (+{} more)",
            ports.len() - DOCTOR_PORT_DISPLAY_LIMIT
        ));
    }

    summary
}
