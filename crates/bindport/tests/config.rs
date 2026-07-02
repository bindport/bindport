// SPDX-License-Identifier: MIT

mod support;

use support::*;

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
#[test]
fn config_validate_reports_ok_for_valid_config() {
    let registry_path = temp_registry_path("config-validate-ok-registry");
    let root = temp_test_dir("config-validate-ok-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"valid-project\"\ndefault_range = \"29108-29109\"\nskip_ports = [29108]\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.service }}.yml\"\n",
    )
    .expect("write valid config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("run bindport config validate");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("config validate stdout");

    assert!(stdout.contains("BindPort config validate"));
    assert!(stdout.contains("validation: ok"));
}
#[test]
fn config_validate_reports_actionable_errors() {
    let registry_path = temp_registry_path("config-validate-error-registry");
    let root = temp_test_dir("config-validate-error-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"invalid-project\"\n[[services]]\npath = \"../api\"\n[[services]]\nname = \"web\"\npath = \"/tmp/web\"\n[[services]]\nname = \"web\"\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\n[[outputs]]\nname = \"debug\"\ntarget = \"debug/{{ route.service }}.txt\"\n",
    )
    .expect("write invalid config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("run bindport config validate");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("config validate stdout");

    assert!(stdout.contains("BindPort config validate"));
    assert!(stdout.contains("validation: 6 errors"));
    assert!(stdout.contains("error: services[0].name: service name is required"));
    assert!(stdout.contains("error: services[0].path: service path must be relative"));
    assert!(stdout.contains("error: services[1].path: service path must be relative"));
    assert!(stdout.contains("error: services[2].name: duplicate service name `web`"));
    assert!(
        stdout.contains("error: outputs[0].target: output `traefik` is missing required `target`")
    );
    assert!(
        stdout
            .contains("error: outputs[1].template: output `debug` is missing required `template`")
    );
}
#[test]
fn checked_in_monorepo_example_resolves_services() {
    let registry_path = temp_registry_path("monorepo-example-registry");
    let root = workspace_root().join("examples").join("monorepo");

    let validate = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["config", "validate"])
        .output()
        .expect("validate monorepo example");
    assert!(
        validate.status.success(),
        "config validate failed: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
    let validate_stdout = String::from_utf8(validate.stdout).expect("validate stdout");
    assert!(validate_stdout.contains("validation: ok"));

    for (path, service) in [("apps/web", "web"), ("apps/api", "api")] {
        let explain = bindport_with_registry(&registry_path)
            .current_dir(root.join(path))
            .args(["config", "explain"])
            .output()
            .expect("explain monorepo service");
        assert!(
            explain.status.success(),
            "config explain failed for {path}: {}",
            String::from_utf8_lossy(&explain.stderr)
        );
        let stdout = String::from_utf8(explain.stdout).expect("explain stdout");
        assert!(stdout.contains("project: example (project config `project`)"));
        assert!(stdout.contains(&format!(
            "service: {service} (project config `[[services]].path`)"
        )));
    }
}
#[test]
fn local_project_config_overrides_base_project_config() {
    let registry_path = temp_registry_path("local-project-config-registry");
    let root = temp_test_dir("local-project-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"base-project\"\ndefault_range = \"29102-29102\"\nskip_ports = []\n",
    )
    .expect("write base config");
    fs::write(
        root.join(".bindport.local.toml"),
        "project = \"local-project\"\ndefault_range = \"29103-29103\"\n",
    )
    .expect("write local config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29103");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "local-project");

    let doctor_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");
    let stdout = String::from_utf8(doctor_output.stdout).expect("doctor stdout");

    assert!(stdout.contains("config local override:"));
    assert!(stdout.contains(".bindport.local.toml"));
}
#[test]
fn toml_config_wins_over_json_in_same_directory() {
    let registry_path = temp_registry_path("config-precedence-registry");
    let root = temp_test_dir("config-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "default_range = \"29110-29110\"\n",
    )
    .expect("write toml config");
    fs::write(
        root.join(".bindport.json"),
        r#"{"default_range":"29111-29111"}"#,
    )
    .expect("write json config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29110");
}
#[test]
fn fallback_config_from_config_home_is_used_when_no_project_config_exists() {
    let state_dir = temp_test_dir("fallback-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = config_home_for_registry(&registry_path)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);
    let cwd = temp_test_dir("fallback-config-cwd");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("config dir");
    fs::write(&config_path, "default_range = \"29200-29200\"\n").expect("write fallback config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29200");
}
#[test]
fn init_creates_fallback_config_in_config_home() {
    let state_dir = temp_test_dir("init-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = config_home_for_registry(&registry_path)
        .join(SERVICE_NAME)
        .join(FALLBACK_CONFIG_FILE);

    let output = bindport_with_registry(&registry_path)
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(config_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let config = fs::read_to_string(&config_path).expect("fallback config");

    assert!(stdout.contains(&config_path.display().to_string()));
    assert!(config.contains("default_range = \"29000-29999\""));
}
