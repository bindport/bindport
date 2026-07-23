// SPDX-License-Identifier: MIT

use crate::support::*;
use bindport_registry::{OutputFileRecord, OutputFileScope, OutputFileStatus, ReserveLease};

fn status_schema() -> Value {
    serde_json::from_str(include_str!("../../../../../docs/status.schema.json"))
        .expect("status schema json")
}

#[test]
fn status_schema_document_freezes_the_v1_contract() {
    let schema = status_schema();

    assert_eq!(schema["properties"]["schema_version"]["const"], "1.0");
    assert_eq!(schema["additionalProperties"], true);
    assert_eq!(
        schema["$defs"]["service"]["properties"]["state"]["enum"],
        serde_json::json!(["active", "reserved", "stopped", "stale"])
    );
    assert_eq!(
        schema["$defs"]["service"]["properties"]["health"]["enum"],
        serde_json::json!(["unknown", "pending", "healthy", "failing"])
    );
    assert_eq!(
        schema["$defs"]["serviceOutput"]["properties"]["status"]["enum"],
        serde_json::json!(["pending", "rendered", "removed", "error"])
    );
    assert_eq!(
        schema["$defs"]["proxy"]["properties"]["target"]["type"],
        "string"
    );

    for definition in schema["$defs"]
        .as_object()
        .expect("schema definitions")
        .values()
    {
        if definition["type"] == "object" {
            assert_eq!(
                definition["additionalProperties"], true,
                "v1 objects must permit additive fields"
            );
        }
    }
}

