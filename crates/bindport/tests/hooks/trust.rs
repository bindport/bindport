// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn project_config_hooks_require_cli_trust() {
    let registry_path = temp_registry_path("hooks-untrusted-registry");
    let root = temp_test_dir("hooks-untrusted-root");
    let hook_log = root.join("hook.log");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-untrusted\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"route-log\"\nevents = [\"route_started\", \"route_finished\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("printf hook >> \"$1\""),
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");
    fs::write(
        root.join(".bindport.local.toml"),
        "[hooks]\ntrusted = true\n",
    )
    .expect("write ignored local trust config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!hook_log.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("hook `route-log` not run (pending approval)"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn trusted_local_hooks_run_for_route_events() {
    let registry_path = temp_registry_path("hooks-trusted-registry");
    let root = temp_test_dir("hooks-trusted-root");
    let hook_log = root.join("hook.log");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-trusted\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"route-log\"\nevents = [\"route_started\", \"route_finished\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("printf '%s|%s\\n' \"$BINDPORT_HOOK_EVENTS\" \"$BINDPORT_HOOK_SOURCES\" >> \"$1\""),
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");
    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "route-log"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );
    assert!(String::from_utf8_lossy(&trust.stdout).contains("approved 1 hook(s)"));

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let hook_output = fs::read_to_string(&hook_log).expect("hook output");

    assert!(hook_output.contains("route_started|cli_runner"));
    assert!(hook_output.contains("route_finished|cli_runner"));
}

#[cfg(unix)]
#[test]
fn trusted_project_hook_runs_from_config_root_when_invoked_from_service_dir() {
    let registry_path = temp_registry_path("hooks-config-root-registry");
    let root = temp_test_dir("hooks-config-root-root");
    init_git_repo(&root, "main");
    let service_dir = root.join("apps").join("web");
    let hook_dir = root.join("ops").join("localhost").join("bindport");
    fs::create_dir_all(&service_dir).expect("create service dir");
    fs::create_dir_all(&hook_dir).expect("create hook dir");
    write_executable(
        &hook_dir.join("reload.sh"),
        "#!/bin/sh\nprintf '%s|%s\\n' \"$BINDPORT_HOOK_EVENTS\" \"$(pwd)\" >> hook.log\n",
    );
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-config-root\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./ops/localhost/bindport/reload.sh\"]\n",
        ),
    )
    .expect("write hook config");

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "reload"])
        .output()
        .expect("trust hook from repo root");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let output = bindport_with_registry(&registry_path)
        .current_dir(&service_dir)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport from service dir");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let hook_output = fs::read_to_string(root.join("hook.log")).expect("hook output");

    assert!(hook_output.contains("route_started"));
    assert!(hook_output.contains(root.to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
fn project_hook_trust_uses_config_root_without_git() {
    let registry_path = temp_registry_path("hooks-config-root-no-git-registry");
    let root = temp_test_dir("hooks-config-root-no-git-root");
    let service_dir = root.join("apps").join("web");
    let hook_dir = root.join("ops");
    fs::create_dir_all(&service_dir).expect("create service dir");
    fs::create_dir_all(&hook_dir).expect("create hook dir");
    write_executable(
        &hook_dir.join("reload.sh"),
        "#!/bin/sh\nprintf '%s|%s\\n' \"$BINDPORT_HOOK_EVENTS\" \"$(pwd)\" >> hook.log\n",
    );
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-config-root-no-git\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./ops/reload.sh\"]\n",
        ),
    )
    .expect("write hook config");

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "reload"])
        .output()
        .expect("trust hook from project root");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let output = bindport_with_registry(&registry_path)
        .current_dir(&service_dir)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport from service dir");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let hook_output = fs::read_to_string(root.join("hook.log")).expect("hook output");

    assert!(hook_output.contains("route_started"));
    assert!(hook_output.contains(root.to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
fn trusted_hook_invalidates_when_local_target_changes() {
    let registry_path = temp_registry_path("hooks-target-change-registry");
    let root = temp_test_dir("hooks-target-change-root");
    let hook_log = root.join("hook.log");
    let hook_script = root.join("reload-hook");
    let port = free_loopback_port();
    write_executable(
        &hook_script,
        "#!/bin/sh\nprintf 'v1:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> \"$1\"\n",
    );
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-target-change\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./reload-hook\", {}]\n",
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "reload"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let first_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");
    assert!(
        first_run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&first_run.stderr)
    );
    assert!(
        fs::read_to_string(&hook_log)
            .expect("hook log")
            .contains("v1:route_started")
    );

    write_executable(
        &hook_script,
        "#!/bin/sh\nprintf 'v2:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> \"$1\"\n",
    );
    let status = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "status"])
        .output()
        .expect("hook status");
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("changed\treload"));

    let second_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");
    assert!(
        second_run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&second_run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second_run.stderr)
            .contains("hook `reload` not run (changed since the last trust decision)"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&second_run.stderr)
    );
    assert!(
        !fs::read_to_string(&hook_log)
            .expect("hook log")
            .contains("v2:route_started")
    );
}
