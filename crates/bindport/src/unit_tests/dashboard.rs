// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn dashboard_command_parser_preserves_serve_args_and_modes() {
    let args = strings([
        "start",
        "--host",
        "0.0.0.0",
        "--port",
        "27081",
        "--auth",
        "required",
        "--register-service",
        "--token",
        "secret",
        "--token-env",
        "CUSTOM_TOKEN",
        "--allowed-host",
        "devbox.test",
        "--static-dir",
        "static",
    ]);
    let (command, options) = parse_dashboard_command(&args).expect("dashboard command");

    assert_eq!(command, DashboardCommand::Start);
    assert_eq!(options.host, Some(Ipv4Addr::UNSPECIFIED));
    assert_eq!(options.port, Some(27081));
    assert_eq!(options.auth_required, Some(true));
    assert_eq!(options.register_service, Some(true));
    assert_eq!(options.token.as_deref(), Some("secret"));
    assert_eq!(options.token_env.as_deref(), Some("CUSTOM_TOKEN"));
    assert_eq!(options.allowed_hosts, vec![String::from("devbox.test")]);
    assert_eq!(options.static_dir, Some(PathBuf::from("static")));
    assert_eq!(
        options.serve_args,
        strings([
            "--host",
            "0.0.0.0",
            "--port",
            "27081",
            "--auth",
            "required",
            "--register-service",
            "--token-env",
            "CUSTOM_TOKEN",
            "--allowed-host",
            "devbox.test",
            "--static-dir",
            "static",
        ])
    );

    let (command, options) = parse_dashboard_command(&strings(["--help"])).expect("dashboard help");
    assert_eq!(command, DashboardCommand::Help);
    assert_eq!(options.token_env_name(), DASHBOARD_TOKEN_ENV);
    assert!(parse_dashboard_command(&strings(["--port", "bad"])).is_err());
    assert!(parse_dashboard_command(&strings(["--host", "bad"])).is_err());
    assert!(parse_dashboard_command(&strings(["--auth", "maybe"])).is_err());
    assert!(parse_dashboard_command(&strings(["--missing"])).is_err());
    assert!(parse_dashboard_command(&strings(["--token"])).is_err());

    let (command, _) = parse_dashboard_command(&[]).expect("default serve");
    assert_eq!(command, DashboardCommand::Serve);
    let (command, _) = parse_dashboard_command(&strings(["serve"])).expect("serve");
    assert_eq!(command, DashboardCommand::Serve);
    let (command, _) = parse_dashboard_command(&strings(["status"])).expect("status");
    assert_eq!(command, DashboardCommand::Status);
    let (command, _) = parse_dashboard_command(&strings(["stop"])).expect("stop");
    assert_eq!(command, DashboardCommand::Stop);

    let (_, options) =
        parse_dashboard_command(&strings(["--auth-required", "--no-register-service"]))
            .expect("boolean dashboard flags");
    assert_eq!(options.auth_required, Some(true));
    assert_eq!(options.register_service, Some(false));
    assert_eq!(
        options.serve_args,
        strings(["--auth-required", "--no-register-service"])
    );

    assert!(parse_dashboard_auth_mode("enabled").expect("enabled"));
    assert!(!parse_dashboard_auth_mode("disabled").expect("disabled"));
    assert!(parse_dashboard_bool("yes", "setting").expect("yes"));
    assert!(!parse_dashboard_bool("no", "setting").expect("no"));
}

