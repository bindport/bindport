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
