// SPDX-License-Identifier: MIT

use crate::support::*;

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
fn fast_child_finalization_records_child_pid_command_cwd_and_exit() {
    let registry_path = temp_registry_path("fast-child-finalization-registry");
    let root = temp_test_dir("fast-child-finalization-root")
        .canonicalize()
        .expect("canonical root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"fast-child-finalization\"\nservice = \"noop\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s|%s' $$ \"$(pwd -P)\"; exit 23"])
        .output()
        .expect("run fast child");

    assert_eq!(output.status.code(), Some(23));
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let (pid, cwd) = stdout.split_once('|').expect("pid and cwd");
    let child_pid = pid.parse::<u32>().expect("child pid");
    assert_eq!(cwd, root.display().to_string());

    let snapshot = Registry::open(&registry_path)
        .expect("registry")
        .export_snapshot()
        .expect("export");
    assert_eq!(snapshot.leases.len(), 1);
    assert_eq!(snapshot.leases[0].state, "stopped");
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.runs[0].pid, child_pid);
    assert!(snapshot.runs[0].command.contains("printf '%s|%s'"));
    assert_eq!(snapshot.runs[0].cwd, root.display().to_string());
    assert_eq!(snapshot.runs[0].exit_code, Some(23));
    assert!(snapshot.runs[0].exited_at.is_some());
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
