// SPDX-License-Identifier: MIT

use crate::support::*;

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
fn config_validate_reports_port_range_minimum_in_every_format() {
    for (extension, contents) in [
        (
            "toml",
            "project = \"zero-port\"\ndefault_range = \"0-10\"\n",
        ),
        (
            "json",
            "{\"project\":\"zero-port\",\"default_range\":\"0-10\"}\n",
        ),
        ("yaml", "project: zero-port\ndefault_range: 0-10\n"),
    ] {
        let registry_path = temp_registry_path(&format!("zero-port-{extension}-registry"));
        let root = temp_test_dir(&format!("zero-port-{extension}-root"));
        fs::write(root.join(format!(".bindport.{extension}")), contents)
            .expect("write zero-port config");

        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args(["config", "validate"])
            .output()
            .expect("validate zero-port config");

        assert!(
            !output.status.success(),
            "{extension} unexpectedly succeeded"
        );
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            message.contains("range start 0 must be at least 1"),
            "unexpected {extension} error: {message}"
        );
    }
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
