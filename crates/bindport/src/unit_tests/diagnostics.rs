// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn config_and_doctor_formatting_helpers_are_stable() {
    assert_eq!(optional_config_value(Some(" value ")), "value");
    assert_eq!(optional_config_value(Some("  ")), "<unset>");
    assert_eq!(optional_config_value(None), "<unset>");
    assert_eq!(configured_value(true), "configured");
    assert_eq!(configured_value(false), "<unset>");
    assert_eq!(list_config_value(Some(1), "entry"), "1 entry");
    assert_eq!(list_config_value(Some(2), "entry"), "2 entries");
    assert_eq!(list_config_value(Some(3), "port"), "3 ports");
    assert_eq!(list_config_value(None, "port"), "<unset>");
    assert_eq!(plural(1, "route"), "route");
    assert_eq!(plural(2, "route"), "routes");
    assert_eq!(source_config_label(ConfigSource::Project), "project config");
    assert_eq!(
        source_config_label(ConfigSource::Fallback),
        "fallback config"
    );
    assert_eq!(non_empty_value(Some("  web  ")), Some("web"));
    assert_eq!(non_empty_value(Some("  ")), None);

    let range = PortRange {
        start: 29_100,
        end: 29_105,
    };
    assert_eq!(
        ports_in_range(&[29_102, 29_102, 29_000, 29_101], range),
        vec![29_101, 29_102]
    );
    assert_eq!(format_limited_ports(&[]), "none");
    assert_eq!(format_limited_ports(&[1, 2, 3]), "1, 2, 3");
    let many_ports = (1..=12).collect::<Vec<u16>>();
    assert_eq!(
        format_limited_ports(&many_ports),
        "1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (+2 more)"
    );
    assert_eq!(
        stale_lease_prune_limit(
            PortRange {
                start: 29_100,
                end: 29_110,
            },
            &[29_100, 29_101, 30_000],
        ),
        4
    );
    let scan = ListenerConflictScan {
        known_registry: vec![29_100],
        unknown: vec![29_101],
        scanned_ports: 2,
        total_ports: 10,
    };
    assert_eq!(
        format_listener_conflict_scan(&scan),
        "29101 (scanned first 2 of 10 ports)"
    );
}

