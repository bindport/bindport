// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn clean_dry_run_reports_without_removing_stopped_entries() {
    let registry_path = temp_registry_path("clean-dry-run");
    let run_output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf clean"])
        .output()
        .expect("run bindport");

    assert!(run_output.status.success());

    let dry_run = bindport_with_registry(&registry_path)
        .args(["clean", "--dry-run", "--json"])
        .output()
        .expect("run bindport clean dry-run");

    assert!(
        dry_run.status.success(),
        "clean dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );

    let report = serde_json::from_slice::<Value>(&dry_run.stdout).expect("clean json");
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["leases"], 1);
    assert_eq!(report["runs"], 1);
    assert_eq!(report["states"]["stopped"], 1);
    assert_eq!(report["states"]["stale"], 0);

    let status_after_dry_run = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status =
        serde_json::from_slice::<Value>(&status_after_dry_run.stdout).expect("status json");
    assert_eq!(status["services"].as_array().expect("services").len(), 1);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 1);

    let clean = bindport_with_registry(&registry_path)
        .args(["clean", "--json"])
        .output()
        .expect("run bindport clean");

    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let report = serde_json::from_slice::<Value>(&clean.stdout).expect("clean json");
    assert_eq!(report["dry_run"], false);
    assert_eq!(report["leases"], 1);
    assert_eq!(report["runs"], 1);

    let status_after_clean = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_after_clean.stdout).expect("status json");
    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
}
#[cfg(unix)]
#[test]
fn clean_requires_yes_for_noninteractive_stale_entries() {
    let registry_path = temp_registry_path("clean-stale-confirmation");
    let mut registry = Registry::open(&registry_path).expect("registry");
    registry
        .record_run_started(&RunStart {
            project: String::from("stale-clean"),
            service: String::from("web"),
            identity: None,
            host: String::from("127.0.0.1"),
            port: 29_501,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: PathBuf::from("/tmp/bindport-stale-clean-fixture"),
        })
        .expect("record stale candidate");

    let rejected = bindport_with_registry(&registry_path)
        .args(["clean", "--stale", "--json"])
        .output()
        .expect("run bindport clean");

    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("stale cleanup requires confirmation; rerun with --yes")
    );

    let accepted = bindport_with_registry(&registry_path)
        .args(["clean", "--stale", "--json", "--yes"])
        .output()
        .expect("run bindport clean with confirmation");

    assert!(
        accepted.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );

    let report = serde_json::from_slice::<Value>(&accepted.stdout).expect("clean json");
    assert_eq!(report["leases"], 1);
    assert_eq!(report["states"]["stale"], 1);
}
#[test]
fn clean_keeps_active_entries() {
    let registry_path = temp_registry_path("clean-keeps-active");
    let run_output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf clean"])
        .output()
        .expect("run bindport");

    assert!(run_output.status.success());
    reserve_registry_port(&registry_path, 29_501);

    let clean = bindport_with_registry(&registry_path)
        .args(["clean", "--json"])
        .output()
        .expect("run bindport clean");

    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let report = serde_json::from_slice::<Value>(&clean.stdout).expect("clean json");
    assert_eq!(report["leases"], 1);
    assert_eq!(report["states"]["stopped"], 1);

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(services.len(), 1);
    assert_eq!(runs.len(), 1);
    assert_eq!(services[0]["state"], "active");
    assert_eq!(services[0]["port"], 29_501);
}
#[test]
fn clean_removes_owned_output_files_for_removed_routes() {
    let registry_path = temp_registry_path("clean-output-removed-registry");
    let root = temp_test_dir("clean-output-removed-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"clean-output-removed\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"clean.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
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
    assert!(rendered_path.is_file());

    let clean_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["clean", "--stopped"])
        .output()
        .expect("clean stopped entries");

    assert!(
        clean_output.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );
    assert!(!rendered_path.exists());
}