#[test]
fn dashboard_option_resolution_enforces_auth_and_precedence() {
    let config = ResolvedConfig {
        loaded: Some(bindport_core::LoadedConfig {
            path: PathBuf::from("/workspace/demo/bindport.toml"),
            format: bindport_core::ConfigFormat::Toml,
            source: ConfigSource::Project,
            local_override: None,
            config: BindPortConfig {
                dashboard: Some(bindport_core::DashboardConfig {
                    host: Some(String::from("127.0.0.2")),
                    port: Some(27_081),
                    register_service: Some(true),
                    allowed_hosts: Some(vec![
                        String::from("config.test"),
                        String::from("localhost"),
                    ]),
                    auth: Some(bindport_core::DashboardAuthConfig {
                        required: Some(true),
                        token: Some(String::from("config-token")),
                        token_env: Some(String::from("CONFIG_DASHBOARD_TOKEN")),
                    }),
                }),
                ..BindPortConfig::default()
            },
            unknown_keys: Vec::new(),
        }),
        fallback_path: None,
        port_range: PortRange {
            start: 29_100,
            end: 29_110,
        },
        skip_ports: vec![29_101],
    };
    let cli = DashboardCliOptions {
        host: Some(Ipv4Addr::LOCALHOST),
        port: Some(27_082),
        auth_required: Some(true),
        register_service: Some(false),
        token: Some(String::from("cli-token")),
        allowed_hosts: vec![String::from("cli.test")],
        static_dir: Some(PathBuf::from("dashboard/static")),
        ..DashboardCliOptions::default()
    };

    let options =
        resolve_dashboard_options(&config, &cli, vec![29_102]).expect("dashboard options");

    assert_eq!(options.host, Ipv4Addr::LOCALHOST);
    assert_eq!(options.preferred_port, 27_082);
    assert_eq!(options.fallback_range.start, 29_100);
    assert_eq!(options.skip_ports, vec![29_102]);
    assert!(options.allowed_hosts.contains(&String::from("127.0.0.1")));
    assert!(options.allowed_hosts.contains(&String::from("cli.test")));
    assert!(options.allowed_hosts.contains(&String::from("config.test")));
    assert!(options.auth.required);
    assert_eq!(options.auth.token.as_deref(), Some("cli-token"));
    assert_eq!(options.static_dir, Some(PathBuf::from("dashboard/static")));
    assert!(!resolve_dashboard_registration(&config, &cli).expect("registration"));

    let non_loopback_without_auth = DashboardCliOptions {
        host: Some(Ipv4Addr::UNSPECIFIED),
        auth_required: Some(false),
        ..DashboardCliOptions::default()
    };
    assert!(matches!(
        resolve_dashboard_options(&config, &non_loopback_without_auth, Vec::new()),
        Err(DashboardCommandError::InvalidArgument(message))
            if message.contains("requires auth")
    ));

    let missing_token = DashboardCliOptions {
        auth_required: Some(true),
        token_env: Some(String::from("BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST")),
        ..DashboardCliOptions::default()
    };
    let config_without_token = ResolvedConfig {
        loaded: None,
        fallback_path: None,
        port_range: DEFAULT_PORT_RANGE,
        skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
    };
    assert!(matches!(
        resolve_dashboard_options(&config_without_token, &missing_token, Vec::new()),
        Err(DashboardCommandError::MissingToken { source_name })
            if source_name == "BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST"
    ));
}

#[test]
fn dashboard_env_options_parse_values_and_report_errors() {
    assert_eq!(
        parse_env_dashboard_host(Some(String::from("127.0.0.2"))).expect("host"),
        Some(Ipv4Addr::new(127, 0, 0, 2))
    );
    assert_eq!(
        parse_env_dashboard_port(Some(String::from("27081"))).expect("port"),
        Some(27_081)
    );
    assert_eq!(
        parse_env_dashboard_auth_required(Some(String::from("required"))).expect("auth"),
        Some(true)
    );
    assert_eq!(
        parse_env_dashboard_register_service(Some(String::from("false"))).expect("register"),
        Some(false)
    );

    assert!(parse_env_dashboard_host(Some(String::from("bad"))).is_err());
    assert!(parse_env_dashboard_port(Some(String::from("bad"))).is_err());
    assert!(parse_env_dashboard_auth_required(Some(String::from("maybe"))).is_err());
    assert!(parse_env_dashboard_register_service(Some(String::from("maybe"))).is_err());
    assert_eq!(parse_env_dashboard_host(None).expect("host"), None);
    assert_eq!(parse_env_dashboard_port(None).expect("port"), None);
    assert_eq!(parse_env_dashboard_auth_required(None).expect("auth"), None);
    assert_eq!(
        parse_env_dashboard_register_service(None).expect("register"),
        None
    );
}

