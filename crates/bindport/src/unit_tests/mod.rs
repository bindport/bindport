// SPDX-License-Identifier: MIT

use super::*;
use bindport_core::HooksConfig;
use std::{collections::BTreeMap, sync::Mutex};

mod clean_open;
mod dashboard;
mod diagnostics;
mod dispatch;
mod hooks;
mod list;
mod render;
mod route_events;
mod run;
mod templates;

static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

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

fn temp_registry_path(name: &str) -> PathBuf {
    temp_test_dir(name).join("registry.sqlite")
}

fn current_process_command() -> String {
    env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| String::from("bindport test"))
}

fn with_default_registry_path<T>(path: &Path, callback: impl FnOnce() -> T) -> T {
    let _guard = TEST_ENV_LOCK.lock().expect("test env lock");
    let previous = env::var_os(REGISTRY_PATH_ENV);

    // SAFETY: these unit tests serialize process environment mutation with
    // TEST_ENV_LOCK and restore the previous value before returning.
    unsafe {
        env::set_var(REGISTRY_PATH_ENV, path);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
    match previous {
        Some(previous) => unsafe {
            env::set_var(REGISTRY_PATH_ENV, previous);
        },
        None => unsafe {
            env::remove_var(REGISTRY_PATH_ENV);
        },
    }

    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn with_state_home<T>(path: &Path, callback: impl FnOnce() -> T) -> T {
    let _guard = TEST_ENV_LOCK.lock().expect("test env lock");
    let previous = env::var_os("XDG_STATE_HOME");

    // SAFETY: these unit tests serialize process environment mutation with
    // TEST_ENV_LOCK and restore the previous value before returning.
    unsafe {
        env::set_var("XDG_STATE_HOME", path);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
    match previous {
        Some(previous) => unsafe {
            env::set_var("XDG_STATE_HOME", previous);
        },
        None => unsafe {
            env::remove_var("XDG_STATE_HOME");
        },
    }

    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn with_env_values<T>(updates: &[(&str, Option<&str>)], callback: impl FnOnce() -> T) -> T {
    let _guard = TEST_ENV_LOCK.lock().expect("test env lock");
    let previous = updates
        .iter()
        .map(|(name, _)| (*name, env::var_os(name)))
        .collect::<Vec<_>>();

    // SAFETY: these unit tests serialize process environment mutation with
    // TEST_ENV_LOCK and restore previous values before returning.
    unsafe {
        for (name, value) in updates {
            match value {
                Some(value) => env::set_var(*name, *value),
                None => env::remove_var(*name),
            }
        }
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
    unsafe {
        for (name, value) in previous {
            match value {
                Some(value) => env::set_var(name, value),
                None => env::remove_var(name),
            }
        }
    }

    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
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

fn test_status_snapshot(services: Vec<StatusService>) -> StatusSnapshot {
    StatusSnapshot {
        schema_version: bindport_registry::STATUS_SCHEMA_VERSION,
        generated_at: String::from("2026-06-29T00:02:00Z"),
        outputs: Vec::new(),
        services,
        runs: Vec::new(),
    }
}
