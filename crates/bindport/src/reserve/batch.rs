use super::*;

pub(super) fn reserve_all_command(
    args: &[String],
) -> Result<Vec<RegistryService>, ReserveCommandError> {
    if args != [String::from("--all")] {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "--all cannot be combined with a service or reservation options",
        )));
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let service_names = configured_service_names_for_all(&config)?;
    let identities = service_names
        .iter()
        .map(|service| {
            let options = RunOptions {
                service: Some(service.clone()),
                ..RunOptions::default()
            };
            resolve_run_identity(&cwd, &[], &options, &config)
        })
        .collect::<Vec<_>>();
    let mut registry = Registry::open_default()?;

    match prune_stale_leases_for_range(&cwd, &config, &mut registry) {
        Ok(summary) if summary.total_leases() > 0 => {
            eprintln!(
                "bindport: pruned {} stale registry entries under configured range pressure",
                summary.total_leases()
            );
        }
        Ok(_) => {}
        Err(error) => print_registry_warning("failed to prune stale registry leases", &error),
    }

    let mut hostname_changes = BTreeMap::new();
    let services = registry.reserve_services_with_preflight(
        &identities,
        |identity, occupied_ports, previous_port| {
            let mut skip_ports = config.skip_ports.clone();
            skip_ports.extend_from_slice(occupied_ports);
            let allocation_hints = AllocationHints {
                preferred_port: previous_port,
                scan_start: identity.port_scan_start(config.port_range),
            };
            let port = allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints)?;
            let service_config = configured_service(&config, identity);
            let templates = resolve_run_templates(&[], &RunOptions::default(), service_config);
            let metadata = resolve_reservation_metadata(identity, port, &templates)?;
            hostname_changes.insert(
                identity.identity_key.clone(),
                (metadata.hostname.clone(), metadata.hostname_changes.clone()),
            );

            Ok::<_, ReserveCommandError>(ReservationCandidate {
                host: String::from("127.0.0.1"),
                port,
                hostname: metadata.hostname,
                route_url: metadata.route_url,
                health_url: metadata.health_url,
            })
        },
        |registry, planned| {
            if has_blocking_auto_outputs(&config)? {
                let pending_routes = planned
                    .iter()
                    .map(|(identity, candidate)| {
                        let metadata = RunMetadata {
                            command: None,
                            hostname: candidate.hostname.clone(),
                            route_url: candidate.route_url.clone(),
                            health_url: candidate.health_url.clone(),
                            hostname_changes: Vec::new(),
                            env: Vec::new(),
                        };
                        let mut route = pending_route_record(
                            identity,
                            candidate.port,
                            &metadata,
                            "reserved",
                            &cwd,
                        );
                        route.state = String::from("reserved");
                        route
                    })
                    .collect();
                preflight_blocking_outputs_for_routes(&cwd, &config, registry, pending_routes)?;
            }
            Ok(())
        },
    );
    let services = match services {
        Ok(services) => services,
        Err(BatchReservationError::Registry(error)) => return Err(error.into()),
        Err(BatchReservationError::Plan(error)) => return Err(error),
    };

    for (identity, (service, newly_reserved)) in identities.iter().zip(&services) {
        if *newly_reserved
            && let Some((hostname, changes)) = hostname_changes.get(&identity.identity_key)
            && service.hostname == *hostname
        {
            print_hostname_change_warnings(&identity.service, changes);
        }
    }

    if services.iter().any(|(_, newly_reserved)| *newly_reserved) {
        let events =
            RouteEventCollector::single(RouteEventSource::CliReserve, RouteEventKind::RouteStarted);
        if let Err(error) = auto_render_outputs_for_events(&cwd, &config, &mut registry, &events) {
            print_auto_render_warning(&events.warning_context(), &error);
        }
    }

    Ok(services.into_iter().map(|(service, _)| service).collect())
}

fn configured_service_names_for_all(
    config: &ResolvedConfig,
) -> Result<Vec<String>, ReserveCommandError> {
    let loaded = config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .ok_or_else(|| {
            ReserveCommandError::InvalidArgument(String::from(
                "reserve --all requires a discovered project config",
            ))
        })?;
    let mut names = loaded
        .config
        .services
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|service| service.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if names.is_empty()
        && let Some(service) = loaded
            .config
            .service
            .as_deref()
            .map(str::trim)
            .filter(|service| !service.is_empty())
    {
        names.push(service.to_string());
    }
    if names.is_empty() {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "project config does not define any named services",
        )));
    }
    let unique = names.iter().collect::<BTreeSet<_>>();
    if unique.len() != names.len() {
        return Err(ReserveCommandError::InvalidArgument(String::from(
            "project config defines duplicate service names",
        )));
    }

    Ok(names)
}