#[test]
fn dashboard_command_errors_preserve_error_variants() {
    let config = ConfigError::UnknownFormat {
        path: PathBuf::from("/tmp/bindport.txt"),
    };
    assert!(matches!(
        DashboardCommandError::from(config),
        DashboardCommandError::Config(_)
    ));

    let dashboard = bindport_dashboard::DashboardError::LocalAddress(io::Error::other("gone"));
    assert!(matches!(
        DashboardCommandError::from(dashboard),
        DashboardCommandError::Dashboard(_)
    ));

    assert!(matches!(
        DashboardCommandError::from(io::Error::other("dashboard io")),
        DashboardCommandError::Io(_)
    ));
}

#[test]
fn dashboard_command_dispatch_handles_service_and_error_paths() {
    let state_home = temp_test_dir("dashboard-dispatch-state");
    with_state_home(&state_home, || {
        assert!(run_dashboard_result(&strings(["status"])).is_ok());
        assert!(run_dashboard_result(&strings(["stop"])).is_ok());
        assert!(run_dashboard_result(&strings(["--help"])).is_ok());
    });

    assert!(matches!(
        run_dashboard_result(&strings(["--port", "bad"])),
        Err(DashboardCommandError::InvalidArgument(_))
    ));
    assert_eq!(run_dashboard(&strings(["--bad"])), ExitCode::FAILURE);
    assert_eq!(run_dashboard(&strings(["--help"])), ExitCode::SUCCESS);

    let config_home = temp_test_dir("dashboard-dispatch-config");
    let state_home = temp_test_dir("dashboard-dispatch-auth-state");
    let config_value = config_home.to_string_lossy().to_string();
    let state_value = state_home.to_string_lossy().to_string();
    with_env_values(
        &[
            ("XDG_CONFIG_HOME", Some(config_value.as_str())),
            ("XDG_STATE_HOME", Some(state_value.as_str())),
            (DASHBOARD_TOKEN_ENV, None),
        ],
        || {
            assert_eq!(
                run_dashboard(&strings(["--auth-required"])),
                ExitCode::FAILURE
            );
        },
    );
}

#[test]
fn dashboard_registration_records_and_finishes_dashboard_service() {
    let registry_path = temp_registry_path("dashboard-registration");
    with_default_registry_path(&registry_path, || {
        let server = DashboardServer::bind(DashboardOptions {
            preferred_port: 0,
            ..DashboardOptions::default()
        })
        .expect("dashboard server");
        let cwd = temp_test_dir("dashboard-registration-cwd");
        let registration = register_dashboard_service(true, &server, "127.0.0.1", &cwd);

        assert!(registration.registry.is_some());
        assert!(registration.started.is_some());

        drop(registration);
        let mut registry = Registry::open(&registry_path).expect("registry");
        let snapshot = registry.status_snapshot().expect("status");
        let service = snapshot
            .services
            .iter()
            .find(|service| service.project == SERVICE_NAME && service.service == "dashboard")
            .expect("dashboard service");

        assert_eq!(service.state, "stopped");
        assert_eq!(service.host, "127.0.0.1");
        assert_eq!(service.port, server.port());
        assert_eq!(service.route_url.as_deref(), Some(server.url().as_str()));
        assert!(service.exited_at.is_some());
    });
}

#[test]
fn dashboard_registration_can_be_inactive_or_registry_disabled() {
    let server = DashboardServer::bind(DashboardOptions {
        preferred_port: 0,
        ..DashboardOptions::default()
    })
    .expect("dashboard server");
    let cwd = temp_test_dir("dashboard-registration-inactive-cwd");
    let inactive = register_dashboard_service(false, &server, "127.0.0.1", &cwd);

    assert!(inactive.registry.is_none());
    assert!(inactive.started.is_none());

    let blocked_parent = temp_test_dir("dashboard-registration-blocked").join("parent-file");
    fs::write(&blocked_parent, "not a directory").expect("blocked parent");
    let registry_path = blocked_parent.join("registry.sqlite");
    with_default_registry_path(&registry_path, || {
        let registration = register_dashboard_service(true, &server, "127.0.0.1", &cwd);

        assert!(registration.registry.is_none());
        assert!(registration.started.is_none());
    });
}

