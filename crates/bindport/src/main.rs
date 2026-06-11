// SPDX-License-Identifier: MIT

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
    process::ExitStatus,
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use bindport_adapters::AdapterKind;
use bindport_core::{
    APPLIED_CONFIG_KEYS, ConfigError, DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, FALLBACK_CONFIG_FILE,
    LoadedConfig, PortRange, SERVICE_NAME, default_fallback_config, discover_config,
};
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
        Some("doctor") => print_doctor(),
        Some("init") => init_fallback_config(),
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
        Err(RunCommandError::Runner(error)) => {
            print_runner_error(&error);
            ExitCode::FAILURE
        }
        Err(RunCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run_wrapped_command_result(command: &[String]) -> Result<ExitCode, RunCommandError> {
    if command.is_empty() {
        return Err(RunnerError::NoCommand.into());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let mut registry = open_optional_registry();
    let mut skip_ports = config.skip_ports.clone();

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

    let mut child = spawn_child(command, config.port_range, &skip_ports)?;
    let run = RunStart {
        project: config.project_name(&cwd),
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

#[derive(Debug)]
enum RunCommandError {
    Runner(RunnerError),
    Config(ConfigError),
}

impl From<RunnerError> for RunCommandError {
    fn from(error: RunnerError) -> Self {
        Self::Runner(error)
    }
}

impl From<ConfigError> for RunCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

struct ResolvedConfig {
    loaded: Option<LoadedConfig>,
    fallback_path: Option<PathBuf>,
    port_range: PortRange,
    skip_ports: Vec<u16>,
}

impl ResolvedConfig {
    fn project_name(&self, cwd: &Path) -> String {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.config.project.as_deref())
            .map(str::to_owned)
            .unwrap_or_else(|| infer_project_name(cwd))
    }
}

fn resolve_config(cwd: &Path) -> Result<ResolvedConfig, ConfigError> {
    let fallback_path = fallback_config_path().ok();
    let loaded = discover_config(cwd, fallback_path.as_deref())?;
    let port_range = loaded
        .as_ref()
        .map(LoadedConfig::port_range)
        .transpose()?
        .unwrap_or(DEFAULT_PORT_RANGE);
    let skip_ports = loaded
        .as_ref()
        .map(LoadedConfig::skip_ports)
        .unwrap_or_else(|| DEFAULT_SKIP_PORTS.to_vec());

    Ok(ResolvedConfig {
        loaded,
        fallback_path,
        port_range,
        skip_ports,
    })
}

fn fallback_config_path() -> Result<PathBuf, RegistryError> {
    let registry_path = default_registry_path()?;
    let path = registry_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(FALLBACK_CONFIG_FILE))
        .unwrap_or_else(|| PathBuf::from(FALLBACK_CONFIG_FILE));

    Ok(path)
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

fn print_doctor() -> ExitCode {
    println!("BindPort bootstrap doctor");

    match default_registry_path() {
        Ok(path) => println!("registry: {}", path.display()),
        Err(error) => println!("registry: unavailable ({error})"),
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    match resolve_config(&cwd) {
        Ok(config) => print_config_diagnostics(&config),
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    }

    println!("first proxy adapter: {}", AdapterKind::Traefik.as_str());
    ExitCode::SUCCESS
}

fn print_config_diagnostics(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => println!(
            "config: {} ({} {})",
            loaded.path.display(),
            loaded.source.as_str(),
            loaded.format.as_str()
        ),
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }

    if let Some(loaded) = config.loaded.as_ref()
        && !loaded.unknown_keys.is_empty()
    {
        println!(
            "config warning: ignored unknown top-level keys: {}",
            loaded.unknown_keys.join(", ")
        );
        println!("config applied keys: {}", APPLIED_CONFIG_KEYS.join(", "));
    }

    println!(
        "effective port range: {}-{}",
        config.port_range.start, config.port_range.end
    );
    println!("skip ports: {}", config.skip_ports.len());
}

fn init_fallback_config() -> ExitCode {
    match write_fallback_config() {
        Ok(InitConfigResult::Created(path)) => {
            println!("created config: {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(InitConfigResult::AlreadyExists(path)) => {
            println!("config already exists: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bindport: failed to initialize fallback config: {error}");
            ExitCode::FAILURE
        }
    }
}

enum InitConfigResult {
    Created(PathBuf),
    AlreadyExists(PathBuf),
}

fn write_fallback_config() -> io::Result<InitConfigResult> {
    let path = fallback_config_path().map_err(io::Error::other)?;

    if path.is_file() {
        return Ok(InitConfigResult::AlreadyExists(path));
    }

    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("`{}` exists but is not a file", path.display()),
        ));
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, default_fallback_config())?;

    Ok(InitConfigResult::Created(path))
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

fn print_config_error(error: &ConfigError) {
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
    println!("  bindport init                Create optional fallback config");
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
