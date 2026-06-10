// SPDX-License-Identifier: MIT

use std::process::Command;

use bindport_core::{DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS};

fn bindport() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bindport"))
}

#[test]
fn dash_dash_runs_child_with_assigned_port() {
    let output = bindport()
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
    let status = bindport()
        .args(["--", "sh", "-c", "exit 37"])
        .status()
        .expect("run bindport");

    assert_eq!(status.code(), Some(37));
}

#[test]
fn run_subcommand_accepts_dash_dash_separator() {
    let output = bindport()
        .args(["run", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}

#[test]
fn wrapped_command_flags_are_passed_to_child() {
    let output = bindport()
        .args(["--", "sh", "-c", "printf '%s' \"$1\"", "sh", "--version"])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"--version");
}