#[test]
fn dashboard_command_redaction_hides_literal_token_values() {
    let command = redacted_dashboard_command_from(strings([
        "bindport",
        "dashboard",
        "serve",
        "--token",
        "secret",
        "--port",
        "27080",
    ]));

    assert_eq!(command, "bindport dashboard serve --token *** --port 27080");
    assert!(!command.contains("secret"));

    let trailing_token = redacted_dashboard_command_from(strings(["bindport", "--token"]));
    assert_eq!(trailing_token, "bindport --token");
}

#[test]
fn dashboard_state_file_round_trips_and_removes_idempotently() {
    let state_home = temp_test_dir("dashboard-state-home");
    with_state_home(&state_home, || {
        assert!(read_dashboard_state().expect("read missing").is_none());
        remove_dashboard_state().expect("remove missing state");

        let state = DashboardServiceState {
            pid: 12_345,
            url: String::from("http://127.0.0.1:27080"),
            process_start_time: Some(99),
        };
        write_dashboard_state(&state).expect("write state");

        assert_eq!(read_dashboard_state().expect("read state"), Some(state));

        fs::write(
            dashboard_state_path().expect("state path"),
            "pid=bad\nurl=http://x\n",
        )
        .expect("write invalid pid");
        assert!(read_dashboard_state().expect("read invalid pid").is_none());

        fs::write(dashboard_state_path().expect("state path"), "pid=1\n").expect("write no url");
        assert!(read_dashboard_state().expect("read missing url").is_none());

        remove_dashboard_state().expect("remove state");
        assert!(read_dashboard_state().expect("read removed").is_none());
    });
}

#[test]
fn dashboard_service_status_and_stop_handle_missing_state() {
    let state_home = temp_test_dir("dashboard-service-state-home");
    with_state_home(&state_home, || {
        print_dashboard_service_status().expect("missing status");
        stop_dashboard_service().expect("missing stop");
    });
}

#[test]
fn dashboard_start_error_uses_log_when_available() {
    let state_home = temp_test_dir("dashboard-start-error-state-home");
    with_state_home(&state_home, || {
        assert_eq!(
            dashboard_start_error().to_string(),
            "dashboard did not start"
        );

        create_dashboard_state_dir().expect("state dir");
        fs::write(
            dashboard_log_path().expect("dashboard log"),
            format!("{}\n", "x".repeat(600)),
        )
        .expect("write dashboard log");
        let message = dashboard_start_error().to_string();

        assert!(message.starts_with("dashboard did not start: "));
        assert_eq!(
            message
                .strip_prefix("dashboard did not start: ")
                .expect("prefix")
                .len(),
            500
        );
    });
}

#[test]
fn dashboard_process_helpers_match_current_process_shape() {
    let pid = std::process::id();

    assert!(process_is_running(pid));

    #[cfg(target_os = "linux")]
    {
        let state = DashboardServiceState {
            pid,
            url: String::from("http://127.0.0.1:27080"),
            process_start_time: process_start_time(pid),
        };

        assert!(!process_cmdline_is_dashboard(pid));
        assert!(!dashboard_process_matches_state(&state));
        assert!(!dashboard_process_is_running(&state));
        assert!(process_start_time(u32::MAX).is_none());
        assert!(!process_cmdline_is_dashboard(u32::MAX));
    }

    #[cfg(not(target_os = "linux"))]
    {
        let state = DashboardServiceState {
            pid,
            url: String::from("http://127.0.0.1:27080"),
            process_start_time: None,
        };

        assert!(process_start_time(pid).is_none());
        assert_eq!(
            dashboard_process_matches_state(&state),
            process_cmdline_is_dashboard(pid).unwrap_or(true)
        );
        assert_eq!(process_cmdline_is_dashboard(pid), Some(false));
    }
}

#[test]
fn dashboard_command_line_detection_requires_dashboard_serve() {
    assert!(dashboard_args_contain_serve([
        "bindport",
        "dashboard",
        "serve",
        "--port",
        "27080"
    ]));
    assert!(!dashboard_args_contain_serve([
        "bindport",
        "dashboard",
        "status"
    ]));
    assert!(!dashboard_args_contain_serve([
        "bindport",
        "serve",
        "dashboard"
    ]));
}
