// SPDX-License-Identifier: MIT

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

mod config;
mod constants;
mod hash;
mod identity;
mod paths;
mod ports;
mod validation;
mod workspace;

pub use config::*;
pub use constants::*;
pub(crate) use hash::*;
pub use identity::*;
pub(crate) use paths::*;
pub use ports::*;
pub use validation::*;
pub(crate) use workspace::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn default_range_matches_roadmap() {
        assert_eq!(DEFAULT_PORT_RANGE.start, 29_000);
        assert_eq!(DEFAULT_PORT_RANGE.end, 29_999);
        assert_eq!(DEFAULT_PORT_RANGE.len(), 1_000);
    }

    #[test]
    fn inverted_range_is_empty() {
        let range = PortRange { start: 100, end: 0 };

        assert!(range.is_empty());
        assert_eq!(range.len(), 0);
    }

    #[test]
    fn default_skiplist_marks_reserved_ports() {
        assert!(is_default_skip_port(29_000));
        assert!(is_default_skip_port(29_999));
        assert!(!is_default_skip_port(29_500));
    }

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
    fn service_paths_infer_service_from_cwd() {
        let root = temp_test_dir("service-paths");
        let web_src = root.join("apps").join("web").join("src");
        let api = root.join("apps").join("api");
        fs::create_dir_all(&web_src).expect("web src");
        fs::create_dir_all(&api).expect("api dir");
        let config = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
        )
        .expect("config");

        assert_eq!(
            config.configured_service_name_for_cwd(&root, &web_src),
            Some("web")
        );
        let matched = config
            .configured_service_for_cwd(&root, &web_src)
            .expect("matched web service");
        assert_eq!(matched.name, "web");
        assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);
        assert_eq!(
            config.configured_service_name_for_cwd(&root, &api),
            Some("api")
        );
        assert_eq!(config.configured_service_name_for_cwd(&root, &root), None);
    }

    #[test]
    fn deepest_service_path_match_wins() {
        let root = temp_test_dir("service-path-depth");
        let api_src = root.join("apps").join("api").join("src");
        fs::create_dir_all(&api_src).expect("api src");
        let config = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[[services]]\nname = \"apps\"\npath = \"apps\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
        )
        .expect("config");

        assert_eq!(
            config.configured_service_name_for_cwd(&root, &api_src),
            Some("api")
        );
        let matched = config
            .configured_service_for_cwd(&root, &api_src)
            .expect("matched api service");
        assert_eq!(matched.name, "api");
        assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);
    }

    #[test]
    fn configured_service_precedence_covers_path_ties_and_single_service() {
        let root = temp_test_dir("service-precedence");
        let web_src = root.join("apps").join("web").join("src");
        fs::create_dir_all(&web_src).expect("web src");
        let config = BindPortConfig {
            services: Some(vec![
                ServiceConfig {
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("empty-path")),
                    path: Some(String::from(" ")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("first-web")),
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("second-web")),
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("apps")),
                    path: Some(String::from("apps")),
                    ..ServiceConfig::default()
                },
            ]),
            ..BindPortConfig::default()
        };

        let matched = config
            .configured_service_for_cwd(&root, &web_src)
            .expect("matched first web service");
        assert_eq!(matched.name, "first-web");
        assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);

        let explicit = BindPortConfig {
            service: Some(String::from("explicit")),
            services: config.services.clone(),
            ..BindPortConfig::default()
        };
        assert_eq!(
            explicit.configured_service_for_cwd(&root, &web_src),
            Some(ConfiguredService {
                name: "explicit",
                source: ConfiguredServiceSource::ServiceField
            })
        );

        let single = BindPortConfig {
            services: Some(vec![ServiceConfig {
                name: Some(String::from("solo")),
                ..ServiceConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        assert_eq!(
            single.configured_service_for_cwd(&root, &root),
            Some(ConfiguredService {
                name: "solo",
                source: ConfiguredServiceSource::SingleService
            })
        );
    }

    #[test]
    fn parses_output_config_formats() {
        let toml = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[output_defaults]\nroot = \".bindport/generated\"\ntarget_host = \"127.0.0.1\"\ntarget_scheme = \"http\"\nauto_render = true\ndelete_on = [\"removed\"]\non_failure = \"warn\"\ndebounce_ms = 250\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"web\"]\ntls = false\n",
        )
        .expect("toml config");
        let json = parse_config(
            ConfigFormat::Json,
            r#"{"project":"demo","output_defaults":{"root":".bindport/generated","target_host":"127.0.0.1","target_scheme":"http","auto_render":true,"delete_on":["removed"],"on_failure":"warn","debounce_ms":250},"outputs":[{"name":"traefik","template":"bindport-traefik","target":"traefik/{{ route.slug }}.yml","vars":{"entrypoints":["web"],"tls":false}}]}"#,
        )
        .expect("json config");
        let yaml = parse_config(
            ConfigFormat::Yaml,
            "project: demo\noutput_defaults:\n  root: .bindport/generated\n  target_host: 127.0.0.1\n  target_scheme: http\n  auto_render: true\n  delete_on:\n    - removed\n  on_failure: warn\n  debounce_ms: 250\noutputs:\n  - name: traefik\n    template: bindport-traefik\n    target: traefik/{{ route.slug }}.yml\n    vars:\n      entrypoints:\n        - web\n      tls: false\n",
        )
        .expect("yaml config");

        assert_eq!(toml, json);
        assert_eq!(json, yaml);
        let defaults = toml.output_defaults.as_ref().expect("output defaults");
        assert_eq!(defaults.root.as_deref(), Some(".bindport/generated"));
        assert_eq!(defaults.delete_on, Some(vec![OutputDeleteState::Removed]));
        assert_eq!(defaults.on_failure, Some(OutputFailurePolicy::Warn));
        assert_eq!(defaults.debounce_ms, Some(250));

        let output = toml.output_config("traefik").expect("output by name");
        assert_eq!(output.template.as_deref(), Some("bindport-traefik"));
        assert_eq!(
            output
                .vars
                .as_ref()
                .and_then(|vars| vars.get("entrypoints")),
            Some(&serde_json::json!(["web"]))
        );
        assert_eq!(
            output.vars.as_ref().and_then(|vars| vars.get("tls")),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn local_override_merges_output_config_by_name() {
        let root = temp_test_dir("local-output-override");
        fs::write(
            root.join(".bindport.toml"),
            "project = \"base-project\"\n[output_defaults]\nroot = \".bindport/generated\"\ndebounce_ms = 250\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"web\"]\ntls = false\n[[outputs]]\nname = \"debug\"\ntemplate = \"debug-route\"\ntarget = \"debug/{{ route.slug }}.txt\"\n",
        )
        .expect("write base config");
        fs::write(
            root.join(".bindport.local.toml"),
            "project = \"local-project\"\n[output_defaults]\nroot = \".bindport/local-traefik\"\n[[outputs]]\nname = \"traefik\"\ntarget = \"{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"websecure\"]\n[[outputs]]\nname = \"extra\"\ntemplate = \"extra-template\"\ntarget = \"extra/{{ route.slug }}.txt\"\n",
        )
        .expect("write local override");

        let loaded = discover_config(&root, None)
            .expect("discover config")
            .expect("loaded config");

        assert_eq!(loaded.config.project.as_deref(), Some("local-project"));
        assert_eq!(
            loaded
                .local_override
                .as_ref()
                .map(|local| local.path.as_path()),
            Some(root.join(".bindport.local.toml").as_path())
        );
        let defaults = loaded
            .config
            .output_defaults
            .as_ref()
            .expect("output defaults");
        assert_eq!(defaults.root.as_deref(), Some(".bindport/local-traefik"));
        assert_eq!(defaults.debounce_ms, Some(250));

        let traefik = loaded
            .config
            .output_config("traefik")
            .expect("merged traefik output");
        assert_eq!(traefik.template.as_deref(), Some("bindport-traefik"));
        assert_eq!(traefik.target.as_deref(), Some("{{ route.slug }}.yml"));
        assert_eq!(
            traefik
                .vars
                .as_ref()
                .and_then(|vars| vars.get("entrypoints")),
            Some(&serde_json::json!(["websecure"]))
        );
        assert_eq!(
            traefik.vars.as_ref().and_then(|vars| vars.get("tls")),
            Some(&serde_json::json!(false))
        );
        assert!(loaded.config.output_config("debug").is_some());
        assert!(loaded.config.output_config("extra").is_some());
    }

    #[test]
    fn local_override_reports_git_tracked_state() {
        let root = temp_test_dir("local-override-tracked");
        git(&root, ["init"]);
        fs::write(root.join(".bindport.toml"), "project = \"base\"\n").expect("write base config");
        fs::write(root.join(".bindport.local.toml"), "project = \"local\"\n")
            .expect("write local config");
        git(&root, ["add", "-f", ".bindport.local.toml"]);

        let loaded = discover_config(&root, None)
            .expect("discover config")
            .expect("loaded config");

        assert_eq!(loaded.config.project.as_deref(), Some("local"));
        assert!(
            loaded
                .local_override
                .as_ref()
                .is_some_and(|local| local.git_tracked)
        );
    }

    #[test]
    fn local_override_merges_dashboard_defaults_and_output_edges() {
        let mut config = BindPortConfig {
            dashboard: Some(DashboardConfig {
                host: Some(String::from("127.0.0.1")),
                port: Some(27_080),
                register_service: Some(false),
                auth: Some(DashboardAuthConfig {
                    required: Some(false),
                    token_env: Some(String::from("OLD_TOKEN")),
                    ..DashboardAuthConfig::default()
                }),
                ..DashboardConfig::default()
            }),
            output_defaults: Some(OutputDefaultsConfig {
                root: Some(String::from(".bindport/generated")),
                target_host: Some(String::from("127.0.0.1")),
                ..OutputDefaultsConfig::default()
            }),
            ..BindPortConfig::default()
        };

        config.merge_local_override(BindPortConfig {
            dashboard: Some(DashboardConfig {
                port: Some(27_081),
                allowed_hosts: Some(vec![String::from("localhost")]),
                auth: Some(DashboardAuthConfig {
                    required: Some(true),
                    token: Some(String::from("test-token")),
                    ..DashboardAuthConfig::default()
                }),
                ..DashboardConfig::default()
            }),
            output_defaults: Some(OutputDefaultsConfig {
                target_scheme: Some(String::from("https")),
                ..OutputDefaultsConfig::default()
            }),
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("traefik")),
                template: Some(String::from("bindport-traefik")),
                target: Some(String::from("{{ route.slug }}.yml")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        });

        let dashboard = config.dashboard.as_ref().expect("dashboard");
        assert_eq!(dashboard.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(dashboard.port, Some(27_081));
        assert_eq!(dashboard.register_service, Some(false));
        assert_eq!(
            dashboard.allowed_hosts,
            Some(vec![String::from("localhost")])
        );
        let auth = dashboard.auth.as_ref().expect("auth");
        assert_eq!(auth.required, Some(true));
        assert_eq!(auth.token.as_deref(), Some("test-token"));
        assert_eq!(auth.token_env.as_deref(), Some("OLD_TOKEN"));

        let defaults = config.output_defaults.as_ref().expect("output defaults");
        assert_eq!(defaults.root.as_deref(), Some(".bindport/generated"));
        assert_eq!(defaults.target_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(defaults.target_scheme.as_deref(), Some("https"));
        assert!(config.output_config("traefik").is_some());

        config.merge_local_override(BindPortConfig {
            outputs: Some(vec![OutputConfig {
                template: Some(String::from("nameless")),
                target: Some(String::from("nameless.txt")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        });
        assert_eq!(config.outputs.as_ref().expect("outputs").len(), 2);
    }

    #[test]
    fn effective_outputs_apply_defaults_and_skip_disabled_entries() {
        let config = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[output_defaults]\nroot = \".bindport/generated\"\ntarget_host = \"host.docker.internal\"\ntarget_scheme = \"https\"\nauto_render = false\ndelete_on = [\"stopped\", \"removed\"]\non_failure = \"block\"\ndebounce_ms = 500\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"websecure\"]\n[[outputs]]\nname = \"disabled\"\nenabled = false\n",
        )
        .expect("config");

        let outputs = config.effective_outputs().expect("effective outputs");

        assert_eq!(outputs.len(), 1);
        let output = &outputs[0];
        assert_eq!(output.name, "traefik");
        assert_eq!(output.template, "bindport-traefik");
        assert_eq!(output.root.as_deref(), Some(".bindport/generated"));
        assert_eq!(output.target, "traefik/{{ route.slug }}.yml");
        assert_eq!(output.target_host, "host.docker.internal");
        assert_eq!(output.target_scheme, "https");
        assert!(!output.auto_render);
        assert_eq!(
            output.delete_on,
            vec![OutputDeleteState::Stopped, OutputDeleteState::Removed]
        );
        assert_eq!(output.on_failure, OutputFailurePolicy::Block);
        assert_eq!(output.debounce_ms, 500);
        assert_eq!(
            output.vars.get("entrypoints"),
            Some(&serde_json::json!(["websecure"]))
        );
    }

    #[test]
    fn effective_outputs_use_builtin_defaults() {
        let config = parse_config(
            ConfigFormat::Toml,
            "[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"{{ route.slug }}.yml\"\n",
        )
        .expect("config");

        let output = config
            .effective_outputs()
            .expect("effective outputs")
            .pop()
            .expect("output");

        assert_eq!(output.root, None);
        assert_eq!(output.target_host, DEFAULT_OUTPUT_TARGET_HOST);
        assert_eq!(output.target_scheme, DEFAULT_OUTPUT_TARGET_SCHEME);
        assert_eq!(output.auto_render, DEFAULT_OUTPUT_AUTO_RENDER);
        assert_eq!(output.delete_on, vec![OutputDeleteState::Removed]);
        assert_eq!(output.on_failure, OutputFailurePolicy::Warn);
        assert_eq!(output.debounce_ms, DEFAULT_OUTPUT_DEBOUNCE_MS);
    }

    #[test]
    fn effective_outputs_report_required_field_errors() {
        let missing_name = BindPortConfig {
            outputs: Some(vec![OutputConfig {
                template: Some(String::from("bindport-traefik")),
                target: Some(String::from("{{ route.slug }}.yml")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        assert!(matches!(
            missing_name.effective_outputs(),
            Err(OutputConfigError::MissingName { index: 0 })
        ));

        let missing_template = BindPortConfig {
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("traefik")),
                target: Some(String::from("{{ route.slug }}.yml")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        assert!(matches!(
            missing_template.effective_outputs(),
            Err(OutputConfigError::MissingTemplate { name }) if name == "traefik"
        ));

        let missing_target = BindPortConfig {
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("traefik")),
                template: Some(String::from("bindport-traefik")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        let error = missing_target
            .effective_outputs()
            .expect_err("missing target error");
        assert_eq!(
            error.to_string(),
            "output `traefik` is missing required `target`"
        );

        let duplicate = BindPortConfig {
            outputs: Some(vec![
                OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("{{ route.slug }}.yml")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("{{ route.slug }}.yml")),
                    ..OutputConfig::default()
                },
            ]),
            ..BindPortConfig::default()
        };
        let error = duplicate.effective_outputs().expect_err("duplicate error");
        assert_eq!(
            error.to_string(),
            "output `traefik` is defined more than once"
        );
    }

    #[test]
    fn validate_reports_service_and_output_errors() {
        let config = BindPortConfig {
            services: Some(vec![
                ServiceConfig {
                    path: Some(String::from("../api")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("worker")),
                    path: Some(String::from(" ")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("web")),
                    path: Some(String::from("/tmp/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("args-only")),
                    args: Some(vec![String::from("--port")]),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("empty-command")),
                    command: Some(Vec::new()),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("empty-health")),
                    health_url: Some(String::from(" ")),
                    ..ServiceConfig::default()
                },
            ]),
            outputs: Some(vec![
                OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    name: Some(String::from("debug")),
                    target: Some(String::from("debug/{{ route.slug }}.txt")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    name: Some(String::from("debug")),
                    template: Some(String::from("debug-route")),
                    target: Some(String::from("debug/{{ route.slug }}.txt")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    enabled: Some(false),
                    name: Some(String::from("disabled")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    template: Some(String::from("nameless")),
                    target: Some(String::from("nameless.txt")),
                    ..OutputConfig::default()
                },
            ]),
            hooks: Some(HooksConfig {
                timeout_ms: Some(0),
                commands: Some(vec![
                    HookCommandConfig {
                        name: Some(String::from("reload")),
                        events: Some(Vec::new()),
                        command: Some(vec![String::from(" ")]),
                        timeout_ms: Some(0),
                        ..HookCommandConfig::default()
                    },
                    HookCommandConfig {
                        name: Some(String::from("reload")),
                        command: Some(vec![String::from("true")]),
                        ..HookCommandConfig::default()
                    },
                    HookCommandConfig {
                        name: Some(String::from("missing-command")),
                        events: Some(vec![HookEvent::RouteStarted]),
                        ..HookCommandConfig::default()
                    },
                    HookCommandConfig {
                        enabled: Some(false),
                        name: Some(String::from("disabled-placeholder")),
                        ..HookCommandConfig::default()
                    },
                ]),
            }),
            ..BindPortConfig::default()
        };

        let issues = config.validate();

        assert_eq!(issues.len(), 19);
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].name" && issue.message == "service name is required"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].path"
                && issue
                    .message
                    .contains("must be relative to the config file")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[1].path" && issue.message == "service path must not be empty"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[2].path"
                && issue
                    .message
                    .contains("must be relative to the config file")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[3].name"
                && issue.message.contains("duplicate service name `web`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[4].args"
                && issue.message == "service args require a service command"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[5].command"
                && issue.message == "service command must start with a program"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[6].health_url"
                && issue.message == "service health URL must not be empty"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[0].target"
                && issue
                    .message
                    .contains("output `traefik` is missing required `target`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[1].template"
                && issue
                    .message
                    .contains("output `debug` is missing required `template`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[2].name"
                && issue.message.contains("duplicate output name `debug`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[4].name" && issue.message == "output name is required"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.timeout_ms"
                && issue.message == "hook timeout must be greater than 0"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[0].command"
                && issue.message == "hook command must start with a program"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[0].events"
                && issue.message == "hook events must not be empty"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[0].timeout_ms"
                && issue.message == "hook timeout must be greater than 0"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[1].name"
                && issue.message.contains("duplicate hook name `reload`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[1].events" && issue.message == "hook events are required"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[2].command"
                && issue.message == "hook command is required"
        }));
        assert!(BindPortConfig::default().validate().is_empty());
        assert_eq!(
            ConfigValidationIssue::new("field", "message").to_string(),
            "field: message"
        );
    }

    #[test]
    fn validate_reports_security_sensitive_values() {
        let config = BindPortConfig {
            output_defaults: Some(OutputDefaultsConfig {
                root: Some(String::from("/tmp/bindport-out")),
                ..OutputDefaultsConfig::default()
            }),
            services: Some(vec![
                ServiceConfig {
                    name: Some(String::from("web")),
                    hostname: Some(String::from("web\nlocalhost")),
                    route_url: Some(String::from("http://web.localhost\r\nX-Test: 1")),
                    health_url: Some(String::from("http://127.0.0.1:6379/\r\nPING")),
                    env: Some(BTreeMap::from([
                        (
                            String::from("NODE_OPTIONS"),
                            String::from("--require ./x.js"),
                        ),
                        (String::from("LD_AUDIT"), String::from("./audit.so")),
                        (String::from("GCONV_PATH"), String::from("./gconv")),
                        (String::from("1INVALID"), String::from("value")),
                        (String::from("SAFE_VALUE"), String::from("ok")),
                    ])),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("api")),
                    hostname: Some(String::from("api`localhost")),
                    ..ServiceConfig::default()
                },
            ]),
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("debug")),
                template: Some(String::from("debug-route")),
                root: Some(String::from("../outside")),
                target: Some(String::from("debug/{{ route.slug }}.txt")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };

        let issues = config.validate();

        assert!(issues.iter().any(|issue| {
            issue.field == "output_defaults.root"
                && issue
                    .message
                    .contains("must be relative to the config file")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].hostname"
                && issue
                    .message
                    .contains("must not contain control characters")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].route_url"
                && issue
                    .message
                    .contains("must not contain control characters")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].health_url"
                && issue
                    .message
                    .contains("must not contain control characters")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].env.NODE_OPTIONS"
                && issue.message.contains("can affect child process execution")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].env.LD_AUDIT"
                && issue.message.contains("can affect child process execution")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].env.GCONV_PATH"
                && issue.message.contains("can affect child process execution")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].env.1INVALID"
                && issue.message.contains("must contain only ASCII")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[1].hostname"
                && issue.message.contains("must not contain backticks")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[0].root"
                && issue
                    .message
                    .contains("must be relative to the config file")
        }));
    }

    #[test]
    fn yaml_config_rejects_anchors_aliases_and_oversized_documents() {
        let alias_error = parse_config(
            ConfigFormat::Yaml,
            "project: &project demo\nservice: *project\n",
        )
        .expect_err("yaml aliases are rejected");
        assert!(alias_error.contains("anchors and aliases"));

        let quoted_star = parse_config(ConfigFormat::Yaml, "project: \"demo*\"\n")
            .expect("quoted star is not an alias");
        assert_eq!(quoted_star.project.as_deref(), Some("demo*"));

        let oversized = format!("project: demo\n#{}\n", "x".repeat(MAX_YAML_CONFIG_BYTES));
        let size_error =
            parse_config(ConfigFormat::Yaml, &oversized).expect_err("oversized yaml rejected");
        assert!(size_error.contains("byte limit"));
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
    fn normalizes_branch_labels_for_hostnames() {
        assert_eq!(normalize_branch_label("feature/tree"), "feature-tree");
        assert_eq!(
            normalize_branch_label("BUGFIX/JIRA-123_widget"),
            "bugfix-jira-123-widget"
        );
        assert_eq!(normalize_branch_label("!!!"), "branch");
    }

    #[test]
    fn identity_sources_follow_precedence() {
        let cwd = Path::new("/tmp/bindport");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: None,
            cli_service: Some("cli-service"),
            env_project: Some("env-project"),
            env_service: Some("env-service"),
            config_project: Some("config-project"),
            config_service: Some("config-service"),
        });

        assert_eq!(identity.project, "env-project");
        assert_eq!(identity.service, "cli-service");
    }

    #[test]
    fn config_identity_beats_inference() {
        let cwd = Path::new("/tmp/bindport");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: Some("config-project"),
            config_service: Some("config-service"),
        });

        assert_eq!(identity.project, "config-project");
        assert_eq!(identity.service, "config-service");
    }

    #[test]
    fn package_metadata_infers_standalone_identity() {
        let root = temp_test_dir("package-standalone");
        fs::write(root.join("package.json"), r#"{"name":"@example/portal"}"#)
            .expect("write package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "portal");
        assert_eq!(identity.service, "portal");
    }

    #[test]
    fn package_workspaces_infer_root_project_without_git() {
        let root = temp_test_dir("package-workspaces-root");
        fs::write(
            root.join("package.json"),
            r#"{"name":"example","workspaces":["apps/*"]}"#,
        )
        .expect("write root package json");
        let api = root.join("apps").join("api");
        let api_src = api.join("src");
        fs::create_dir_all(&api_src).expect("api src");
        fs::write(api.join("package.json"), r#"{"name":"@example/api"}"#)
            .expect("write api package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &api_src,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "example");
        assert_eq!(identity.service, "api");
    }

    #[test]
    fn package_workspace_object_infers_root_project() {
        let root = temp_test_dir("package-workspace-object");
        fs::write(
            root.join("package.json"),
            r#"{"name":"example-suite","workspaces":{"packages":["packages/*"]}}"#,
        )
        .expect("write root package json");
        let web = root.join("packages").join("web");
        fs::create_dir_all(&web).expect("web dir");
        fs::write(web.join("package.json"), r#"{"name":"@example/web"}"#)
            .expect("write web package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &web,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "example-suite");
        assert_eq!(identity.service, "web");
    }

    #[test]
    fn pnpm_workspace_yaml_infers_root_project_without_git() {
        let root = temp_test_dir("pnpm-workspace-root");
        fs::write(root.join("package.json"), r#"{"name":"example"}"#)
            .expect("write root package json");
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n")
            .expect("write pnpm workspace");
        let web = root.join("apps").join("web");
        fs::create_dir_all(&web).expect("web dir");
        fs::write(web.join("package.json"), r#"{"name":"@example/web"}"#)
            .expect("write web package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &web,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "example");
        assert_eq!(identity.service, "web");
    }

    #[test]
    fn package_workspace_root_beats_outer_git_root_package() {
        let root = temp_test_dir("workspace-below-git-root");
        git(&root, ["init"]);
        git(&root, ["config", "user.email", "bindport@example.invalid"]);
        git(&root, ["config", "user.name", "BindPort Test"]);
        git(&root, ["config", "commit.gpgsign", "false"]);
        fs::write(root.join("package.json"), r#"{"name":"outer"}"#)
            .expect("write outer package json");
        let workspace = root.join("frontend");
        fs::create_dir_all(&workspace).expect("workspace dir");
        fs::write(
            workspace.join("package.json"),
            r#"{"name":"example","workspaces":["apps/*"]}"#,
        )
        .expect("write workspace package json");
        let web = workspace.join("apps").join("web");
        fs::create_dir_all(&web).expect("web dir");
        fs::write(web.join("package.json"), r#"{"name":"@example/web"}"#)
            .expect("write web package json");
        fs::write(root.join("README.md"), "test\n").expect("write fixture");
        git(
            &root,
            [
                "add",
                "README.md",
                "package.json",
                "frontend/package.json",
                "frontend/apps/web/package.json",
            ],
        );
        git(&root, ["commit", "-m", "initial"]);
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &web,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "example");
        assert_eq!(identity.service, "web");
        assert!(identity.git.is_some());
    }

    #[test]
    fn package_metadata_uses_git_root_project_and_nearest_service() {
        let root = temp_test_dir("package-monorepo");
        git(&root, ["init"]);
        git(&root, ["config", "user.email", "bindport@example.invalid"]);
        git(&root, ["config", "user.name", "BindPort Test"]);
        git(&root, ["config", "commit.gpgsign", "false"]);
        fs::write(root.join("package.json"), r#"{"name":"example"}"#)
            .expect("write root package json");
        let service = root.join("apps").join("web");
        fs::create_dir_all(&service).expect("service dir");
        fs::write(service.join("package.json"), r#"{"name":"@example/web"}"#)
            .expect("write service package json");
        fs::write(root.join("README.md"), "test\n").expect("write fixture");
        git(
            &root,
            ["add", "README.md", "package.json", "apps/web/package.json"],
        );
        git(&root, ["commit", "-m", "initial"]);
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &service,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "example");
        assert_eq!(identity.service, "web");
        assert!(identity.git.is_some());
    }

    #[test]
    fn explicit_identity_beats_package_metadata() {
        let root = temp_test_dir("package-explicit");
        fs::write(root.join("package.json"), r#"{"name":"package-project"}"#)
            .expect("write package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &command,
            cli_project: None,
            cli_service: Some("cli-service"),
            env_project: Some("env-project"),
            env_service: Some("env-service"),
            config_project: Some("config-project"),
            config_service: Some("config-service"),
        });

        assert_eq!(identity.project, "env-project");
        assert_eq!(identity.service, "cli-service");
    }

    #[test]
    fn invalid_package_metadata_falls_back_to_directory_and_command() {
        let root = temp_test_dir("package-invalid");
        fs::write(root.join("package.json"), r#"{"name":""}"#).expect("write package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(
            identity.project,
            root.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(identity.service, "next");
    }

    #[test]
    fn package_identity_handles_scoped_names_and_workspace_fallbacks() {
        assert_eq!(
            package_identity_name("@scope/web"),
            Some(String::from("web"))
        );
        assert_eq!(package_identity_name("@scope/"), None);
        assert_eq!(package_identity_name(" "), None);
        assert_eq!(
            directory_identity_name(Path::new("/")),
            String::from("workspace")
        );

        let root = temp_test_dir("workspace-name-fallback");
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n")
            .expect("write pnpm workspace");
        let metadata = workspace_root_metadata(&root);
        assert_eq!(
            metadata.identity_name,
            root.file_name().unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn identity_key_delimits_project_and_service_values() {
        let cwd = Path::new("/tmp/bindport");
        let command = [String::from("next")];
        let first = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: Some("a:b"),
            cli_service: Some("c"),
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });
        let second = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: Some("a"),
            cli_service: Some("b:c"),
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_ne!(first.identity_key, second.identity_key);
        assert!(first.identity_key.starts_with("v1:"));
    }

    #[test]
    fn identity_port_scan_start_is_stable_and_in_range() {
        let identity = ServiceIdentity {
            project: String::from("bindport"),
            service: String::from("web"),
            git: None,
            identity_key: String::from("v1:test"),
        };
        let range = PortRange {
            start: 29_100,
            end: 29_199,
        };
        let scan_start = identity.port_scan_start(range).expect("scan start");

        assert!(range.contains(scan_start));
        assert_eq!(identity.port_scan_start(range), Some(scan_start));
        assert_eq!(
            identity.port_scan_start(PortRange { start: 100, end: 0 }),
            None
        );
    }

    #[test]
    fn detects_git_worktree_branch_and_commit() {
        let root = temp_test_dir("git-identity");
        git(&root, ["init"]);
        git(&root, ["config", "user.email", "bindport@example.invalid"]);
        git(&root, ["config", "user.name", "BindPort Test"]);
        git(&root, ["config", "commit.gpgsign", "false"]);
        fs::write(root.join("README.md"), "test\n").expect("write fixture");
        git(&root, ["add", "README.md"]);
        git(&root, ["commit", "-m", "initial"]);
        git(&root, ["checkout", "-B", "feature/tree"]);
        let nested = root.join("apps").join("web");
        fs::create_dir_all(&nested).expect("nested dir");

        let identity = detect_git_identity(&nested).expect("git identity");

        assert_eq!(identity.worktree_path, root.canonicalize().expect("root"));
        assert_eq!(identity.branch, "feature/tree");
        assert_eq!(identity.branch_label, "feature-tree");
        assert!(!identity.commit.is_empty());
        assert!(!identity.worktree_hash.is_empty());
    }

    #[test]
    fn parses_port_range() {
        assert_eq!(
            parse_port_range("29100-29199").expect("range"),
            PortRange {
                start: 29_100,
                end: 29_199
            }
        );
        assert_eq!(
            parse_port_range("29100")
                .expect_err("missing separator")
                .to_string(),
            "expected START-END"
        );
        assert_eq!(
            parse_port_range("start-29199")
                .expect_err("invalid start")
                .to_string(),
            "invalid range start `start`"
        );
        assert_eq!(
            parse_port_range("29100-end")
                .expect_err("invalid end")
                .to_string(),
            "invalid range end `end`"
        );
        assert!(matches!(
            parse_port_range("29199-29100"),
            Err(PortRangeParseError::Empty(_))
        ));
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

    fn temp_test_dir(name: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bindport-core-{name}-{}-{now}", std::process::id()));

        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .expect("run git");

        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
