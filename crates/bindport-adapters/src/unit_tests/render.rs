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

    let plan = render_output_routes(
        &output,
        "target={{ route.target_url }} mode={{ vars.mode }} output={{ output.name }}",
        &[route],
    )
    .expect("render plan");

    assert_eq!(plan.output.name, "debug");
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].target, "debug/demo-web-feature-tree.txt");
    assert_eq!(
        plan.files[0].contents,
        "target=https://host.docker.internal:29100 mode=dev output=debug"
    );
    assert_eq!(
        plan.files[0].context.route.unique_slug,
        "demo-web-feature-tree-abc12345"
    );
    assert_eq!(plan.files[0].context.output.delete_on, vec!["removed"]);
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

    let error = render_output_routes(&output, "ok", &[first, second]).expect_err("collision");

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

    let error = render_output_routes(&output, "ok", &[route]).expect_err("unsafe hostname");

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

    let target_error =
        render_output_routes(&output, "ok", std::slice::from_ref(&route)).expect_err("target");
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
    let body_error = render_output_routes(&output, "{{ missing }}", &[route]).expect_err("body");
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

    let plan = render_output_routes(&output, &template.contents, &[route]).expect("plan");

    assert_eq!(plan.files[0].target, "traefik/demo-web-feature-tree.yml");
    assert!(plan.files[0].contents.contains("is stopped"));
    assert!(!plan.files[0].contents.contains("routers:"));
}
