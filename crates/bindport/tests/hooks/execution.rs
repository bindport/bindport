// SPDX-License-Identifier: MIT

use crate::support::*;

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
