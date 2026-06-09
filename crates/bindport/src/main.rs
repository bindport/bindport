// SPDX-License-Identifier: MIT

use std::{env, process::ExitCode};

use bindport_adapters::AdapterKind;
use bindport_core::{DEFAULT_PORT_RANGE, SERVICE_NAME};
use bindport_registry::DEFAULT_REGISTRY_FILE;
use bindport_runner::PORT_ENV_VAR;

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
        Some("--" | "run") => {
            eprintln!(
                "bindport runner bootstrap is present, but command wrapping is not implemented yet"
            );
            eprintln!("planned default env injection: {PORT_ENV_VAR}=<assigned>");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("unknown bindport command: {command}");
            eprintln!("run `bindport --help` for available bootstrap commands");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!("BindPort - proxy-neutral local development port registry");
    println!();
    println!("Usage:");
    println!("  bindport -- <command>        Run a command with an assigned PORT (planned)");
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
    fn runner_placeholder_fails_until_implemented() {
        assert_eq!(run([String::from("--")]), ExitCode::FAILURE);
    }

    #[test]
    fn wrapped_command_flags_are_not_treated_as_global_flags() {
        assert_eq!(
            run([
                String::from("--"),
                String::from("tool"),
                String::from("--version")
            ]),
            ExitCode::FAILURE
        );
        assert_eq!(
            run([String::from("--"), String::from("tool"), String::from("-h")]),
            ExitCode::FAILURE
        );
    }
}
