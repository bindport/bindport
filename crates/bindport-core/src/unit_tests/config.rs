// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn config_filenames_preserve_format_precedence() {
    assert_eq!(
        CONFIG_FILENAMES,
        [".bindport.toml", ".bindport.json", ".bindport.yaml"]
    );
    assert_eq!(
        ConfigFormat::from_path(Path::new("bindport.yml")),
        Some(ConfigFormat::Yaml)
    );
    assert_eq!(ConfigFormat::from_path(Path::new("bindport.txt")), None);
    assert_eq!(ConfigFormat::Toml.as_str(), "toml");
    assert_eq!(ConfigFormat::Json.as_str(), "json");
    assert_eq!(ConfigFormat::Yaml.as_str(), "yaml");
    assert_eq!(ConfigSource::Project.as_str(), "project");
    assert_eq!(ConfigSource::Fallback.as_str(), "fallback");
}

#[test]
fn parses_config_formats() {
    let toml = parse_config(
        ConfigFormat::Toml,
        "project = \"demo\"\ndefault_range = \"29100-29199\"\nskip_ports = [29100]\n[dashboard]\nhost = \"127.0.0.1\"\nport = 27080\nregister_service = true\nallowed_hosts = [\"localhost\"]\n[dashboard.auth]\nrequired = true\ntoken_env = \"BINDPORT_DASHBOARD_TOKEN\"\n[[services]]\nname = \"web\"\npath = \"apps/web\"\ncommand = [\"storybook\", \"dev\"]\nargs = [\"--port\", \"{port}\"]\nhostname = \"{branch}.{project}.localhost\"\nroute_url = \"http://{hostname}\"\nhealth_url = \"{route_url}/health\"\nenv.PORT = \"{port}\"\nenv.NEXT_PUBLIC_BINDPORT_URL = \"{route_url}\"\n",
    )
    .expect("toml config");
    let json = parse_config(
        ConfigFormat::Json,
        r#"{"project":"demo","default_range":"29100-29199","skip_ports":[29100],"dashboard":{"host":"127.0.0.1","port":27080,"register_service":true,"allowed_hosts":["localhost"],"auth":{"required":true,"token_env":"BINDPORT_DASHBOARD_TOKEN"}},"services":[{"name":"web","path":"apps/web","command":["storybook","dev"],"args":["--port","{port}"],"hostname":"{branch}.{project}.localhost","route_url":"http://{hostname}","health_url":"{route_url}/health","env":{"PORT":"{port}","NEXT_PUBLIC_BINDPORT_URL":"{route_url}"}}]}"#,
    )
    .expect("json config");
    let yaml = parse_config(
        ConfigFormat::Yaml,
        "project: demo\ndefault_range: 29100-29199\nskip_ports:\n  - 29100\ndashboard:\n  host: 127.0.0.1\n  port: 27080\n  register_service: true\n  allowed_hosts:\n    - localhost\n  auth:\n    required: true\n    token_env: BINDPORT_DASHBOARD_TOKEN\nservices:\n  - name: web\n    path: apps/web\n    command:\n      - storybook\n      - dev\n    args:\n      - --port\n      - \"{port}\"\n    hostname: \"{branch}.{project}.localhost\"\n    route_url: \"http://{hostname}\"\n    health_url: \"{route_url}/health\"\n    env:\n      PORT: \"{port}\"\n      NEXT_PUBLIC_BINDPORT_URL: \"{route_url}\"\n",
    )
    .expect("yaml config");

    assert_eq!(toml, json);
    assert_eq!(json, yaml);
    let dashboard = toml.dashboard.as_ref().expect("dashboard config");
    assert_eq!(dashboard.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(dashboard.port, Some(27_080));
    assert_eq!(dashboard.register_service, Some(true));
    assert_eq!(
        dashboard.allowed_hosts,
        Some(vec![String::from("localhost")])
    );
    let auth = dashboard.auth.as_ref().expect("dashboard auth");
    assert_eq!(auth.required, Some(true));
    assert_eq!(auth.token_env.as_deref(), Some("BINDPORT_DASHBOARD_TOKEN"));
    let service = toml.service_config("web").expect("service config by name");
    assert_eq!(service.path.as_deref(), Some("apps/web"));
    assert_eq!(
        service.command_argv(),
        Some(vec![
            String::from("storybook"),
            String::from("dev"),
            String::from("--port"),
            String::from("{port}"),
        ])
    );
    assert_eq!(
        service.hostname.as_deref(),
        Some("{branch}.{project}.localhost")
    );
    assert_eq!(service.route_url.as_deref(), Some("http://{hostname}"));
    assert_eq!(service.health_url.as_deref(), Some("{route_url}/health"));
    assert_eq!(
        service
            .env
            .as_ref()
            .and_then(|env| env.get("NEXT_PUBLIC_BINDPORT_URL"))
            .map(String::as_str),
        Some("{route_url}")
    );
    assert_eq!(toml.configured_service_name(), Some("web"));
}

#[test]
fn parses_hook_config_formats() {
    let toml = parse_config(
        ConfigFormat::Toml,
        "project = \"demo\"\n[hooks]\ntimeout_ms = 2500\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\", \"output_rendered\"]\ncommand = [\"bindport\", \"render\"]\ntimeout_ms = 1000\n",
    )
    .expect("toml hooks config");
    let json = parse_config(
        ConfigFormat::Json,
        r#"{"project":"demo","hooks":{"timeout_ms":2500,"commands":[{"name":"reload","events":["route_started","output_rendered"],"command":["bindport","render"],"timeout_ms":1000}]}}"#,
    )
    .expect("json hooks config");
    let yaml = parse_config(
        ConfigFormat::Yaml,
        "project: demo\nhooks:\n  timeout_ms: 2500\n  commands:\n    - name: reload\n      events:\n        - route_started\n        - output_rendered\n      command:\n        - bindport\n        - render\n      timeout_ms: 1000\n",
    )
    .expect("yaml hooks config");

    assert_eq!(toml, json);
    assert_eq!(json, yaml);
    let hooks = toml.hooks.as_ref().expect("hooks");
    assert_eq!(hooks.timeout_ms, Some(2_500));
    let command = &hooks.commands.as_ref().expect("hook commands")[0];
    assert_eq!(command.name.as_deref(), Some("reload"));
    assert_eq!(
        command.events,
        Some(vec![HookEvent::RouteStarted, HookEvent::OutputRendered])
    );
    assert_eq!(
        command.command,
        Some(vec![String::from("bindport"), String::from("render")])
    );
    assert_eq!(command.timeout_ms, Some(1_000));
}

#[test]
fn local_override_filenames_preserve_format_precedence() {
    assert_eq!(
        LOCAL_CONFIG_FILENAMES,
        [
            ".bindport.local.toml",
            ".bindport.local.json",
            ".bindport.local.yaml",
            ".bindport.local.yml",
            "bindport.local.toml",
            "bindport.local.json",
            "bindport.local.yaml",
            "bindport.local.yml"
        ]
    );
}

#[test]
fn reports_unknown_top_level_config_keys() {
    let keys = unknown_top_level_config_keys(
        ConfigFormat::Toml,
        "project = \"demo\"\ndefaultrange = \"29100-29199\"\n[proxy.traefik]\nenabled = true\n",
    )
    .expect("unknown keys");

    assert_eq!(keys, ["defaultrange", "proxy"]);
    assert_eq!(
        unknown_top_level_config_keys(ConfigFormat::Json, "[]").expect("json array"),
        Vec::<String>::new()
    );
    assert_eq!(
        unknown_top_level_config_keys(ConfigFormat::Yaml, "[]").expect("yaml array"),
        Vec::<String>::new()
    );
    assert_eq!(
        unknown_top_level_config_keys(ConfigFormat::Json, r#"{"project":"demo","extra":true}"#)
            .expect("json object"),
        vec![String::from("extra")]
    );
    assert_eq!(
        unknown_top_level_config_keys(ConfigFormat::Yaml, "project: demo\nextra: true\n")
            .expect("yaml mapping"),
        vec![String::from("extra")]
    );
    assert!(unknown_top_level_config_keys(ConfigFormat::Toml, "=").is_err());
    assert!(unknown_top_level_config_keys(ConfigFormat::Json, "{").is_err());
    assert!(unknown_top_level_config_keys(ConfigFormat::Yaml, "bad: [").is_err());
}

#[test]
fn load_config_reports_read_format_and_parse_errors() {
    let root = temp_test_dir("load-config-errors");
    let missing = root.join("missing.toml");
    let read_error = load_config(&missing, ConfigSource::Project).expect_err("missing config");
    assert!(matches!(read_error, ConfigError::Read { .. }));

    fs::write(root.join("bindport.txt"), "project = \"demo\"\n").expect("unknown format");
    let format_error =
        load_config(root.join("bindport.txt"), ConfigSource::Project).expect_err("format");
    assert!(matches!(format_error, ConfigError::UnknownFormat { .. }));

    fs::write(root.join("bad.toml"), "=").expect("bad toml");
    let parse_error = load_config(root.join("bad.toml"), ConfigSource::Project).expect_err("toml");
    assert!(matches!(
        parse_error,
        ConfigError::Parse {
            format: ConfigFormat::Toml,
            ..
        }
    ));

    fs::write(root.join("bad.json"), "{").expect("bad json");
    let parse_error = load_config(root.join("bad.json"), ConfigSource::Project).expect_err("json");
    assert!(matches!(
        parse_error,
        ConfigError::Parse {
            format: ConfigFormat::Json,
            ..
        }
    ));

    fs::write(root.join("bad.yaml"), "bad: [").expect("bad yaml");
    let parse_error = load_config(root.join("bad.yaml"), ConfigSource::Project).expect_err("yaml");
    assert!(matches!(
        parse_error,
        ConfigError::Parse {
            format: ConfigFormat::Yaml,
            ..
        }
    ));
}

#[test]
fn discover_config_uses_fallback_and_project_local_override() {
    let root = temp_test_dir("discover-config");
    let nested = root.join("apps/web");
    fs::create_dir_all(&nested).expect("nested dir");
    let fallback = root.join("fallback.toml");
    fs::write(&fallback, "project = \"fallback\"\n").expect("fallback config");

    let loaded = discover_config(&nested, Some(&fallback))
        .expect("discover fallback")
        .expect("fallback loaded");

    assert_eq!(loaded.source, ConfigSource::Fallback);
    assert_eq!(loaded.config.project.as_deref(), Some("fallback"));
    assert!(loaded.local_override.is_none());
    assert!(
        load_project_local_override(loaded.clone())
            .expect("fallback local override")
            .local_override
            .is_none()
    );

    fs::write(
        root.join(".bindport.toml"),
        "project = \"project\"\ndefault_range = \"29100-29110\"\nunknown_base = true\n",
    )
    .expect("project config");
    fs::write(
        root.join(".bindport.local.toml"),
        "service = \"web\"\nunknown_local = true\n",
    )
    .expect("local config");

    let loaded = discover_config(&nested, Some(&fallback))
        .expect("discover project")
        .expect("project loaded");

    assert_eq!(loaded.source, ConfigSource::Project);
    assert_eq!(loaded.config.project.as_deref(), Some("project"));
    assert_eq!(loaded.config.service.as_deref(), Some("web"));
    assert_eq!(
        loaded.unknown_keys,
        vec![String::from("unknown_base"), String::from("unknown_local")]
    );
    let local = loaded.local_override.expect("local override");
    assert_eq!(local.format, ConfigFormat::Toml);
    assert!(!local.git_tracked);
    assert_eq!(local.config.service.as_deref(), Some("web"));
    assert_eq!(local.unknown_keys, vec![String::from("unknown_local")]);
}

#[test]
fn yaml_anchor_detection_handles_compact_flow_tokens_without_false_positives() {
    for contents in [
        "defaults: [&defaults value]\n",
        "--- &defaults\nvalue: safe\n",
        "value: !custom &defaults safe\n",
        "service: [*defaults]\n",
        "service: {value:*defaults}\n",
    ] {
        assert!(yaml_contains_anchor_or_alias(contents), "{contents:?}");
    }
    for contents in [
        "url: 'https://example.test/a*b'\n",
        "literal: \"rock&roll\"\n",
        "value: plain*text\n",
        "flags: echo &word\n",
        "flags: -Wl, &word\n",
        "# [*defaults]\nvalue: safe\n",
    ] {
        assert!(!yaml_contains_anchor_or_alias(contents), "{contents:?}");
    }

    let config = parse_config(
        ConfigFormat::Yaml,
        "services:\n  - name: web\n    env:\n      LINKER_FLAGS: -Wl, &word\n",
    )
    .expect("plain YAML scalar containing ampersand word");
    assert_eq!(
        config.services.expect("services")[0]
            .env
            .as_ref()
            .and_then(|env| env.get("LINKER_FLAGS"))
            .map(String::as_str),
        Some("-Wl, &word")
    );
}

#[test]
fn loaded_config_helpers_report_defaults_and_invalid_ranges() {
    let loaded = LoadedConfig {
        path: PathBuf::from("/workspace/demo/.bindport.toml"),
        format: ConfigFormat::Toml,
        source: ConfigSource::Project,
        local_override: None,
        config: BindPortConfig {
            default_range: Some(String::from("bad")),
            services: Some(vec![ServiceConfig {
                name: Some(String::from("web")),
                path: Some(String::from("apps/web")),
                ..ServiceConfig::default()
            }]),
            ..BindPortConfig::default()
        },
        unknown_keys: Vec::new(),
    };

    assert_eq!(
        loaded.configured_service_name_for_cwd(Path::new("/workspace/demo/apps/web")),
        Some("web")
    );
    assert!(matches!(
        loaded.port_range(),
        Err(ConfigError::InvalidPortRange { .. })
    ));
    assert_eq!(loaded.skip_ports(), DEFAULT_SKIP_PORTS);
}

#[test]
fn config_errors_preserve_display_and_sources() {
    let path = PathBuf::from("/tmp/bindport.toml");
    let read = ConfigError::Read {
        path: path.clone(),
        source: io::Error::new(io::ErrorKind::NotFound, "missing"),
    };
    assert!(read.to_string().contains("failed to read config"));
    assert!(std::error::Error::source(&read).is_some());

    let unknown = ConfigError::UnknownFormat {
        path: PathBuf::from("/tmp/bindport.txt"),
    };
    assert_eq!(
        unknown.to_string(),
        "unsupported config format `/tmp/bindport.txt`"
    );
    assert!(std::error::Error::source(&unknown).is_none());

    let parse = ConfigError::Parse {
        path: path.clone(),
        format: ConfigFormat::Json,
        source: String::from("bad json"),
    };
    assert!(
        parse
            .to_string()
            .contains("failed to parse json config `/tmp/bindport.toml`")
    );
    assert!(std::error::Error::source(&parse).is_none());

    let range = ConfigError::InvalidPortRange {
        path,
        source: PortRangeParseError::MissingSeparator,
    };
    assert!(
        range
            .to_string()
            .contains("invalid default_range in config `/tmp/bindport.toml`")
    );
    assert!(std::error::Error::source(&range).is_some());
}
