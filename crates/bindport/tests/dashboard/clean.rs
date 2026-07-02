// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn dashboard_cleans_stopped_entries() {
    let registry_path = temp_registry_path("dashboard-clean-registry");
    let output = bindport_with_registry(&registry_path)
        .env(BINDPORT_PROJECT_ENV, "dashboard-clean-fixture")
        .args(["run", "web", "--", "sh", "-c", "printf dashboard-clean"])
        .output()
        .expect("run bindport fixture");

    assert!(output.status.success());

    let dashboard = start_dashboard(bindport_with_registry(&registry_path));
    let clean_response = http_post_clean(dashboard.port, "/api/clean/stopped", None);

    assert!(clean_response.starts_with("HTTP/1.1 200 OK"));

    let report = serde_json::from_str::<Value>(http_body(&clean_response)).expect("clean json");
    assert_eq!(report["leases"], 1);
    assert_eq!(report["runs"], 1);
    assert_eq!(report["states"]["stopped"], 1);

    let status_response = http_get(dashboard.port, "/api/status");
    let status = serde_json::from_str::<Value>(http_body(&status_response)).expect("status json");

    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
}
#[test]
fn dashboard_clean_removes_owned_output_files_for_removed_routes() {
    let registry_path = temp_registry_path("dashboard-clean-output-registry");
    let root = temp_test_dir("dashboard-clean-output-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"dashboard-clean-output\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"dashboard-clean.localhost\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{{{ route.service }}}}.yml\"\n"
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

    let mut command = bindport_with_registry(&registry_path);
    command.current_dir(&root);
    let dashboard = start_dashboard(command);
    let clean_response = http_post_clean(dashboard.port, "/api/clean/stopped", None);

    assert!(clean_response.starts_with("HTTP/1.1 200 OK"));
    assert!(!rendered_path.exists());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("status after dashboard clean");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["outputs"][0]["name"], "traefik");
    assert_eq!(status["outputs"][0]["removed"], 1);
}
