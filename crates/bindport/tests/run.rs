// SPDX-License-Identifier: MIT

mod support;

use support::*;

#[test]
fn dash_dash_runs_child_with_assigned_port() {
    let registry_path = temp_registry_path("dash-dash");
    let output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let port = stdout.parse::<u16>().expect("stdout is a port number");

    assert!(DEFAULT_PORT_RANGE.contains(port));
    assert!(!DEFAULT_SKIP_PORTS.contains(&port));
}
#[cfg(unix)]
#[test]
fn package_script_runs_bindport_next_dev_flow() {
    let registry_path = temp_registry_path("package-script-registry");
    let root = temp_test_dir("package-script-root");
    let bindport_bin_dir = root.join(".test-bin");
    let next_bin_dir = root.join("node_modules").join(".bin");

    fs::create_dir_all(&bindport_bin_dir).expect("bindport bin dir");
    fs::create_dir_all(&next_bin_dir).expect("next bin dir");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_bindport"),
        bindport_bin_dir.join("bindport"),
    )
    .expect("link bindport binary");
    write_executable(
        &next_bin_dir.join("next"),
        "#!/bin/sh\nif [ \"$1\" != \"dev\" ]; then echo \"unexpected next args: $*\" >&2; exit 64; fi\nprintf 'next-dev-port=%s\\n' \"$PORT\"\n",
    );
    fs::write(
        root.join("package.json"),
        r#"{"name":"bindport-package-script-fixture","private":true,"scripts":{"dev":"bindport -- next dev"}}"#,
    )
    .expect("write package json");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"package-script-fixture\"\nservice = \"web\"\ndefault_range = \"29420-29421\"\nskip_ports = []\n",
    )
    .expect("write config");

    let output = Command::new("npm")
        .current_dir(&root)
        .env(REGISTRY_PATH_ENV, &registry_path)
        .env_remove(BINDPORT_PROJECT_ENV)
        .env_remove(BINDPORT_SERVICE_ENV)
        .env("PATH", prepend_path(&bindport_bin_dir))
        .env("NO_UPDATE_NOTIFIER", "1")
        .env("NPM_CONFIG_AUDIT", "false")
        .env("NPM_CONFIG_FUND", "false")
        .args(["run", "--silent", "dev"])
        .output()
        .expect("run package script");

    assert!(
        output.status.success(),
        "package script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let port = stdout
        .trim()
        .strip_prefix("next-dev-port=")
        .expect("next dev port marker")
        .parse::<u16>()
        .expect("port");

    assert!(matches!(port, 29_420 | 29_421));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "package-script-fixture");
    assert_eq!(status["services"][0]["service"], "web");
    assert_eq!(status["services"][0]["command"], "next dev");
    assert_eq!(status["services"][0]["hostname"], Value::Null);
    assert_eq!(status["services"][0]["route_url"], Value::Null);
    assert_eq!(status["services"][0]["proxy"], Value::Null);
    assert_eq!(status["services"][0]["exit_code"], 0);
    assert_eq!(
        status["services"][0]["port"]
            .as_u64()
            .expect("service port"),
        u64::from(port)
    );
    assert_eq!(status["runs"][0]["exit_code"], 0);
}
#[test]
fn runner_preserves_child_exit_code() {
    let registry_path = temp_registry_path("exit-code");
    let status = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "exit 37"])
        .status()
        .expect("run bindport");

    assert_eq!(status.code(), Some(37));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["exit_code"], 37);
}
#[test]
fn run_subcommand_accepts_dash_dash_separator() {
    let registry_path = temp_registry_path("run-subcommand");
    let output = bindport_with_registry(&registry_path)
        .args(["run", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}
#[test]
fn configured_service_command_expands_port_arguments() {
    let registry_path = temp_registry_path("configured-command-registry");
    let root = temp_test_dir("configured-command-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "storybook-project"
default_range = "{port}-{port}"
skip_ports = []

[[services]]
name = "storybook"
command = ["sh", "-c", "printf '%s|%s|%s' \"$PORT\" \"$1\" \"$2\"", "sh"]
args = ["--port", "{{port}}"]
"#
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "storybook"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!("{port}|--port|{port}")
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["service"], "storybook");
    assert!(
        status["services"][0]["command"]
            .as_str()
            .expect("command")
            .ends_with(&format!("--port {port}"))
    );
}
#[test]
fn explicit_child_command_overrides_configured_service_command() {
    let registry_path = temp_registry_path("configured-command-override-registry");
    let root = temp_test_dir("configured-command-override-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"override-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\ncommand = [\"sh\", \"-c\", \"exit 99\"]\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, port.to_string().as_bytes());
}
#[test]
fn command_surface_reports_invalid_arguments() {
    let registry_path = temp_registry_path("invalid-command-surface-registry");
    let cases: &[(&[&str], &str)] = &[
        (&["unknown"], "unknown bindport command: unknown"),
        (&["run"], "no command provided after `--`"),
        (&["run", "--"], "no command provided after `--`"),
        (
            &["run", "web", "api", "--", "true"],
            "only one service name can be provided",
        ),
        (
            &["run", "--unknown", "--", "true"],
            "unknown run option `--unknown`",
        ),
        (
            &["run", "--env", "PORT", "--", "true"],
            "invalid env assignment `PORT`; expected NAME=VALUE",
        ),
        (
            &["run", "--env", "1PORT=3000", "--", "true"],
            "invalid env variable name `1PORT`",
        ),
        (
            &["run", "--hostname", "--", "true"],
            "--hostname requires a value",
        ),
        (
            &["run", "--route-url", "--", "true"],
            "--route-url requires a value",
        ),
        (
            &["config", "explain", "extra"],
            "config explain does not take arguments",
        ),
        (&["config", "missing"], "unknown config command `missing`"),
        (
            &["doctor", "outputs", "extra"],
            "doctor outputs does not take arguments",
        ),
        (&["doctor", "missing"], "unknown doctor command `missing`"),
    ];

    for (args, expected_error) in cases {
        let output = bindport_with_registry(&registry_path)
            .args(*args)
            .output()
            .expect("run bindport");

        assert!(
            !output.status.success(),
            "expected failure for args {args:?}"
        );
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        assert!(
            stderr.contains(expected_error),
            "stderr for args {args:?} did not contain `{expected_error}`:\n{stderr}"
        );
    }
}
#[test]
fn run_subcommand_service_argument_overrides_env_and_config() {
    let registry_path = temp_registry_path("identity-precedence");
    let root = temp_test_dir("identity-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"config-project\"\nservice = \"config-service\"\ndefault_range = \"29120-29120\"\n",
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(BINDPORT_PROJECT_ENV, "env-project")
        .env(BINDPORT_SERVICE_ENV, "env-service")
        .args([
            "run",
            "cli-service",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PORT\"",
        ])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29120");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "env-project");
    assert_eq!(status["services"][0]["service"], "cli-service");
}
#[test]
fn service_config_injects_env_templates_and_route_metadata() {
    let registry_path = temp_registry_path("service-env-registry");
    let root = temp_test_dir("service-env-root");
    let port = free_loopback_port();
    init_git_repo(&root, "feature/tree");
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"example-app\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"{{branch}}.{{project}}.localhost\"\nhealth_url = \"{{route_url}}/health\"\nenv.BINDPORT_ASSIGNED_PORT = \"{{port}}\"\nenv.BINDPORT_ROUTE = \"{{route_url}}\"\nenv.BINDPORT_HEALTH = \"{{health_url}}\"\nenv.BINDPORT_DIRECT_URL = \"{{url}}\"\nenv.HOSTNAME = \"0.0.0.0\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s|%s' \"$BINDPORT_ASSIGNED_PORT\" \"$BINDPORT_ROUTE\" \"$BINDPORT_HEALTH\" \"$BINDPORT_DIRECT_URL\" \"$HOSTNAME\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!(
            "{port}|http://feature-tree.example-app.localhost|http://feature-tree.example-app.localhost/health|http://127.0.0.1:{port}|0.0.0.0"
        )
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(service["project"], "example-app");
    assert_eq!(service["service"], "web");
    assert_eq!(service["hostname"], "feature-tree.example-app.localhost");
    assert_eq!(
        service["route_url"],
        "http://feature-tree.example-app.localhost"
    );
    assert_eq!(
        service["health_url"],
        "http://feature-tree.example-app.localhost/health"
    );
    assert_eq!(service["port"], port);
}
#[test]
fn service_config_rejects_execution_sensitive_env_names() {
    let registry_path = temp_registry_path("service-env-deny-registry");
    let root = temp_test_dir("service-env-deny-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"env-deny\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nenv.NODE_OPTIONS = \"--require ./evil.js\"\nenv.LD_AUDIT = \"./audit.so\"\nenv.GCONV_PATH = \"./gconv\"\nenv.SAFE_VALUE = \"allowed\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s' \"${NODE_OPTIONS-unset}\" \"${LD_AUDIT-unset}\" \"${GCONV_PATH-unset}\" \"$SAFE_VALUE\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"unset|unset|unset|allowed");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `NODE_OPTIONS`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `LD_AUDIT`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `GCONV_PATH`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn run_cli_templates_override_service_config() {
    let registry_path = temp_registry_path("cli-template-registry");
    let root = temp_test_dir("cli-template-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"template-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"config.{{project}}.localhost\"\nenv.NEXT_PUBLIC_BINDPORT_URL = \"config\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--hostname",
            "cli-{service}.localhost",
            "--env",
            "NEXT_PUBLIC_BINDPORT_URL={route_url}",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$NEXT_PUBLIC_BINDPORT_URL\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"http://cli-web.localhost");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["hostname"], "cli-web.localhost");
    assert_eq!(
        status["services"][0]["route_url"],
        "http://cli-web.localhost"
    );
}
#[test]
fn run_templates_reject_unknown_placeholders() {
    let registry_path = temp_registry_path("template-error-registry");
    let output = bindport_with_registry(&registry_path)
        .args([
            "run",
            "web",
            "--env",
            "NEXT_PUBLIC_BINDPORT_URL={missing}",
            "--",
            "sh",
            "-c",
            "true",
        ])
        .output()
        .expect("run bindport");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unknown or unavailable template placeholder `missing`"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn run_templates_escape_literal_braces() {
    let registry_path = temp_registry_path("template-escape-registry");
    let output = bindport_with_registry(&registry_path)
        .args([
            "run",
            "web",
            "--env",
            r#"APP_CONFIG={{"api":"{service}"}}"#,
            "--",
            "sh",
            "-c",
            "printf '%s' \"$APP_CONFIG\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, br#"{"api":"web"}"#);
}
#[test]
fn wrapped_command_flags_are_passed_to_child() {
    let registry_path = temp_registry_path("flags");
    let output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$1\"", "sh", "--version"])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"--version");
}
#[test]
fn runner_reuses_previous_identity_port_when_available() {
    let registry_path = temp_registry_path("sticky-registry");
    let root = temp_test_dir("sticky-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29300-29301\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    let second_port = run_print_port(&registry_path, &root);

    assert_eq!(second_port, first_port);
}
#[test]
fn runner_falls_back_when_previous_identity_port_is_active() {
    let registry_path = temp_registry_path("sticky-occupied-registry");
    let root = temp_test_dir("sticky-occupied-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29310-29311\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    reserve_registry_port(&registry_path, first_port);
    let second_port = run_print_port(&registry_path, &root);

    assert_ne!(second_port, first_port);
    assert!(matches!(second_port, 29_310 | 29_311));
}
#[cfg(unix)]
#[test]
fn runner_retries_once_when_assigned_port_is_claimed_after_spawn() {
    let registry_path = temp_registry_path("allocation-retry-registry");
    let root = temp_test_dir("allocation-retry-root");
    let marker_path = temp_path("allocation-retry-marker");
    let pid_path = temp_path("allocation-retry-pid");
    let marker_arg = marker_path.display().to_string();
    let pid_arg = pid_path.display().to_string();

    fs::write(
        root.join(".bindport.toml"),
        "project = \"retry-project\"\nservice = \"web\"\ndefault_range = \"29400-29401\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            concat!(
                "if [ ! -f \"$1\" ]; then ",
                "python3 -c 'import os,socket,sys,time; from pathlib import Path; ",
                "s=socket.socket(); ",
                "s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); ",
                "s.bind((\"127.0.0.1\", int(sys.argv[1]))); ",
                "s.listen(); ",
                "Path(sys.argv[2]).write_text(str(os.getpid())); ",
                "Path(sys.argv[3]).write_text(sys.argv[1]); ",
                "time.sleep(5)' \"$PORT\" \"$2\" \"$1\" & ",
                "i=0; ",
                "while [ ! -f \"$1\" ] && [ \"$i\" -lt 100 ]; do ",
                "i=$((i + 1)); sleep 0.02; ",
                "done; ",
                "[ -f \"$1\" ] || exit 99; ",
                "exit 98; ",
                "fi; ",
                "printf '%s' \"$PORT\"",
            ),
            "sh",
            &marker_arg,
            &pid_arg,
        ])
        .output()
        .expect("run bindport");

    terminate_process_from_file(&pid_path);

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let first_port = fs::read_to_string(&marker_path)
        .expect("first port marker")
        .parse::<u16>()
        .expect("first port");
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let second_port = stdout.parse::<u16>().expect("second port");
    let stderr = String::from_utf8(output.stderr).expect("stderr");

    assert_ne!(second_port, first_port);
    assert!(matches!(first_port, 29_400 | 29_401));
    assert!(matches!(second_port, 29_400 | 29_401));
    assert!(stderr.contains("retrying with another port"));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let runs = status["runs"].as_array().expect("runs");
    let mut exit_codes = runs
        .iter()
        .map(|run| run["exit_code"].as_i64().expect("exit code"))
        .collect::<Vec<_>>();
    exit_codes.sort_unstable();

    assert_eq!(runs.len(), 2);
    assert_eq!(exit_codes, [0, 98]);
    assert_eq!(status["services"][0]["exit_code"], 0);
    assert_eq!(
        status["services"][0]["port"]
            .as_u64()
            .expect("service port"),
        u64::from(second_port)
    );
}
#[test]
fn runner_continues_when_registry_path_is_unavailable() {
    let output = bindport_without_registry_path()
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let port = stdout.parse::<u16>().expect("stdout is a port number");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");

    assert!(DEFAULT_PORT_RANGE.contains(port));
    assert!(stderr.contains("running without registry recording"));
}
#[cfg(unix)]
#[test]
fn forwards_sigterm_to_wrapped_child_and_records_exit() {
    let registry_path = temp_registry_path("signal-registry");
    let child_pid_path = temp_registry_path("signal-child-pid");
    let marker_path = temp_registry_path("signal-marker");
    let child_pid_path_arg = child_pid_path.display().to_string();
    let marker_path_arg = marker_path.display().to_string();

    let mut bindport = bindport_with_registry(&registry_path)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s\n' $$ > \"$1\"; trap 'printf forwarded > \"$2\"; exit 42' TERM INT; printf 'ready\n'; while :; do sleep 1; done",
            "sh",
            &child_pid_path_arg,
            &marker_path_arg,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run bindport");

    let stdout = bindport.stdout.take().expect("stdout pipe");
    let mut stdout = BufReader::new(stdout);
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read readiness line");
    assert_eq!(ready, "ready\n");

    let child_pid = fs::read_to_string(&child_pid_path)
        .expect("child pid file")
        .trim()
        .parse::<u32>()
        .expect("child pid");

    send_signal(bindport.id(), libc::SIGTERM);

    let status = match wait_for_child(&mut bindport, Duration::from_secs(5)) {
        Some(status) => status,
        None => {
            send_signal(child_pid, libc::SIGKILL);
            let _ = bindport.kill();
            panic!("bindport did not exit after SIGTERM");
        }
    };

    assert_eq!(status.code(), Some(42));
    assert_eq!(
        fs::read_to_string(&marker_path).expect("marker"),
        "forwarded"
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    assert_eq!(status["services"][0]["state"], "stopped");
    assert_eq!(status["services"][0]["exit_code"], 42);
    assert_eq!(status["runs"][0]["exit_code"], 42);
}
