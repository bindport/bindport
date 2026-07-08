// SPDX-License-Identifier: MIT

use super::*;
use bindport_core::{OutputDeleteState, OutputFailurePolicy};
use std::collections::BTreeMap;

mod cleanup;
mod plan;
mod render;
mod templates;

fn temp_test_dir(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("bindport-{name}-{unique}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn test_render_plan(target: &str, contents: &str) -> RenderPlan {
    RenderPlan {
        output: OutputContext {
            name: String::from("debug"),
            template: String::from("debug-template"),
            root: Some(String::from(".bindport/out")),
            target: String::from("routes/{{ route.slug }}.yml"),
            auto_render: true,
            delete_on: vec![String::from("removed")],
            on_failure: String::from("warn"),
        },
        files: vec![RenderedRouteFile {
            route_key: String::from("route-1"),
            target: target.to_string(),
            contents: contents.to_string(),
            context: RenderContext {
                snapshot: SnapshotContext {
                    generated_at: String::from("2026-06-29T00:02:00Z"),
                    route_count: 1,
                },
                route: RouteContext {
                    key: String::from("route-1"),
                    project: String::from("demo"),
                    service: String::from("web"),
                    state: String::from("active"),
                    health: String::from("unknown"),
                    port: 29_100,
                    host: String::from("127.0.0.1"),
                    url: String::from("http://127.0.0.1:29100"),
                    hostname: Some(String::from("demo.localhost")),
                    route_url: Some(String::from("http://demo.localhost")),
                    target_url: String::from("http://127.0.0.1:29100"),
                    branch: Some(String::from("feature/tree")),
                    branch_label: Some(String::from("feature-tree")),
                    worktree_path: Some(String::from("/workspace/demo-feature-tree")),
                    worktree_label: String::from("demo-feature-tree"),
                    worktree_hash: Some(String::from("abc123456789")),
                    slug: String::from("demo-web-feature-tree"),
                    unique_slug: String::from("demo-web-feature-tree-abc12345"),
                    pid: Some(12_345),
                    command: String::from("next dev"),
                    cwd: String::from("/workspace/demo-feature-tree"),
                    started_at: String::from("2026-06-29T00:00:00Z"),
                    updated_at: String::from("2026-06-29T00:01:00Z"),
                },
                output: OutputContext {
                    name: String::from("debug"),
                    template: String::from("debug-template"),
                    root: Some(String::from(".bindport/out")),
                    target: String::from("routes/{{ route.slug }}.yml"),
                    auto_render: true,
                    delete_on: vec![String::from("removed")],
                    on_failure: String::from("warn"),
                },
                vars: BTreeMap::new(),
            },
        }],
    }
}

fn test_route(key: &str, state: &str, hostname: Option<&str>) -> RouteRecord {
    RouteRecord {
        key: key.to_string(),
        project: String::from("demo"),
        service: String::from("web"),
        state: state.to_string(),
        health: String::from("unknown"),
        port: 29_100,
        host: String::from("127.0.0.1"),
        url: String::from("http://127.0.0.1:29100"),
        hostname: hostname.map(str::to_string),
        route_url: hostname.map(|hostname| format!("http://{hostname}")),
        branch: Some(String::from("feature/tree")),
        branch_label: Some(String::from("feature-tree")),
        worktree_path: Some(String::from("/workspace/demo-feature-tree")),
        worktree_hash: Some(String::from("abc123456789")),
        pid: Some(12_345),
        command: String::from("next dev"),
        cwd: String::from("/workspace/demo-feature-tree"),
        started_at: String::from("2026-06-29T00:00:00Z"),
        updated_at: String::from("2026-06-29T00:01:00Z"),
    }
}

fn test_route_snapshot(routes: Vec<RouteRecord>) -> OutputRouteSnapshot {
    OutputRouteSnapshot::new("2026-06-29T00:02:00Z", routes)
}
