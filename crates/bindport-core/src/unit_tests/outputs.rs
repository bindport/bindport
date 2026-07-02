// SPDX-License-Identifier: MIT

use super::*;

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
