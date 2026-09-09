// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn built_in_traefik_template_renders_active_route() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-traefik", None)
        .expect("built-in template");
    let rendered = render_template(
        &template.contents,
        minijinja::context! {
            route => minijinja::context! {
                key => "demo:web:feature",
                state => "active",
                hostname => "feature.demo.localhost",
                slug => "demo-web-feature",
                unique_slug => "demo-web-feature-abc12345",
                target_url => "http://127.0.0.1:29100",
            },
            vars => minijinja::context! {},
        },
    )
    .expect("built-in template renders");

    assert!(rendered.contains("routers:\n    demo-web-feature-abc12345:\n"));
    assert!(rendered.contains("service: \"demo-web-feature-abc12345\"\n"));
    assert!(rendered.contains("services:\n    demo-web-feature-abc12345:\n"));
    assert!(rendered.contains("rule: \"Host(`feature.demo.localhost`)\""));
    assert!(rendered.contains("url: \"http://127.0.0.1:29100\""));
}

#[test]
fn built_in_traefik_template_escapes_yaml_scalars() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-traefik", None)
        .expect("built-in template");
    let rendered = render_template(
        &template.contents,
        minijinja::context! {
            route => minijinja::context! {
                key => "demo:web:feature",
                state => "active",
                hostname => "feature\".demo.localhost",
                slug => "demo-web-feature",
                unique_slug => "demo-web-feature-abc12345",
                target_url => "http://127.0.0.1:29100/path\"",
            },
            vars => minijinja::context! {
                entrypoints => ["web\nbad"],
                middlewares => ["auth\"middleware"],
            },
        },
    )
    .expect("built-in template renders");

    assert!(rendered.contains("rule: \"Host(`feature\\\".demo.localhost`)\""));
    assert!(rendered.contains("- \"web\\nbad\""));
    assert!(rendered.contains("- \"auth\\\"middleware\""));
    assert!(rendered.contains("url: \"http://127.0.0.1:29100/path\\\"\""));
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
fn built_in_traefik_ids_distinguish_worktrees_with_the_same_slug() {
    let first = test_route("route-1", "active", Some("first.demo.localhost"));
    let mut second = test_route("route-2", "active", Some("second.demo.localhost"));
    second.worktree_path = Some(String::from("/workspace/another/demo-feature-tree"));
    second.worktree_hash = Some(String::from("def678901234"));
    second.port = 29_101;
    second.url = String::from("http://127.0.0.1:29101");

    assert_distinct_route_ids(first, second);
}

#[test]
fn built_in_traefik_ids_distinguish_non_git_routes_with_the_same_slug() {
    let mut first = test_route(
        "first-directory:web",
        "active",
        Some("first.demo.localhost"),
    );
    first.branch = None;
    first.branch_label = None;
    first.worktree_path = None;
    first.worktree_hash = None;
    let mut second = first.clone();
    second.key = String::from("second-directory:web");
    second.cwd = String::from("/workspace/another/demo");
    second.hostname = Some(String::from("second.demo.localhost"));
    second.route_url = Some(String::from("http://second.demo.localhost"));
    second.port = 29_101;
    second.url = String::from("http://127.0.0.1:29101");

    assert_distinct_route_ids(first, second);
}

fn assert_distinct_route_ids(first: RouteRecord, second: RouteRecord) {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-traefik", None)
        .expect("built-in template");
    let output = OutputRenderConfig::from(&EffectiveOutputConfig {
        name: String::from("traefik"),
        template: String::from("bindport-traefik"),
        root: None,
        target: String::from("traefik/{{ route.unique_slug }}.yml"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: vec![OutputDeleteState::Removed],
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 250,
        vars: BTreeMap::new(),
    });
    let snapshot = test_route_snapshot(vec![first, second]);
    let plan = render_output_plan(&output, &template.contents, &snapshot).expect("plan");

    assert_eq!(plan.files.len(), 2);
    let first = &plan.files[0].context.as_ref().expect("first context").route;
    let second = &plan.files[1]
        .context
        .as_ref()
        .expect("second context")
        .route;
    assert_eq!(first.slug, second.slug);
    assert_ne!(first.unique_slug, second.unique_slug);
    assert_ne!(plan.files[0].target, plan.files[1].target);
    for file in &plan.files {
        let route = &file.context.as_ref().expect("route context").route;
        let id = &route.unique_slug;
        assert_eq!(file.target, format!("traefik/{id}.yml"));
        assert!(file.contents.contains(&format!("routers:\n    {id}:\n")));
        assert!(file.contents.contains(&format!("service: \"{id}\"\n")));
        assert!(file.contents.contains(&format!("services:\n    {id}:\n")));
        assert!(file.contents.contains(&format!(
            "rule: \"Host(`{}`)\"",
            route.hostname.as_ref().expect("hostname")
        )));
        assert!(
            file.contents
                .contains(&format!("url: \"{}\"", route.target_url))
        );
    }
    let repeated = render_output_plan(&output, &template.contents, &snapshot).expect("repeat");
    assert_eq!(plan, repeated);
}
