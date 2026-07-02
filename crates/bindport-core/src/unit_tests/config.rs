// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn config_filenames_preserve_format_precedence() {
    assert_eq!(
        CONFIG_FILENAMES,
        [".bindport.toml", ".bindport.json", ".bindport.yaml"]
    );
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
