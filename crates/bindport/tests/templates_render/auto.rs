// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn runner_blocks_start_when_required_output_preflight_fails() {
    let registry_path = temp_registry_path("render-block-preflight-registry");
    let root = temp_test_dir("render-block-preflight-root");
    let port = free_loopback_port();
    let marker_path = root.join("child-ran");
    let marker_arg = marker_path.display().to_string();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"render-block\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"block.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"../blocked.yml\"\non_failure = \"block\"\n"
        ),
    )
    .expect("write render config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf ran > \"$1\"",
            "sh",
            &marker_arg,
        ])
        .output()
        .expect("run bindport");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsafe output target"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker_path.exists());
}
#[test]
fn runner_auto_renders_outputs_on_start_and_exit() {
    let registry_path = temp_registry_path("auto-render-registry");
    let root = temp_test_dir("auto-render-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"auto-render-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"auto.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
        ),
    )
    .expect("write render config");

    let rendered_path = root
        .join(".bindport")
        .join("generated")
        .join("traefik")
        .join("web.yml");
    let mut bindport = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "sleep 2"])
        .spawn()
        .expect("spawn bindport");

    let active_contents =
        wait_for_file_contains(&rendered_path, "routers:", Duration::from_secs(5));
    assert!(active_contents.contains("Host(`auto.localhost`)"));
    assert!(active_contents.contains(&format!("url: \"http://127.0.0.1:{port}\"")));

    let status = wait_for_child(&mut bindport, Duration::from_secs(5)).expect("bindport exits");
    assert!(status.success());

    let stopped_contents =
        wait_for_file_contains(&rendered_path, "is stopped", Duration::from_secs(5));
    assert!(!stopped_contents.contains("routers:"));
}
#[test]
fn runner_skips_outputs_when_auto_render_is_disabled() {
    let registry_path = temp_registry_path("auto-render-disabled-registry");
    let root = temp_test_dir("auto-render-disabled-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"auto-render-disabled\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"disabled.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\nauto_render = false\n"
        ),
    )
    .expect("write render config");

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
    assert!(!root.join(".bindport/generated/traefik/web.yml").exists());
}
#[test]
#[cfg(unix)]
fn runner_auto_renders_stale_routes_reconciled_during_route_event() {
    let registry_path = temp_registry_path("auto-render-stale-registry");
    let root = temp_test_dir("auto-render-stale-root");
    let stale_port = free_loopback_port();
    let active_port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"auto-render-stale\"\ndefault_range = \"{active_port}-{active_port}\"\nskip_ports = []\n[[services]]\nname = \"api\"\nhostname = \"api.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
        ),
    )
    .expect("write render config");

    let mut registry = Registry::open(&registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("auto-render-stale"),
        service: String::from("web"),
        git: None,
        identity_key: String::from("v1:auto-render-stale:web"),
    };
    registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port: stale_port,
            hostname: Some(String::from("web.localhost")),
            route_url: Some(String::from("http://web.localhost")),
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: root.clone(),
        })
        .expect("record stale fixture");
    drop(registry);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "api", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stale_contents = fs::read_to_string(root.join(".bindport/generated/traefik/web.yml"))
        .expect("stale route render");
    assert!(stale_contents.contains(" is stale, so no live router was rendered."));
    assert!(!stale_contents.contains("routers:"));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    assert!(status_output.status.success());
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let stale_service = services
        .iter()
        .find(|service| service["service"] == "web")
        .expect("stale service");

    assert_eq!(stale_service["state"], "stale");
    assert_eq!(stale_service["outputs"][0]["status"], "rendered");
}
#[test]
fn runner_delete_on_stopped_removes_owned_output_file_after_exit() {
    let registry_path = temp_registry_path("delete-on-stopped-registry");
    let root = temp_test_dir("delete-on-stopped-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"delete-on-stopped\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"stopped.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\ndelete_on = [\"stopped\", \"removed\"]\n"
        ),
    )
    .expect("write output config");
    let rendered_path = root.join(".bindport/generated/traefik/web.yml");

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
    assert!(!rendered_path.exists());
}
