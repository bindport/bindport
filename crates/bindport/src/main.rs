// SPDX-License-Identifier: MIT

use std::{env, path::Path, process::ExitCode, process::ExitStatus};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use bindport_adapters::AdapterKind;
use bindport_core::{DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, SERVICE_NAME};
use bindport_registry::{
    REGISTRY_PATH_ENV, Registry, RegistryError, RunStart, default_registry_path,
};
use bindport_runner::{RunnerError, spawn_child};

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
                print_status_json()
            } else {
                print_status()
            }
        }
        Some("doctor") => {
            println!("BindPort bootstrap doctor");
            match default_registry_path() {
                Ok(path) => println!("registry: {}", path.display()),
                Err(error) => println!("registry: unavailable ({error})"),
            }
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
    match run_wrapped_command_result(command) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            print_runner_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run_wrapped_command_result(command: &[String]) -> Result<ExitCode, RunnerError> {
    if command.is_empty() {
        return Err(RunnerError::NoCommand);
    }

    let mut registry = open_optional_registry();
    let mut skip_ports = DEFAULT_SKIP_PORTS.to_vec();

    let mut disable_registry = false;
    if let Some(registry) = registry.as_mut() {
        match registry.active_ports() {
            Ok(active_ports) => skip_ports.extend(active_ports),
            Err(error) => {
                print_registry_warning("failed to read active registry ports", &error);
                registry_disabled_warning();
                disable_registry = true;
            }
        }
    }
    if disable_registry {
        registry = None;
    }

    let mut child = spawn_child(command, DEFAULT_PORT_RANGE, &skip_ports)?;
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let run = RunStart {
        project: infer_project_name(&cwd),
        service: infer_service_name(command),
        host: String::from("127.0.0.1"),
        port: child.port(),
        pid: child.pid(),
        command: command.join(" "),
        cwd,
    };

    let started = if let Some(registry) = registry.as_mut() {
        match registry.record_run_started(&run) {
            Ok(started) => Some(started),
            Err(error) => {
                print_registry_warning("failed to record run start", &error);
                registry_disabled_warning();
                None
            }
        }
    } else {
        None
    };

    let status = child.wait()?;
    let exit_code = status_registry_exit_code(&status);

    if let (Some(registry), Some(started)) = (registry.as_mut(), started)
        && let Err(error) = registry.record_run_finished(started, exit_code)
    {
        print_registry_warning("failed to record run finish", &error);
    }

    Ok(status_to_exit_code(&status))
}

fn open_optional_registry() -> Option<Registry> {
    match Registry::open_default() {
        Ok(registry) => Some(registry),
        Err(error) => {
            print_registry_warning("registry unavailable", &error);
            registry_disabled_warning();
            None
        }
    }
}

fn print_status_json() -> ExitCode {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => match serde_json::to_string_pretty(&snapshot) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("bindport: failed to serialize status JSON: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn print_status() -> ExitCode {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => {
            if snapshot.services.is_empty() {
                println!("No BindPort runs recorded yet.");
            } else {
                for service in snapshot.services {
                    let pid = service
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| String::from("-"));
                    println!(
                        "{}\t{}\t{}:{}\tpid {}\t{}",
                        service.state,
                        service.service,
                        service.host,
                        service.port,
                        pid,
                        service.command
                    );
                }
            }

            ExitCode::SUCCESS
        }
        Err(error) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn status_to_exit_code(status: &ExitStatus) -> ExitCode {
    match status_registry_exit_code(status) {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

fn status_registry_exit_code(status: &ExitStatus) -> Option<i32> {
    status.code().or_else(|| signal_exit_code(status))
}

#[cfg(unix)]
fn signal_exit_code(status: &ExitStatus) -> Option<i32> {
    status.signal().map(|signal| 128 + signal)
}

#[cfg(not(unix))]
fn signal_exit_code(_status: &ExitStatus) -> Option<i32> {
    None
}

fn print_runner_error(error: &RunnerError) {
    eprintln!("bindport: {error}");
}

fn print_registry_error(error: &RegistryError) {
    eprintln!("bindport: {error}");
    eprintln!("bindport: set {REGISTRY_PATH_ENV} to override the registry path");
}

fn print_registry_warning(context: &str, error: &RegistryError) {
    eprintln!("bindport: warning: {context}: {error}");
}

fn registry_disabled_warning() {
    eprintln!(
        "bindport: warning: running without registry recording; set {REGISTRY_PATH_ENV} to restore it"
    );
}

fn infer_project_name(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn infer_service_name(command: &[String]) -> String {
    command
        .first()
        .and_then(|program| Path::new(program).file_stem())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("command")
        .to_owned()
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
