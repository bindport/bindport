// SPDX-License-Identifier: MIT

use super::*;

const TOML_CONTRACT: &str = include_str!("../../tests/fixtures/config-v1-candidate.toml");
const JSON_CONTRACT: &str = include_str!("../../tests/fixtures/config-v1-candidate.json");
const YAML_CONTRACT: &str = include_str!("../../tests/fixtures/config-v1-candidate.yaml");

#[test]
fn stable_candidate_fixtures_cover_the_complete_config_shape() {
    let expected = stable_candidate_config();

    for (format, contents) in [
        (ConfigFormat::Toml, TOML_CONTRACT),
        (ConfigFormat::Json, JSON_CONTRACT),
        (ConfigFormat::Yaml, YAML_CONTRACT),
    ] {
        assert_eq!(
            parse_config(format, contents).expect("stable candidate config"),
            expected,
            "{} fixture drifted from the config contract",
            format.as_str()
        );
    }
}

#[test]
fn checked_in_schema_names_every_supported_key_and_enum_value() {
    let schema = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../../docs/config.schema.json"
    ))
    .expect("config schema json");

    assert_eq!(
        schema["title"],
        "BindPort configuration v1 stable candidate"
    );
    assert_eq!(schema["additionalProperties"], true);
    let range_schema = &schema["properties"]["default_range"];
    let range_description = range_schema["description"]
        .as_str()
        .expect("default_range description");
    assert!(range_description.contains("START must be at least 1"));
    assert!(range_description.contains("Whitespace around each value"));
    assert!(range_description.contains("leading zeroes"));
    assert!(range_schema.get("pattern").is_none());
    assert_eq!(
        APPLIED_CONFIG_KEYS,
        [
            "project",
            "service",
            "default_range",
            "skip_ports",
            "services",
            "dashboard",
            "output_defaults",
            "outputs",
            "hooks",
        ]
    );
    assert_eq!(
        property_names(&schema),
        BTreeSet::from([
            "dashboard",
            "default_range",
            "hooks",
            "output_defaults",
            "outputs",
            "project",
            "service",
            "services",
            "skip_ports",
        ])
    );
    assert_eq!(
        definition_property_names(&schema, "service"),
        BTreeSet::from([
            "args",
            "command",
            "env",
            "health_url",
            "hostname",
            "name",
            "path",
            "route_url",
        ])
    );
    assert_eq!(
        definition_property_names(&schema, "dashboard"),
        BTreeSet::from(["allowed_hosts", "auth", "host", "port", "register_service",])
    );
    assert_eq!(
        definition_property_names(&schema, "dashboardAuth"),
        BTreeSet::from(["required", "token", "token_env"])
    );
    assert_eq!(
        definition_property_names(&schema, "outputDefaults"),
        BTreeSet::from([
            "auto_render",
            "debounce_ms",
            "delete_on",
            "on_failure",
            "root",
            "target_host",
            "target_scheme",
        ])
    );
    assert_eq!(
        definition_property_names(&schema, "output"),
        BTreeSet::from([
            "auto_render",
            "debounce_ms",
            "delete_on",
            "enabled",
            "name",
            "on_failure",
            "root",
            "target",
            "target_host",
            "target_scheme",
            "template",
            "vars",
        ])
    );
    assert_eq!(
        definition_property_names(&schema, "hooks"),
        BTreeSet::from(["commands", "timeout_ms"])
    );
    assert_eq!(
        definition_property_names(&schema, "hookCommand"),
        BTreeSet::from(["command", "enabled", "events", "name", "timeout_ms"])
    );
    assert_eq!(
        enum_strings(&schema["$defs"]["nullableDeleteStates"]["items"]),
        BTreeSet::from(["removed", "stale", "stopped"])
    );
    assert_eq!(
        enum_strings(&schema["$defs"]["nullableFailurePolicy"]),
        BTreeSet::from(["block", "warn"])
    );
    assert_eq!(
        enum_strings(&schema["$defs"]["hookEvent"]),
        BTreeSet::from([
            "output_rendered",
            "render_requested",
            "route_finished",
            "route_started",
            "routes_marked_stale",
            "routes_removed",
        ])
    );
}

#[test]
fn stable_candidate_enum_matches_are_exhaustive() {
    assert_eq!(
        [
            output_delete_state_name(OutputDeleteState::Stopped),
            output_delete_state_name(OutputDeleteState::Stale),
            output_delete_state_name(OutputDeleteState::Removed),
        ],
        ["stopped", "stale", "removed"]
    );
    assert_eq!(
        [
            output_failure_policy_name(OutputFailurePolicy::Warn),
            output_failure_policy_name(OutputFailurePolicy::Block),
        ],
        ["warn", "block"]
    );
    assert_eq!(
        [
            hook_event_name(HookEvent::RouteStarted),
            hook_event_name(HookEvent::RouteFinished),
            hook_event_name(HookEvent::RoutesRemoved),
            hook_event_name(HookEvent::RoutesMarkedStale),
            hook_event_name(HookEvent::RenderRequested),
            hook_event_name(HookEvent::OutputRendered),
        ],
        [
            "route_started",
            "route_finished",
            "routes_removed",
            "routes_marked_stale",
            "render_requested",
            "output_rendered",
        ]
    );
}

