// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn assigned_port_stays_consistent_across_environment_overrides() {
    for (case, config_port, cli_port) in [
        ("inherited", None, None),
        ("config", Some("2"), None),
        ("cli", None, Some("3")),
        ("combined", Some("2"), Some("3")),
        ("empty-config", Some(""), None),
        ("empty-cli", None, Some("")),
        ("template", Some("{port}"), Some("{port}")),
    ] {
        let registry_path = temp_registry_path(&format!("port-env-{case}-registry"));
        let root = temp_test_dir(&format!("port-env-{case}-root"))
            .canonicalize()
            .expect("canonical root");
        let range_start = free_loopback_port();
        let range_end = range_start.saturating_add(10);
        let mut config = format!(
            r#"project = "port-env"
default_range = "{range_start}-{range_end}"
skip_ports = []

[[services]]
name = "web"
command = ["sh", "-c", "printf '%s|%s|%s|%s|%s' \"$PORT\" \"$1\" \"$BINDPORT_ASSIGNED_PORT\" \"$BINDPORT_TEST_ENV\" \"$(pwd -P)\"", "sh"]
args = ["{{port}}"]
env.BINDPORT_ASSIGNED_PORT = "{{port}}"
env.BINDPORT_TEST_ENV = "config"
"#
        );
        if let Some(value) = config_port {
            config.push_str(&format!("env.PORT = \"{value}\"\n"));
        }
        fs::write(root.join(".bindport.toml"), config).expect("write config");

        let mut command = bindport_with_registry(&registry_path);
        command
            .current_dir(&root)
            .env("PORT", "1")
            .env("BINDPORT_TEST_ENV", "parent")
            .args(["run", "web", "--env", "BINDPORT_TEST_ENV=cli"]);
        if let Some(value) = cli_port {
            command.args(["--env", &format!("PORT={value}")]);
        }
        let output = command.output().expect("run bindport");
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let status_output = bindport_with_registry(&registry_path)
            .args(["status", "--json"])
            .output()
            .expect("run status");
        assert!(status_output.status.success());
        let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
        let services = status["services"].as_array().expect("services");
        assert_eq!(services.len(), 1);
        let assigned_port = services[0]["port"].as_u64().expect("assigned port");
        assert!((u64::from(range_start)..=u64::from(range_end)).contains(&assigned_port));
        assert_eq!(
            String::from_utf8(output.stdout).expect("stdout"),
            format!(
                "{assigned_port}|{assigned_port}|{assigned_port}|cli|{}",
                root.display()
            ),
            "{case}"
        );
    }
}
