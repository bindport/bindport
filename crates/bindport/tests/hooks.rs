// SPDX-License-Identifier: MIT

mod support;

use support::*;

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
#[cfg(unix)]
#[test]
fn hook_timeout_kills_spawned_process_group() {
    let registry_path = temp_registry_path("hooks-timeout-group-registry");
    let root = temp_test_dir("hooks-timeout-group-root");
    let leaked_marker = root.join("leaked-child");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-timeout-group\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 100\n[[hooks.commands]]\nname = \"leaky\"\nevents = [\"route_started\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("sh -c 'sleep 0.5; printf leaked > \"$1\"' sh \"$1\" & sleep 10"),
            toml_string(&leaked_marker.display().to_string()),
        ),
    )
    .expect("write hook config");

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "leaky"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

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
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("hook `sh -c"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("timed out after 100ms"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    thread::sleep(Duration::from_millis(800));
    assert!(
        !leaked_marker.exists(),
        "timed-out hook left a background child running"
    );
}
#[cfg(unix)]
#[test]
fn repo_scoped_hook_trust_applies_to_matching_worktree() {
    let registry_path = temp_registry_path("hooks-repo-scope-registry");
    let repo = temp_test_dir("hooks-repo-scope-root");
    let port = free_loopback_port();
    init_git_repo(&repo, "main");
    write_executable(
        &repo.join("reload-hook"),
        "#!/bin/sh\nprintf 'repo:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> hook.log\n",
    );
    fs::write(
        repo.join(".bindport.toml"),
        format!("project = \"hooks-repo-scope\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./reload-hook\"]\n"),
    )
    .expect("write hook config");
    run_git(&repo, ["add", ".bindport.toml", "reload-hook"]);
    run_git(&repo, ["commit", "-m", "add hook fixture"]);

    let worktree = temp_test_dir("hooks-repo-scope-worktree");
    fs::remove_dir(&worktree).expect("remove empty temp dir before worktree add");
    let worktree_arg = worktree.display().to_string();
    run_git(
        &repo,
        ["worktree", "add", &worktree_arg, "-b", "feature/reuse"],
    );

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&repo)
        .args(["hooks", "trust", "--scope", "repo", "reload"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let run = bindport_with_registry(&registry_path)
        .current_dir(&worktree)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport from second worktree");
    assert!(
        run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        fs::read_to_string(worktree.join("hook.log"))
            .expect("worktree hook log")
            .contains("repo:route_started")
    );
}
#[test]
fn render_dry_run_reports_hooks_without_running_them() {
    let registry_path = temp_registry_path("hooks-dry-run-registry");
    let root = temp_test_dir("hooks-dry-run-root");
    let hook_log = root.join("hook.log");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"hooks-dry-run\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"dry-run.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"render-log\"\nevents = [\"render_requested\", \"output_rendered\"]\ncommand = [\"sh\", \"-c\", {}, \"sh\", {}]\n",
            toml_string("printf hook >> \"$1\""),
            toml_string(&hook_log.display().to_string()),
        ),
    )
    .expect("write hook config");
    let trust = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hooks", "trust", "render-log"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let run_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");
    assert!(
        run_output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&run_output.stderr)
    );

    let dry_run = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["render", "--dry-run"])
        .output()
        .expect("dry-run render");

    assert!(
        dry_run.status.success(),
        "render dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let stdout = String::from_utf8(dry_run.stdout).expect("dry-run stdout");

    assert!(stdout.contains("would render traefik: 1 files"));
    assert!(stdout.contains("would run hook render-log"));
    assert!(stdout.contains("BINDPORT_HOOK_CONTEXT=<redacted>"));
    assert!(!hook_log.exists());
}
