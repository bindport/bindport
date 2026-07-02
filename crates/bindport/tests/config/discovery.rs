// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn parent_project_config_sets_port_range_and_project() {
    let registry_path = temp_registry_path("project-config-registry");
    let root = temp_test_dir("project-config-root");
    let nested = root.join("packages").join("web");
    fs::create_dir_all(&nested).expect("nested dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"configured-project\"\ndefault_range = \"29100-29101\"\nskip_ports = [29100]\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&nested)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29101");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "configured-project");
}
#[test]
fn service_path_config_infers_service_from_current_directory() {
    let registry_path = temp_registry_path("service-path-registry");
    let root = temp_test_dir("service-path-root");
    let api_src = root.join("apps").join("api").join("src");
    fs::create_dir_all(&api_src).expect("api source dir");
    fs::create_dir_all(root.join("apps").join("web")).expect("web dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"monorepo\"\ndefault_range = \"29104-29104\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\nhostname = \"web.{project}.localhost\"\nenv.BINDPORT_SELECTED_SERVICE = \"{service}\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\nhostname = \"{service}.{project}.localhost\"\nenv.BINDPORT_SELECTED_SERVICE = \"{service}\"\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&api_src)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s' \"$PORT\" \"$BINDPORT_SELECTED_SERVICE\"",
        ])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29104|api");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "monorepo");
    assert_eq!(status["services"][0]["service"], "api");
    assert_eq!(status["services"][0]["hostname"], "api.monorepo.localhost");
    assert_eq!(
        status["services"][0]["route_url"],
        "http://api.monorepo.localhost"
    );
}
#[test]
fn config_explain_reports_field_and_identity_sources() {
    let registry_path = temp_registry_path("config-explain-registry");
    let root = temp_test_dir("config-explain-root");
    let api_src = root.join("apps").join("api").join("src");
    fs::create_dir_all(&api_src).expect("api source dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"base-project\"\ndefault_range = \"29105-29106\"\nskip_ports = [29105]\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
    )
    .expect("write base config");
    fs::write(
        root.join(".bindport.local.toml"),
        "project = \"local-project\"\ndefault_range = \"29107-29107\"\n",
    )
    .expect("write local config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&api_src)
        .args(["config", "explain"])
        .output()
        .expect("run bindport config explain");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("config explain stdout");

    assert!(stdout.contains("BindPort config explain"));
    assert!(stdout.contains("config:"));
    assert!(stdout.contains(".bindport.toml"));
    assert!(stdout.contains("config local override:"));
    assert!(stdout.contains(".bindport.local.toml"));
    assert!(stdout.contains("project: local-project (local override config)"));
    assert!(stdout.contains("default_range: 29107-29107 (local override config)"));
    assert!(stdout.contains("skip_ports: 1 ports (project config)"));
    assert!(stdout.contains("services: 2 entries (project config)"));
    assert!(stdout.contains("project: local-project (local override config `project`)"));
    assert!(stdout.contains("service: api (project config `[[services]].path`)"));
}
