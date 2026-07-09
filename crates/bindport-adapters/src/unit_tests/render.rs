// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn render_output_routes_builds_targets_and_context() {
    let mut vars = BTreeMap::new();
    vars.insert(String::from("mode"), serde_json::json!("dev"));
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("debug"),
        template: String::from("debug-template"),
        root: Some(String::from(".bindport/generated")),
        target: String::from("debug/{{ route.slug }}.txt"),
        target_host: String::from("host.docker.internal"),
        target_scheme: String::from("https"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars,
    });
    let route = test_route("route-1", "active", Some("feature-tree.demo.localhost"));
    let snapshot = test_route_snapshot(vec![route]);

    let plan = render_output_routes(
        &output,
        "target={{ route.target_url }} address={{ route.target_address }} scheme={{ route.target_scheme }} mode={{ vars.mode }} output={{ output.name }} snapshot={{ snapshot.generated_at }} routes={{ snapshot.route_count }}",
        &snapshot,
    )
    .expect("render plan");

    assert_eq!(plan.output.name, "debug");
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].target, "debug/demo-web-feature-tree.txt");
    assert_eq!(
        plan.files[0].contents,
        "target=https://host.docker.internal:29100 address=host.docker.internal:29100 scheme=https mode=dev output=debug snapshot=2026-06-29T00:02:00Z routes=1"
    );
    assert_eq!(
        plan.files[0]
            .context
            .as_ref()
            .expect("route context")
            .route
            .unique_slug,
        "demo-web-feature-tree-abc12345"
    );
    assert_eq!(
        plan.files[0]
            .context
            .as_ref()
            .expect("route context")
            .output
            .delete_on,
        vec!["removed"]
    );
}