fn stable_candidate_config() -> BindPortConfig {
    BindPortConfig {
        project: Some(String::from("contract-project")),
        service: Some(String::from("web")),
        default_range: Some(String::from("29000-29999")),
        skip_ports: Some(vec![29_000_u16, 29_070_u16]),
        services: Some(vec![ServiceConfig {
            name: Some(String::from("web")),
            path: Some(String::from("apps/web")),
            command: Some(vec![String::from("storybook"), String::from("dev")]),
            args: Some(vec![String::from("--port"), String::from("{port}")]),
            env: Some(BTreeMap::<String, String>::from([
                (String::from("PORT"), String::from("{port}")),
                (String::from("SAFE_FLAG"), String::from("enabled")),
            ])),
            hostname: Some(String::from("{service}.localhost")),
            route_url: Some(String::from("https://{hostname}")),
            health_url: Some(String::from("{route_url}/health")),
        }]),
        dashboard: Some(DashboardConfig {
            host: Some(String::from("127.0.0.1")),
            port: Some(27_080_u16),
            register_service: Some(true),
            allowed_hosts: Some(vec![String::from("localhost"), String::from("127.0.0.1")]),
            auth: Some(DashboardAuthConfig {
                required: Some(true),
                token: Some(String::from("local-test-token")),
                token_env: Some(String::from("BINDPORT_DASHBOARD_TOKEN")),
            }),
        }),
        output_defaults: Some(OutputDefaultsConfig {
            root: Some(String::from(".bindport/generated")),
            target_host: Some(String::from("127.0.0.1")),
            target_scheme: Some(String::from("http")),
            auto_render: Some(true),
            delete_on: Some(vec![
                OutputDeleteState::Stopped,
                OutputDeleteState::Stale,
                OutputDeleteState::Removed,
            ]),
            on_failure: Some(OutputFailurePolicy::Warn),
            debounce_ms: Some(250_u64),
        }),
        outputs: Some(vec![OutputConfig {
            enabled: Some(true),
            name: Some(String::from("contract")),
            template: Some(String::from("contract-template")),
            root: Some(String::from(".bindport/contract")),
            target: Some(String::from("routes.json")),
            target_host: Some(String::from("host.docker.internal")),
            target_scheme: Some(String::from("https")),
            auto_render: Some(false),
            delete_on: Some(vec![OutputDeleteState::Removed]),
            on_failure: Some(OutputFailurePolicy::Block),
            debounce_ms: Some(500_u64),
            vars: Some(BTreeMap::<String, serde_json::Value>::from([
                (String::from("array"), serde_json::json!(["one", "two"])),
                (String::from("boolean"), serde_json::json!(true)),
                (String::from("float"), serde_json::json!(1.5)),
                (String::from("integer"), serde_json::json!(7)),
                (
                    String::from("object"),
                    serde_json::json!({"nested": "value"}),
                ),
                (String::from("string"), serde_json::json!("value")),
            ])),
        }]),
        hooks: Some(HooksConfig {
            timeout_ms: Some(5_000_u64),
            commands: Some(vec![HookCommandConfig {
                enabled: Some(true),
                name: Some(String::from("contract-hook")),
                events: Some(vec![
                    HookEvent::RouteStarted,
                    HookEvent::RouteFinished,
                    HookEvent::RoutesRemoved,
                    HookEvent::RoutesMarkedStale,
                    HookEvent::RenderRequested,
                    HookEvent::OutputRendered,
                ]),
                command: Some(vec![String::from("bindport"), String::from("render")]),
                timeout_ms: Some(2_000_u64),
            }]),
        }),
    }
}

fn property_names(value: &serde_json::Value) -> BTreeSet<&str> {
    value["properties"]
        .as_object()
        .expect("schema properties")
        .keys()
        .map(String::as_str)
        .collect()
}

fn definition_property_names<'a>(
    schema: &'a serde_json::Value,
    definition: &str,
) -> BTreeSet<&'a str> {
    property_names(&schema["$defs"][definition])
}

fn enum_strings(value: &serde_json::Value) -> BTreeSet<&str> {
    value["enum"]
        .as_array()
        .expect("schema enum")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

fn output_delete_state_name(value: OutputDeleteState) -> &'static str {
    match value {
        OutputDeleteState::Stopped => "stopped",
        OutputDeleteState::Stale => "stale",
        OutputDeleteState::Removed => "removed",
    }
}

fn output_failure_policy_name(value: OutputFailurePolicy) -> &'static str {
    match value {
        OutputFailurePolicy::Warn => "warn",
        OutputFailurePolicy::Block => "block",
    }
}

fn hook_event_name(value: HookEvent) -> &'static str {
    match value {
        HookEvent::RouteStarted => "route_started",
        HookEvent::RouteFinished => "route_finished",
        HookEvent::RoutesRemoved => "routes_removed",
        HookEvent::RoutesMarkedStale => "routes_marked_stale",
        HookEvent::RenderRequested => "render_requested",
        HookEvent::OutputRendered => "output_rendered",
    }
}
