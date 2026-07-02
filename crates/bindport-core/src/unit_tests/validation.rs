// SPDX-License-Identifier: MIT

use super::*;

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
        issue.field == "services[3].name" && issue.message.contains("duplicate service name `web`")
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
        issue.field == "outputs[2].name" && issue.message.contains("duplicate output name `debug`")
    }));
    assert!(issues.iter().any(|issue| {
        issue.field == "outputs[4].name" && issue.message == "output name is required"
    }));
    assert!(issues.iter().any(|issue| {
        issue.field == "hooks.timeout_ms" && issue.message == "hook timeout must be greater than 0"
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
        issue.field == "hooks.commands[2].command" && issue.message == "hook command is required"
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