#[test]
fn render_output_routes_reports_target_collisions() {
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("debug"),
        template: String::from("debug-template"),
        root: None,
        target: String::from("debug/{{ route.service }}.txt"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let first = test_route("route-1", "active", Some("first.demo.localhost"));
    let second = test_route("route-2", "active", Some("second.demo.localhost"));
    let snapshot = test_route_snapshot(vec![first, second]);

    let error = render_output_routes(&output, "ok", &snapshot).expect_err("collision");

    assert!(matches!(
        error,
        RenderError::TargetCollision { ref target, ref route_keys }
            if target == "debug/web.txt"
                && route_keys == &vec![String::from("route-1"), String::from("route-2")]
    ));
    assert!(std::error::Error::source(&error).is_none());
    assert_eq!(
        error.to_string(),
        "multiple routes render to target `debug/web.txt`: route-1, route-2"
    );
}

#[test]
fn render_output_routes_rejects_hostname_backticks() {
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("debug"),
        template: String::from("debug-template"),
        root: None,
        target: String::from("debug/{{ route.service }}.txt"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let route = test_route("route-1", "active", Some("x`) || PathPrefix(`/admin"));
    let snapshot = test_route_snapshot(vec![route]);

    let error = render_output_routes(&output, "ok", &snapshot).expect_err("unsafe hostname");

    assert!(matches!(
        error,
        RenderError::UnsafeHostname { ref route_key, ref hostname }
            if route_key == "route-1" && hostname.contains("PathPrefix")
    ));
    assert!(std::error::Error::source(&error).is_none());
    assert_eq!(
        error.to_string(),
        "route `route-1` has unsafe hostname `x`) || PathPrefix(`/admin`"
    );
}

#[test]
fn render_output_routes_reports_template_errors_with_sources() {
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("debug"),
        template: String::from("debug-template"),
        root: None,
        target: String::from("debug/{{ missing }}.txt"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let route = test_route("route-1", "active", Some("first.demo.localhost"));
    let snapshot = test_route_snapshot(vec![route]);

    let target_error = render_output_routes(&output, "ok", &snapshot).expect_err("target");
    assert!(matches!(
        target_error,
        RenderError::TargetTemplate { ref route_key, .. } if route_key == "route-1"
    ));
    assert!(std::error::Error::source(&target_error).is_some());
    assert!(
        target_error
            .to_string()
            .starts_with("failed to render target for route `route-1`")
    );

    let mut output = output;
    output.context.target = String::from("debug/{{ route.service }}.txt");
    let body_error = render_output_routes(&output, "{{ missing }}", &snapshot).expect_err("body");
    assert!(matches!(
        body_error,
        RenderError::BodyTemplate { ref route_key, .. } if route_key == "route-1"
    ));
    assert!(std::error::Error::source(&body_error).is_some());
    assert!(
        body_error
            .to_string()
            .starts_with("failed to render template for route `route-1`")
    );
}

#[test]
fn built_in_traefik_plan_renders_comment_for_stopped_route() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-traefik", None)
        .expect("built-in template");
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("traefik"),
        template: String::from("bindport-traefik"),
        root: None,
        target: String::from("traefik/{{ route.slug }}.yml"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let route = test_route("route-1", "stopped", Some("feature-tree.demo.localhost"));
    let snapshot = test_route_snapshot(vec![route]);

    let plan = render_output_routes(&output, &template.contents, &snapshot).expect("plan");

    assert_eq!(plan.files[0].target, "traefik/demo-web-feature-tree.yml");
    assert!(plan.files[0].contents.contains("is stopped"));
    assert!(!plan.files[0].contents.contains("routers:"));
}

#[test]
fn render_output_plan_writes_single_json_snapshot_file() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-json-snapshot", None)
        .expect("built-in template");
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("routes-json"),
        template: String::from("bindport-json-snapshot"),
        root: Some(String::from(".bindport/generated")),
        target: String::from("routes.json"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let first = test_route("route-1", "active", Some("first.demo.localhost"));
    let mut second = test_route("route-2", "stopped", Some("second.demo.localhost"));
    second.service = String::from("api");
    second.port = 29_101;
    let snapshot = test_route_snapshot(vec![first, second]);

    let plan = render_output_plan(&output, &template.contents, &snapshot).expect("snapshot plan");

    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].route_key, SNAPSHOT_OUTPUT_ROUTE_KEY);
    assert_eq!(plan.files[0].target, "routes.json");
    assert!(plan.files[0].context.is_none());
    let document =
        serde_json::from_str::<serde_json::Value>(&plan.files[0].contents).expect("snapshot json");
    assert_eq!(document["snapshot"]["route_count"], 2);
    assert_eq!(document["routes"][0]["hostname"], "first.demo.localhost");
    assert_eq!(document["routes"][1]["service"], "api");
}

#[test]
fn render_output_plan_writes_single_haproxy_file() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-haproxy", None)
        .expect("built-in template");
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("haproxy"),
        template: String::from("bindport-haproxy"),
        root: Some(String::from(".bindport/generated")),
        target: String::from("haproxy/bindport.cfg"),
        target_host: String::from("host.docker.internal"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let active = test_route("route-1", "active", Some("first.demo.localhost"));
    let mut stopped = test_route("route-2", "stopped", Some("second.demo.localhost"));
    stopped.service = String::from("api");
    stopped.port = 29_101;
    let snapshot = test_route_snapshot(vec![active, stopped]);

    let plan = render_output_plan(&output, &template.contents, &snapshot).expect("haproxy plan");

    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].route_key, SNAPSHOT_OUTPUT_ROUTE_KEY);
    assert_eq!(plan.files[0].target, "haproxy/bindport.cfg");
    assert!(plan.files[0].contents.contains("frontend bindport_http"));
    assert!(
        plan.files[0].contents.contains(
            "acl host_demo-web-feature-tree-abc12345 hdr(host) -i \"first.demo.localhost\""
        )
    );
    assert!(
        plan.files[0]
            .contents
            .contains("server demo-web-feature-tree-abc12345 \"host.docker.internal:29100\"")
    );
    assert!(plan.files[0].contents.contains("route-2 is stopped"));
}
