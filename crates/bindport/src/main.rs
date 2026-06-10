// SPDX-License-Identifier: MIT

use std::{env, process::ExitCode};

use bindport_adapters::AdapterKind;
use bindport_core::{DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, SERVICE_NAME};
use bindport_registry::DEFAULT_REGISTRY_FILE;
use bindport_runner::{RunnerError, run_child};

fn main() -> ExitCode {
    run(env::args().skip(1))
}

fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let args = args.into_iter().collect::<Vec<_>>();

    match args.first().map(String::as_str) {
        None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("{SERVICE_NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("status") => {
            if args.iter().any(|arg| arg == "--json") {
                println!(r#"{{"services":[],"runs":[]}}"#);
            } else {
                println!("No BindPort runs recorded yet.");
            }
            ExitCode::SUCCESS
        }
        Some("doctor") => {
            println!("BindPort bootstrap doctor");
            println!("registry: {DEFAULT_REGISTRY_FILE} (not initialized)");
            println!(
                "default port range: {}-{}",
                DEFAULT_PORT_RANGE.start, DEFAULT_PORT_RANGE.end
            );
            println!("first proxy adapter: {}", AdapterKind::Traefik.as_str());
            ExitCode::SUCCESS
        }
        Some("--") => run_wrapped_command(&args[1..]),
        Some("run") => {
            if args.get(1).map(String::as_str) == Some("--") {
                run_wrapped_command(&args[2..])
            } else {
                eprintln!("usage: bindport run -- <command>");
                ExitCode::FAILURE
            }
        }
        Some(command) => {
            eprintln!("unknown bindport command: {command}");
            eprintln!("run `bindport --help` for available bootstrap commands");
            ExitCode::FAILURE
        }
    }
}

fn run_wrapped_command(command: &[String]) -> ExitCode {
    match run_child(command, DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS) {
        Ok(status) => status_to_exit_code(status.code()),
        Err(error) => {
            print_runner_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn status_to_exit_code(code: Option<i32>) -> ExitCode {
    match code {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

fn print_runner_error(error: &RunnerError) {
    eprintln!("bindport: {error}");
}

fn print_help() {
    println!("BindPort - proxy-neutral local development port registry");
    println!();
    println!("Usage:");
    println!("  bindport -- <command>        Run a command with an assigned PORT");
    println!("  bindport run -- <command>    Run a command with an assigned PORT");
    println!("  bindport status [--json]     Show registry status");
    println!("  bindport doctor              Show bootstrap diagnostics");
    println!("  bindport --version           Print version");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_args_print_help_successfully() {
        assert_eq!(run([]), ExitCode::SUCCESS);
    }

    #[test]
    fn version_arg_succeeds() {
        assert_eq!(run([String::from("--version")]), ExitCode::SUCCESS);
    }

    #[test]
    fn empty_runner_command_fails() {
        assert_eq!(run([String::from("--")]), ExitCode::FAILURE);
    }
}
