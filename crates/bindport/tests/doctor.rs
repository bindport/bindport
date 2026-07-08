// SPDX-License-Identifier: MIT

mod support;

use bindport_registry::{OutputFileRecord, OutputFileScope, OutputFileStatus};
use support::*;

#[test]
fn doctor_outputs_reports_configured_output() {
    let registry_path = temp_registry_path("doctor-output-registry");
    let root = temp_test_dir("doctor-output-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{ route.service }}.yml\"\n",
    )
    .expect("write output config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("BindPort output doctor"));
    assert!(stdout.contains("routes: 0"));
    assert!(stdout.contains("output traefik:"));
    assert!(stdout.contains("template: bindport-traefik (built-in)"));
    assert!(stdout.contains("target host: 127.0.0.1 (loopback)"));
    assert!(stdout.contains("target scheme: http"));
    assert!(stdout.contains("resolved root:"));
    assert!(stdout.contains("(missing, will be created)"));
    assert!(stdout.contains("ownership rows: none"));
    assert!(stdout.contains("planned files: 0"));
    assert!(!root.join(".bindport/generated").exists());
}

#[test]
fn doctor_outputs_rejects_invalid_target_host() {
    let registry_path = temp_registry_path("doctor-output-target-host-registry");
    let root = temp_test_dir("doctor-output-target-host-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[output_defaults]\ntarget_host = \"http://127.0.0.1\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{ route.service }}.yml\"\n",
    )
    .expect("write output config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("target host: http://127.0.0.1 (invalid: target_host must be a host name or IP address, not a URL)"));
    assert!(stdout.contains("resolved root:"));
    assert!(stdout.contains("planned files: 0"));
}

#[test]
fn doctor_outputs_reports_foreign_ownership_rows() {
    let registry_path = temp_registry_path("doctor-output-ownership-registry");
    let root = temp_test_dir("doctor-output-ownership-root");
    let foreign_root = temp_test_dir("doctor-output-ownership-foreign-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/{{ route.service }}.yml\"\n",
    )
    .expect("write output config");
    let mut registry = Registry::open(&registry_path).expect("registry");
    registry
        .record_output_file(&OutputFileRecord {
            output_name: String::from("traefik"),
            scope: OutputFileScope::new(
                foreign_root.join(".bindport/generated"),
                foreign_root.clone(),
                None,
                None,
            ),
            route_key: String::from("foreign-route"),
            rendered_path: foreign_root.join(".bindport/generated/traefik/web.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("foreign-hash")),
            template_hash: None,
            lease_id: None,
            run_id: None,
        })
        .expect("record foreign output");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(
        stdout.contains("ownership rows: 0 current-scope, 0 legacy-adoptable, 1 foreign/stale")
    );
    assert!(stdout.contains("ownership warning: 1 rows outside current output root"));
}

#[test]
fn doctor_outputs_reports_hook_trust_status_without_outputs() {
    let registry_path = temp_registry_path("doctor-hook-registry");
    let root = temp_test_dir("doctor-hook-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-hook-project\"\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"route-log\"\nevents = [\"route_started\"]\ncommand = [\"true\"]\n",
    )
    .expect("write hook config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("hooks: 1 configured"));
    assert!(stdout.contains("hook route-log:"));
    assert!(stdout.contains("trust: pending"));
    assert!(stdout.contains("events: route_started"));
    assert!(stdout.contains("target: true (opaque)"));
    assert!(stdout.contains("hook hash:"));
    assert!(stdout.contains("target hash:"));
    assert!(stdout.contains("BINDPORT_HOOK_EVENTS=<redacted>"));
    assert!(stdout.contains("outputs: none configured"));
}
#[test]
fn doctor_outputs_reports_wildcard_template_warning() {
    let registry_path = temp_registry_path("doctor-output-wildcard-registry");
    let root = temp_test_dir("doctor-output-wildcard-root");
    let template_dir = root.join(".bindport").join("templates");
    fs::create_dir_all(&template_dir).expect("template dir");
    fs::write(template_dir.join("debug.10.txt.j2"), "first\n").expect("write first template");
    fs::write(template_dir.join("debug.20.txt.j2"), "second\n").expect("write second template");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"debug\"\ntemplate = \"debug\"\nroot = \".bindport/generated\"\ntarget = \"debug/{{ route.service }}.txt\"\n",
    )
    .expect("write output config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(
        output.status.success(),
        "doctor outputs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("output debug:"));
    assert!(stdout.contains("template: debug (project)"));
    assert!(stdout.contains("template warning: multiple wildcard matches"));
    assert!(stdout.contains("debug.10.txt.j2"));
}
#[test]
fn doctor_outputs_reports_render_plan_errors() {
    let registry_path = temp_registry_path("doctor-output-collision-registry");
    let root = temp_test_dir("doctor-output-collision-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-output-project\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\nroot = \".bindport/generated\"\ntarget = \"traefik/same.yml\"\n",
    )
    .expect("write output config");
    record_registry_service(&registry_path, "web", 29_601);
    record_registry_service(&registry_path, "api", 29_602);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor", "outputs"])
        .output()
        .expect("run bindport doctor outputs");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("routes: 2"));
    assert!(stdout.contains("output traefik:"));
    assert!(stdout.contains("plan: invalid"));
    assert!(stdout.contains("multiple routes render to target `traefik/same.yml`"));
    assert!(!root.join(".bindport/generated/traefik/same.yml").exists());
}
#[test]
fn doctor_reports_unknown_config_keys() {
    let registry_path = temp_registry_path("doctor-unknown-config-registry");
    let root = temp_test_dir("doctor-unknown-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "defaultrange = \"29100-29199\"\n[proxy.traefik]\nenabled = true\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("ignored unknown top-level keys: defaultrange, proxy"));
    assert!(stdout.contains(
        "config applied keys: project, service, default_range, skip_ports, services, dashboard, output_defaults, outputs, hooks"
    ));
}
#[test]
fn doctor_reports_identity_registry_and_next_candidate() {
    let registry_path = temp_registry_path("doctor-diagnostics-registry");
    let root = temp_test_dir("doctor-diagnostics-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-project\"\nservice = \"web\"\ndefault_range = \"29340-29349\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let candidate = doctor_candidate_port(&stdout);

    assert!(stdout.contains(&format!("registry: {} (ok)", registry_path.display())));
    assert!(stdout.contains("effective identity: project=doctor-project service=web"));
    assert!(stdout.contains("identity key: v1:"));
    assert!(stdout.contains("registry active ports in range: none"));
    assert!(stdout.contains("previous identity port: none"));
    assert!(stdout.contains("known registry listener conflicts in range: "));
    assert!(stdout.contains("unknown os listener conflicts in range: "));
    assert!(stdout.contains("allocation scan start: "));
    assert!((29_340..=29_349).contains(&candidate));
}
#[test]
fn doctor_reports_active_registry_port_conflict() {
    let registry_path = temp_registry_path("doctor-active-conflict-registry");
    let root = temp_test_dir("doctor-active-conflict-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-project\"\nservice = \"web\"\ndefault_range = \"29350-29355\"\nskip_ports = []\n",
    )
    .expect("write project config");
    reserve_registry_port(&registry_path, 29_350);

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let candidate = doctor_candidate_port(&stdout);

    assert!(stdout.contains("registry active ports in range: 29350"));
    assert_ne!(candidate, 29_350);
    assert!((29_350..=29_355).contains(&candidate));
}
#[test]
fn doctor_caps_os_listener_conflict_scan_for_wide_ranges() {
    let registry_path = temp_registry_path("doctor-wide-range-registry");
    let root = temp_test_dir("doctor-wide-range-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"doctor-project\"\nservice = \"web\"\ndefault_range = \"28500-65535\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("scanned first 1024 of 37036 ports"));
}
