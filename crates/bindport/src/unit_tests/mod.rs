// SPDX-License-Identifier: MIT

use super::*;
use bindport_core::HooksConfig;
use std::collections::BTreeMap;

mod clean_open;
mod dashboard;
mod diagnostics;
mod dispatch;
mod hooks;
mod render;
mod route_events;
mod run;
mod templates;

fn hook_command(name: &str) -> HookCommandConfig {
    HookCommandConfig {
        name: Some(name.to_string()),
        events: Some(vec![HookEvent::RouteStarted]),
        command: Some(vec![String::from("true")]),
        ..HookCommandConfig::default()
    }
}

fn hook_resolved_config(
    source: ConfigSource,
    hooks: HooksConfig,
    local_hooks: Option<HooksConfig>,
) -> ResolvedConfig {
    let mut config = BindPortConfig {
        hooks: Some(hooks),
        ..BindPortConfig::default()
    };
    let local_override = local_hooks.map(|hooks| {
        let local_config = BindPortConfig {
            hooks: Some(hooks),
            ..BindPortConfig::default()
        };
        config.merge_local_override(local_config.clone());

        bindport_core::LoadedLocalConfig {
            path: PathBuf::from("/workspace/demo/.bindport.local.toml"),
            format: bindport_core::ConfigFormat::Toml,
            git_tracked: false,
            config: local_config,
            unknown_keys: Vec::new(),
        }
    });

    ResolvedConfig {
        loaded: Some(bindport_core::LoadedConfig {
            path: match source {
                ConfigSource::Project => PathBuf::from("/workspace/demo/bindport.toml"),
                ConfigSource::Fallback => PathBuf::from("/home/user/.config/bindport/config.toml"),
            },
            format: bindport_core::ConfigFormat::Toml,
            source,
            local_override,
            config,
            unknown_keys: Vec::new(),
        }),
        fallback_path: None,
        port_range: DEFAULT_PORT_RANGE,
        skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
    }
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(String::from).collect()
}

fn temp_test_dir(name: &str) -> PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("bindport-cli-{name}-{}-{now}", std::process::id()));
    fs::create_dir_all(&path).expect("temp test dir");
    path
}

fn test_output_config(name: &str) -> EffectiveOutputConfig {
    EffectiveOutputConfig {
        name: name.to_string(),
        template: String::from("bindport-traefik"),
        root: None,
        target: String::from("{{ route.slug }}.yml"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: Vec::new(),
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 0,
        vars: BTreeMap::new(),
    }
}

fn status_service(identity_key: &str, state: &str, exited_at: Option<&str>) -> StatusService {
    StatusService {
        project: String::from("demo"),
        service: String::from("web"),
        state: state.to_string(),
        port: 29_100,
        host: String::from("127.0.0.1"),
        url: String::from("http://127.0.0.1:29100"),
        hostname: Some(String::from("feature-tree.demo.localhost")),
        route_url: Some(String::from("https://feature-tree.demo.localhost")),
        health_url: Some(String::from("https://feature-tree.demo.localhost/health")),
        worktree_path: Some(String::from("/workspace/demo-feature-tree")),
        worktree_hash: Some(String::from("abc123456789")),
        git_common_dir: Some(String::from("/workspace/demo/.git")),
        branch: Some(String::from("feature/tree")),
        branch_label: Some(String::from("feature-tree")),
        commit: Some(String::from("0123456789abcdef")),
        identity_key: Some(identity_key.to_string()),
        pid: Some(12_345),
        command: String::from("next dev"),
        cwd: String::from("/workspace/demo"),
        started_at: String::from("2026-06-29T00:00:00Z"),
        exited_at: exited_at.map(str::to_string),
        exit_code: None,
        health: String::from("unknown"),
        outputs: Vec::new(),
        proxy: None,
    }
}
