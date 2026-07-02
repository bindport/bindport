use super::*;

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