#[test]
fn config_and_doctor_diagnostics_render_constructed_state() {
    let cwd = Path::new("/workspace/demo/apps/web");
    let local_override = bindport_core::LoadedLocalConfig {
        path: PathBuf::from("/workspace/demo/.bindport.local.toml"),
        format: bindport_core::ConfigFormat::Toml,
        git_tracked: false,
        config: BindPortConfig {
            project: Some(String::from("local-demo")),
            service: Some(String::from("web")),
            services: Some(vec![ServiceConfig {
                name: Some(String::from("web")),
                path: Some(String::from("apps/web")),
                ..ServiceConfig::default()
            }]),
            outputs: Some(vec![bindport_core::OutputConfig {
                name: Some(String::from("local-output")),
                template: Some(String::from("bindport-traefik")),
                target: Some(String::from("routes/{{ route.slug }}.yml")),
                ..bindport_core::OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        },
        unknown_keys: vec![String::from("local_unknown")],
    };
    let config = ResolvedConfig {
        loaded: Some(bindport_core::LoadedConfig {
            path: PathBuf::from("/workspace/demo/bindport.toml"),
            format: bindport_core::ConfigFormat::Toml,
            source: ConfigSource::Project,
            local_override: Some(local_override),
            config: BindPortConfig {
                project: Some(String::from("demo")),
                service: Some(String::from("web")),
                default_range: Some(String::from("29100-29110")),
                skip_ports: Some(vec![29_101, 29_102]),
                services: Some(vec![ServiceConfig {
                    name: Some(String::from("web")),
                    path: Some(String::from("apps/web")),
                    command: Some(vec![String::from("next"), String::from("dev")]),
                    ..ServiceConfig::default()
                }]),
                dashboard: Some(bindport_core::DashboardConfig::default()),
                output_defaults: Some(bindport_core::OutputDefaultsConfig {
                    root: Some(String::from(".bindport/out")),
                    ..bindport_core::OutputDefaultsConfig::default()
                }),
                outputs: Some(vec![bindport_core::OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("routes/{{ route.slug }}.yml")),
                    auto_render: Some(true),
                    delete_on: Some(vec![OutputDeleteState::Removed]),
                    ..bindport_core::OutputConfig::default()
                }]),
                ..BindPortConfig::default()
            },
            unknown_keys: vec![String::from("mystery")],
        }),
        fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
        port_range: PortRange {
            start: 29_100,
            end: 29_110,
        },
        skip_ports: vec![29_101, 29_102],
    };
    let no_config = ResolvedConfig {
        loaded: None,
        fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
        port_range: DEFAULT_PORT_RANGE,
        skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
    };

    print_config_source_explanation(&config);
    print_config_source_explanation(&no_config);
    print_config_field_explanations(&config);
    print_config_field_explanations(&no_config);
    print_doctor_output_config(&config);
    print_doctor_output_config(&no_config);
    print_config_diagnostics(&config);
    print_config_diagnostics(&no_config);

    let identity = explain_run_identity(
        cwd,
        &strings(["next", "dev"]),
        &RunOptions::default(),
        &config,
    );
    print_identity_diagnostics(&identity.identity);
    assert_eq!(identity.project_source, "local override config `project`");
    assert_eq!(identity.service_source, "local override config `service`");

    let resolver = TemplateResolver::new(None, None);
    let route_snapshot = output_route_snapshot(test_status_snapshot(vec![
        status_service("route-1", "active", None),
        status_service("route-2", "active", None),
        status_service("route-3", "active", None),
        status_service("route-4", "active", None),
        status_service("route-5", "active", None),
        status_service("route-6", "active", None),
    ]));
    let mut output = test_output_config("traefik");
    output.target = String::from("routes/{{ route.key }}.yml");
    assert!(print_doctor_output(
        &output,
        &resolver,
        &route_snapshot,
        &temp_test_dir("doctor-output")
    ));

    let empty_range_config = ResolvedConfig {
        loaded: None,
        fallback_path: None,
        port_range: PortRange { start: 1, end: 0 },
        skip_ports: Vec::new(),
    };
    assert!(!print_allocation_diagnostics(
        &empty_range_config,
        &identity.identity,
        None
    ));

    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
    let held_port = listener.local_addr().expect("listener address").port();
    let allocation_config = ResolvedConfig {
        loaded: None,
        fallback_path: None,
        port_range: PortRange {
            start: held_port,
            end: held_port,
        },
        skip_ports: Vec::new(),
    };
    print_previous_port_diagnostics(None, &allocation_config, &[]);
    print_previous_port_diagnostics(Some(held_port.saturating_sub(1)), &allocation_config, &[]);
    print_previous_port_diagnostics(Some(held_port), &allocation_config, &[held_port]);
    let skipped_allocation_config = ResolvedConfig {
        loaded: None,
        fallback_path: None,
        port_range: PortRange {
            start: held_port,
            end: held_port,
        },
        skip_ports: vec![held_port],
    };
    print_previous_port_diagnostics(Some(held_port), &skipped_allocation_config, &[]);
    print_previous_port_diagnostics(Some(held_port), &allocation_config, &[]);

    print_git_diagnostics(Path::new("/definitely/not-a-bindport-git-worktree"));
    print_git_diagnostics(Path::new(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn command_error_conversions_format_underlying_errors() {
    let render_errors = vec![
        RenderCommandError::from(ConfigError::UnknownFormat {
            path: PathBuf::from("bindport.txt"),
        }),
        RenderCommandError::from(OutputConfigError::MissingName { index: 0 }),
        RenderCommandError::InvalidArgument(String::from("bad render arg")),
        RenderCommandError::from(RegistryError::MissingStateDirectory),
        RenderCommandError::from(AdapterTemplateError::InvalidName(String::from("../bad"))),
        RenderCommandError::from(RenderError::TargetCollision {
            target: String::from("routes/demo.yml"),
            route_keys: vec![String::from("a"), String::from("b")],
        }),
        RenderCommandError::from(OutputFileError::UnsafeRoot {
            root: String::from("../out"),
        }),
    ];

    for error in render_errors {
        assert!(!error.to_string().is_empty());
    }

    let template_config_error = TemplateCommandError::from(ConfigError::UnknownFormat {
        path: PathBuf::from("bindport.txt"),
    });
    assert!(matches!(
        template_config_error,
        TemplateCommandError::Config(_)
    ));
    let template_error =
        TemplateCommandError::from(AdapterTemplateError::InvalidName(String::from("../bad")));
    assert!(matches!(template_error, TemplateCommandError::Template(_)));
    let template_invalid = TemplateCommandError::InvalidArgument(String::from("bad template"));
    assert!(matches!(
        template_invalid,
        TemplateCommandError::InvalidArgument(_)
    ));

    let clean_registry_error = CleanCommandError::from(RegistryError::MissingStateDirectory);
    assert!(matches!(
        clean_registry_error,
        CleanCommandError::Registry(_)
    ));
    let json_error = serde_json::from_str::<serde_json::Value>("{").expect_err("json error");
    let clean_json_error = CleanCommandError::from(json_error);
    assert!(matches!(clean_json_error, CleanCommandError::Json(_)));
}
