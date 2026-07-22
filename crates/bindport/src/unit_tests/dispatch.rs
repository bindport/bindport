// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn empty_args_print_help_successfully() {
    assert_eq!(dispatch([]), ExitCode::SUCCESS);
    assert_eq!(dispatch([String::from("--help")]), ExitCode::SUCCESS);
}

#[test]
fn version_arg_succeeds() {
    assert_eq!(dispatch([String::from("--version")]), ExitCode::SUCCESS);
}

#[test]
fn subcommand_help_surfaces_succeed() {
    assert_eq!(dispatch(strings(["config", "--help"])), ExitCode::SUCCESS);
    assert_eq!(dispatch(strings(["doctor", "--help"])), ExitCode::SUCCESS);
    assert_eq!(dispatch(strings(["list", "--help"])), ExitCode::SUCCESS);
    assert_eq!(dispatch(strings(["port", "--help"])), ExitCode::SUCCESS);
    assert_eq!(dispatch(strings(["registry", "--help"])), ExitCode::SUCCESS);
    assert_eq!(
        dispatch(strings(["registry", "export", "--help"])),
        ExitCode::SUCCESS
    );
    assert_eq!(dispatch(strings(["render", "--help"])), ExitCode::SUCCESS);
    assert_eq!(
        dispatch(strings(["templates", "--help"])),
        ExitCode::SUCCESS
    );
    assert_eq!(dispatch(strings(["clean", "--help"])), ExitCode::SUCCESS);
    assert_eq!(
        dispatch(strings(["dashboard", "--help"])),
        ExitCode::SUCCESS
    );
}

#[test]
fn invalid_command_surfaces_fail_without_panicking() {
    for args in [
        strings(["unknown"]),
        strings(["run", "--bad"]),
        strings(["config", "unknown"]),
        strings(["config", "explain", "extra"]),
        strings(["doctor", "unknown"]),
        strings(["doctor", "outputs", "extra"]),
        strings(["list", "--bad"]),
        strings(["list", "web"]),
        strings(["port"]),
        strings(["port", "web", "api"]),
        strings(["registry", "unknown"]),
        strings(["registry", "export", "--json"]),
        strings(["render", "--bad"]),
        strings(["templates", "unknown"]),
        strings(["templates", "show"]),
        strings(["clean", "--bad"]),
        strings(["dashboard", "--bad"]),
        strings(["dashboard", "serve", "--host", "0.0.0.0"]),
        strings([
            "dashboard",
            "serve",
            "--auth-required",
            "--token-env",
            "BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST",
        ]),
    ] {
        assert_eq!(dispatch(args), ExitCode::FAILURE);
    }
}

#[test]
fn empty_runner_command_fails() {
    assert_eq!(dispatch([String::from("--")]), ExitCode::FAILURE);
}
