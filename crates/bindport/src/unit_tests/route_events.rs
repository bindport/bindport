// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn route_event_collector_retains_source_and_kind() {
    let empty = RouteEventCollector::default();
    assert!(empty.is_empty());
    assert_eq!(empty.warning_context(), "route event");

    let mut collector =
        RouteEventCollector::single(RouteEventSource::CliRunner, RouteEventKind::RouteStarted);
    collector.record(
        RouteEventSource::StaleReconcile,
        RouteEventKind::RoutesMarkedStale,
    );

    assert_eq!(
        collector.events(),
        &[
            RouteEvent::new(RouteEventSource::CliRunner, RouteEventKind::RouteStarted),
            RouteEvent::new(
                RouteEventSource::StaleReconcile,
                RouteEventKind::RoutesMarkedStale,
            )
        ]
    );
    assert_eq!(
        collector.warning_context(),
        "route events from cli_runner,stale_reconcile"
    );
    assert_eq!(collector.hook_sources(), "cli_runner,stale_reconcile");
    assert_eq!(
        collector.hook_events(false),
        BTreeSet::from([HookEvent::RouteStarted, HookEvent::RoutesMarkedStale,])
    );
    assert_eq!(
        collector.hook_events(true),
        BTreeSet::from([
            HookEvent::RouteStarted,
            HookEvent::RoutesMarkedStale,
            HookEvent::OutputRendered,
        ])
    );

    let single = RouteEventCollector::single(
        RouteEventSource::DashboardClean,
        RouteEventKind::RoutesRemoved,
    );
    assert_eq!(single.warning_context(), "dashboard_clean routes_removed");
    assert_eq!(RouteEventSource::CliClean.as_str(), "cli_clean");
    assert_eq!(RouteEventSource::ManualRender.as_str(), "manual_render");
    assert_eq!(RouteEventKind::RouteFinished.as_str(), "route_finished");
    assert_eq!(RouteEventKind::RenderRequested.as_str(), "render_requested");
}

#[test]
fn render_route_helpers_preserve_status_and_pending_metadata() {
    let services = vec![
        status_service("route-1", "active", Some("2026-06-29T00:05:00Z")),
        status_service("route-2", "stopped", None),
        status_service("route-3", "stale", None),
    ];

    let routes = route_records(services);

    assert_eq!(routes.len(), 3);
    assert_eq!(routes[0].key, "route-1");
    assert_eq!(routes[0].updated_at, "2026-06-29T00:05:00Z");
    assert_eq!(routes[1].state, "stopped");
    assert_eq!(routes[1].updated_at, "2026-06-29T00:00:00Z");
    assert_eq!(routes[2].state, "stale");
    assert_eq!(
        route_delete_state(&routes[0]),
        None,
        "active routes are not lifecycle delete candidates"
    );
    assert_eq!(
        route_delete_state(&routes[1]),
        Some(OutputDeleteState::Stopped)
    );
    assert_eq!(
        route_delete_state(&routes[2]),
        Some(OutputDeleteState::Stale)
    );

    let mut output = test_output_config("debug");
    output.delete_on = vec![OutputDeleteState::Stopped];
    assert_eq!(
        delete_route_keys(&output, &routes),
        BTreeSet::from([String::from("route-2")])
    );
    output.delete_on = vec![OutputDeleteState::Stopped, OutputDeleteState::Stale];
    assert_eq!(
        delete_route_keys(&output, &routes),
        BTreeSet::from([String::from("route-2"), String::from("route-3")])
    );

    let identity = ServiceIdentity {
        project: String::from("demo"),
        service: String::from("web"),
        git: Some(bindport_core::GitIdentity {
            worktree_path: PathBuf::from("/workspace/demo-feature-tree"),
            worktree_hash: String::from("abc123456789"),
            git_common_dir: PathBuf::from("/workspace/demo/.git"),
            branch: String::from("feature/tree"),
            branch_label: String::from("feature-tree"),
            commit: String::from("0123456789abcdef"),
        }),
        identity_key: String::from("v1:demo:web"),
    };
    let metadata = RunMetadata {
        command: None,
        hostname: Some(String::from("feature-tree.demo.localhost")),
        route_url: Some(String::from("https://feature-tree.demo.localhost")),
        health_url: Some(String::from("https://feature-tree.demo.localhost/health")),
        env: Vec::new(),
    };
    let pending = pending_route_record(
        &identity,
        29_100,
        &metadata,
        "next dev",
        Path::new("/workspace/demo"),
    );

    assert_eq!(pending.key, "v1:demo:web");
    assert_eq!(pending.state, "active");
    assert_eq!(pending.url, "http://127.0.0.1:29100");
    assert_eq!(
        pending.hostname.as_deref(),
        Some("feature-tree.demo.localhost")
    );
    assert_eq!(pending.branch.as_deref(), Some("feature/tree"));
    assert_eq!(pending.branch_label.as_deref(), Some("feature-tree"));
    assert_eq!(
        pending.worktree_path.as_deref(),
        Some("/workspace/demo-feature-tree")
    );
    assert_eq!(pending.pid, None);
    assert_eq!(pending.started_at, "pending");

    let cwd = Path::new("/workspace/demo/apps/web");
    let config_path = PathBuf::from("/workspace/demo/bindport.toml");
    let project_config = ResolvedConfig {
        loaded: Some(bindport_core::LoadedConfig {
            path: config_path.clone(),
            format: bindport_core::ConfigFormat::Toml,
            source: ConfigSource::Project,
            local_override: None,
            config: BindPortConfig::default(),
            unknown_keys: Vec::new(),
        }),
        fallback_path: None,
        port_range: DEFAULT_PORT_RANGE,
        skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
    };
    assert_eq!(
        output_base_dir(cwd, &project_config),
        Path::new("/workspace/demo")
    );

    let fallback_config = ResolvedConfig {
        loaded: Some(bindport_core::LoadedConfig {
            path: PathBuf::from("/home/user/.config/bindport/config.toml"),
            format: bindport_core::ConfigFormat::Toml,
            source: ConfigSource::Fallback,
            local_override: None,
            config: BindPortConfig::default(),
            unknown_keys: Vec::new(),
        }),
        fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
        port_range: DEFAULT_PORT_RANGE,
        skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
    };
    assert_eq!(output_base_dir(cwd, &fallback_config), cwd);
}