#[test]
fn representative_status_payload_matches_every_documented_shape() {
    let schema = status_schema();
    let registry_path = temp_registry_path("status-v1-contract");
    let root = temp_test_dir("status-v1-contract-root")
        .canonicalize()
        .expect("canonical contract root");
    fs::create_dir_all(root.join(".git")).expect("git directory");
    fs::create_dir_all(root.join("outputs")).expect("output directory");
    fs::write(
        root.join(".bindport.toml"),
        r#"project = "contract-project"

[hooks]
timeout_ms = 1500

[[hooks.commands]]
name = "contract-hook"
events = ["route_started", "routes_removed"]
command = ["contract-hook-program", "--reload"]
"#,
    )
    .expect("write contract config");

    let listeners = (0..4)
        .map(|_| TcpListener::bind(("127.0.0.1", 0)).expect("test-owned port"))
        .collect::<Vec<_>>();
    let ports = listeners
        .iter()
        .map(|listener| listener.local_addr().expect("listener address").port())
        .collect::<Vec<_>>();
    let active_identity = ServiceIdentity {
        project: String::from("contract-project"),
        service: String::from("active"),
        git: Some(bindport_core::GitIdentity {
            worktree_path: root.clone(),
            worktree_hash: String::from("active-worktree-hash"),
            git_common_dir: root.join(".git"),
            branch: String::from("feature/status-contract"),
            branch_label: String::from("feature-status-contract"),
            commit: String::from("0123456789abcdef"),
        }),
        identity_key: String::from("v1:contract-active"),
    };
    let mut registry = Registry::open(&registry_path).expect("contract registry");
    let active = registry
        .record_run_started(&RunStart {
            project: active_identity.project.clone(),
            service: active_identity.service.clone(),
            identity: Some(active_identity.clone()),
            host: String::from("127.0.0.1"),
            port: ports[0],
            hostname: Some(String::from("active.contract.localhost")),
            route_url: Some(String::from("https://active.contract.localhost")),
            health_url: Some(String::from("https://active.contract.localhost/health")),
            pid: std::process::id(),
            command: current_process_command(),
            cwd: root.clone(),
        })
        .expect("active run");

    let stopped_identity = ServiceIdentity {
        project: String::from("contract-project"),
        service: String::from("stopped"),
        git: None,
        identity_key: String::from("v1:contract-stopped"),
    };
    let stopped = registry
        .record_run_started(&RunStart {
            project: stopped_identity.project.clone(),
            service: stopped_identity.service.clone(),
            identity: Some(stopped_identity.clone()),
            host: String::from("127.0.0.1"),
            port: ports[1],
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: root.clone(),
        })
        .expect("stopped run");
    registry
        .record_run_finished(stopped, Some(7))
        .expect("finish stopped run");

    let stale_identity = ServiceIdentity {
        project: String::from("contract-project"),
        service: String::from("stale"),
        git: None,
        identity_key: String::from("v1:contract-stale"),
    };
    let stale = registry
        .record_run_started(&RunStart {
            project: stale_identity.project.clone(),
            service: stale_identity.service.clone(),
            identity: Some(stale_identity.clone()),
            host: String::from("127.0.0.1"),
            port: ports[2],
            hostname: None,
            route_url: None,
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale contract fixture"),
            cwd: root.clone(),
        })
        .expect("stale run");

    let reserved_identity = ServiceIdentity {
        project: String::from("contract-project"),
        service: String::from("reserved"),
        git: None,
        identity_key: String::from("v1:contract-reserved"),
    };
    let reserved = registry
        .record_reserved_lease(&ReserveLease {
            project: reserved_identity.project.clone(),
            service: reserved_identity.service.clone(),
            identity: Some(reserved_identity.clone()),
            host: String::from("127.0.0.1"),
            port: ports[3],
            hostname: None,
            route_url: None,
            health_url: None,
        })
        .expect("reserved lease");

    let scope = OutputFileScope::new(root.join("outputs"), root.clone(), Some(root.clone()), None);
    for (name, route_key, path, status, reason, lease_id, run_id) in [
        (
            "traefik",
            active_identity.identity_key.as_str(),
            "active.yml",
            OutputFileStatus::Rendered,
            None,
            active.lease_id,
            Some(active.run_id),
        ),
        (
            "caddy",
            active_identity.identity_key.as_str(),
            "active.caddy",
            OutputFileStatus::Error,
            Some(String::from("template_error")),
            active.lease_id,
            Some(active.run_id),
        ),
        (
            "json",
            reserved_identity.identity_key.as_str(),
            "reserved.json",
            OutputFileStatus::Pending,
            None,
            reserved.lease_id,
            None,
        ),
        (
            "env",
            stale_identity.identity_key.as_str(),
            "stale.env",
            OutputFileStatus::Removed,
            Some(String::from("route_removed")),
            stale.lease_id,
            Some(stale.run_id),
        ),
    ] {
        registry
            .record_output_file(&OutputFileRecord {
                output_name: name.to_string(),
                scope: scope.clone(),
                route_key: route_key.to_string(),
                rendered_path: root.join("outputs").join(path),
                status,
                reason,
                content_hash: Some(format!("{name}-content-hash")),
                template_hash: Some(format!("{name}-template-hash")),
                lease_id: Some(lease_id),
                run_id,
            })
            .expect("record contract output");
    }
    drop(registry);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env("XDG_STATE_HOME", root.join("state"))
        .args(["status", "--json"])
        .output()
        .expect("run representative status");
    assert!(
        output.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = serde_json::from_slice::<Value>(&output.stdout).expect("status json");

    validate_schema(&status, &schema, &schema).unwrap_or_else(|error| panic!("{error}"));
    assert_current_shape(&status, &schema, &schema);
    assert_eq!(status["schema_version"], "1.0");
    assert_eq!(status["services"].as_array().expect("services").len(), 4);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 3);
    assert_eq!(status["hooks"]["items"].as_array().expect("hooks").len(), 1);
    assert_eq!(status["hooks"]["items"][0]["status"], "pending");
    assert_eq!(status["hooks"]["items"][0]["target"]["kind"], "opaque");

    let states = status["services"]
        .as_array()
        .expect("services")
        .iter()
        .map(|service| service["state"].as_str().expect("state"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        states,
        BTreeSet::from(["active", "reserved", "stale", "stopped"])
    );
    let output_states = status["services"]
        .as_array()
        .expect("services")
        .iter()
        .flat_map(|service| service["outputs"].as_array().expect("service outputs"))
        .map(|output| output["status"].as_str().expect("output status"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        output_states,
        BTreeSet::from(["error", "pending", "removed", "rendered"])
    );
    let active_service = status["services"]
        .as_array()
        .expect("services")
        .iter()
        .find(|service| service["service"] == "active")
        .expect("active service");
    assert!(matches!(
        active_service["health"].as_str(),
        Some("pending" | "unknown")
    ));
    assert_eq!(active_service["proxy"]["adapter"], "traefik");
    let reserved_service = status["services"]
        .as_array()
        .expect("services")
        .iter()
        .find(|service| service["service"] == "reserved")
        .expect("reserved service");
    for nullable in [
        "hostname",
        "route_url",
        "health_url",
        "worktree_path",
        "pid",
        "exited_at",
        "exit_code",
        "proxy",
    ] {
        assert_eq!(reserved_service[nullable], Value::Null, "{nullable}");
    }

    fs::write(root.join(".bindport.toml"), "not = [valid").expect("write invalid config");
    let error_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env("XDG_STATE_HOME", root.join("state"))
        .args(["status", "--json"])
        .output()
        .expect("run status with config error");
    assert!(error_output.status.success());
    let error_status = serde_json::from_slice::<Value>(&error_output.stdout).expect("error status");
    validate_schema(&error_status, &schema, &schema).unwrap_or_else(|error| panic!("{error}"));
    assert_current_shape(&error_status, &schema, &schema);
    assert!(error_status["hooks"]["error"].is_string());
    assert!(
        error_status["hooks"]["items"]
            .as_array()
            .expect("items")
            .is_empty()
    );
}

fn validate_schema(instance: &Value, schema: &Value, root: &Value) -> Result<(), String> {
    let schema = resolve_schema(schema, root)?;
    if let Some(choices) = schema.get("anyOf").and_then(Value::as_array) {
        if choices
            .iter()
            .any(|choice| validate_schema(instance, choice, root).is_ok())
        {
            return Ok(());
        }
        return Err(format!("{instance:?} did not match anyOf {choices:?}"));
    }
    if let Some(expected) = schema.get("const")
        && instance != expected
    {
        return Err(format!("expected const {expected:?}, got {instance:?}"));
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array)
        && !values.contains(instance)
    {
        return Err(format!("{instance:?} is not in enum {values:?}"));
    }
    if let Some(expected_type) = schema.get("type") {
        let matches = expected_type
            .as_str()
            .is_some_and(|kind| value_has_type(instance, kind))
            || expected_type.as_array().is_some_and(|kinds| {
                kinds.iter().any(|kind| {
                    kind.as_str()
                        .is_some_and(|kind| value_has_type(instance, kind))
                })
            });
        if !matches {
            return Err(format!("{instance:?} does not have type {expected_type:?}"));
        }
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
        && instance.as_f64().is_some_and(|value| value < minimum)
    {
        return Err(format!("{instance:?} is below minimum {minimum}"));
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
        && instance.as_f64().is_some_and(|value| value > maximum)
    {
        return Err(format!("{instance:?} is above maximum {maximum}"));
    }
    if let Some(object) = instance.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for field in required {
                let field = field.as_str().ok_or("required field is not a string")?;
                if !object.contains_key(field) {
                    return Err(format!("missing required field `{field}` in {instance:?}"));
                }
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, value) in object {
                if let Some(property_schema) = properties.get(name) {
                    validate_schema(value, property_schema, root)
                        .map_err(|error| format!("{name}: {error}"))?;
                } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                    return Err(format!("unexpected field `{name}`"));
                }
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), instance.as_array()) {
        for (index, value) in values.iter().enumerate() {
            validate_schema(value, items, root).map_err(|error| format!("[{index}]: {error}"))?;
        }
    }

    Ok(())
}

fn assert_current_shape(instance: &Value, schema: &Value, root: &Value) {
    let schema = resolve_schema(schema, root).expect("resolved schema");
    if let Some(choices) = schema.get("anyOf").and_then(Value::as_array) {
        let choice = choices
            .iter()
            .find(|choice| validate_schema(instance, choice, root).is_ok())
            .expect("matching schema branch");
        assert_current_shape(instance, choice, root);
        return;
    }
    if let (Some(object), Some(properties)) = (
        instance.as_object(),
        schema.get("properties").and_then(Value::as_object),
    ) {
        for (name, value) in object {
            let property_schema = properties
                .get(name)
                .unwrap_or_else(|| panic!("producer field `{name}` is missing from status schema"));
            assert_current_shape(value, property_schema, root);
        }
    }
    if let (Some(values), Some(items)) = (instance.as_array(), schema.get("items")) {
        for value in values {
            assert_current_shape(value, items, root);
        }
    }
}

fn resolve_schema<'a>(mut schema: &'a Value, root: &'a Value) -> Result<&'a Value, String> {
    while let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = reference
            .strip_prefix('#')
            .ok_or_else(|| format!("unsupported schema reference `{reference}`"))?;
        schema = root
            .pointer(pointer)
            .ok_or_else(|| format!("missing schema reference `{reference}`"))?;
    }
    Ok(schema)
}

fn value_has_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}
