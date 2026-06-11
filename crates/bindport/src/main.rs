// SPDX-License-Identifier: MIT

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{ExitCode, ExitStatus},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use bindport_adapters::AdapterKind;
use bindport_core::{
    APPLIED_CONFIG_KEYS, BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, ConfigError,
    DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, FALLBACK_CONFIG_FILE, IdentitySources, LoadedConfig,
    PortRange, SERVICE_NAME, ServiceIdentity, default_fallback_config, detect_git_identity,
    discover_config, resolve_identity,
};
use bindport_registry::{
    REGISTRY_PATH_ENV, Registry, RegistryError, RunStart, default_registry_path,
};
use bindport_runner::{
    AllocationHints, RunnerError, allocate_port_with_hints, is_port_available,
    spawn_child_with_hints,
};

const DOCTOR_PORT_DISPLAY_LIMIT: usize = 10;
const DOCTOR_MAX_LISTENER_PROBES: u32 = 1024;
const ALLOCATION_RETRY_WINDOW: Duration = Duration::from_secs(2);
const MAX_ALLOCATION_RETRIES: usize = 1;

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
        Some("--") => run_wrapped_command(&args[1..], RunOptions::default()),
        Some("run") => run_subcommand(&args[1..]),
        Some(command) => {
            eprintln!("unknown bindport command: {command}");
            eprintln!("run `bindport --help` for available bootstrap commands");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Default)]
struct RunOptions {
    service: Option<String>,
}

fn run_subcommand(args: &[String]) -> ExitCode {
    match args {
        [separator, command @ ..] if separator == "--" => {
            run_wrapped_command(command, RunOptions::default())
        }
        [service, separator, command @ ..] if separator == "--" => run_wrapped_command(
            command,
            RunOptions {
                service: Some(service.clone()),
            },
        ),
        _ => {
            eprintln!("usage: bindport run [service] -- <command>");
            ExitCode::FAILURE
        }
    }
}

fn run_wrapped_command(command: &[String], options: RunOptions) -> ExitCode {
    match run_wrapped_command_result(command, &options) {
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

fn run_wrapped_command_result(
    command: &[String],
    options: &RunOptions,
) -> Result<ExitCode, RunCommandError> {
    if command.is_empty() {
        return Err(RunnerError::NoCommand.into());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let identity = resolve_run_identity(&cwd, command, options, &config);
    let mut registry = open_optional_registry();
    let mut skip_ports = config.skip_ports.clone();
    let mut previous_port = None;

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

        if !disable_registry {
            match registry.previous_identity_port(&identity.identity_key) {
                Ok(port) => previous_port = port,
                Err(error) => {
                    print_registry_warning("failed to read previous identity port", &error);
                }
            }
        }
    }
    if disable_registry {
        registry = None;
        previous_port = None;
    }

    let mut retries = 0;
    let command_display = command.join(" ");

    loop {
        let allocation_hints = AllocationHints {
            preferred_port: previous_port,
            scan_start: identity.port_scan_start(config.port_range),
        };
        let mut child =
            spawn_child_with_hints(command, config.port_range, &skip_ports, allocation_hints)?;
        let attempt_started_at = Instant::now();
        let port = child.port();
        let run = RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity.clone()),
            host: String::from("127.0.0.1"),
            port,
            pid: child.pid(),
            command: command_display.clone(),
            cwd: cwd.clone(),
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
        let attempt_elapsed = attempt_started_at.elapsed();
        let exit_code = status_registry_exit_code(&status);

        if let (Some(registry), Some(started)) = (registry.as_mut(), started)
            && let Err(error) = registry.record_run_finished(started, exit_code)
        {
            print_registry_warning("failed to record run finish", &error);
        }

        if retries < MAX_ALLOCATION_RETRIES
            && should_retry_allocation(&status, attempt_elapsed, port)
        {
            eprintln!(
                "bindport: warning: assigned port {port} became unavailable; retrying with another port"
            );
            skip_ports.push(port);
            retries += 1;
            continue;
        }

        return Ok(status_to_exit_code(&status));
    }
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

fn resolve_run_identity(
    cwd: &Path,
    command: &[String],
    options: &RunOptions,
    config: &ResolvedConfig,
) -> ServiceIdentity {
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let env_service = env::var(BINDPORT_SERVICE_ENV).ok();
    let config_project = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.project.as_deref());
    let config_service = config
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.config.service.as_deref());

    resolve_identity(IdentitySources {
        cwd,
        command,
        cli_project: None,
        cli_service: options.service.as_deref(),
        env_project: env_project.as_deref(),
        env_service: env_service.as_deref(),
        config_project,
        config_service,
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

    let mut registry = print_doctor_registry_path();

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = match resolve_config(&cwd) {
        Ok(config) => {
            print_config_diagnostics(&config);
            config
        }
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    let identity = resolve_run_identity(&cwd, &[], &RunOptions::default(), &config);

    print_identity_diagnostics(&identity);
    print_git_diagnostics(&cwd);
    let allocation_ok = print_allocation_diagnostics(&config, &identity, registry.as_mut());

    println!("first proxy adapter: {}", AdapterKind::Traefik.as_str());
    if allocation_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_doctor_registry_path() -> Option<Registry> {
    match default_registry_path() {
        Ok(path) => match Registry::open(&path) {
            Ok(registry) => {
                println!("registry: {} (ok)", path.display());
                Some(registry)
            }
            Err(error) => {
                println!("registry: {} (unavailable: {error})", path.display());
                None
            }
        },
        Err(error) => {
            println!("registry: unavailable ({error})");
            None
        }
    }
}

fn print_identity_diagnostics(identity: &ServiceIdentity) {
    println!(
        "effective identity: project={} service={}",
        identity.project, identity.service
    );
    println!("identity key: {}", identity.identity_key);
}

fn print_git_diagnostics(cwd: &Path) {
    match detect_git_identity(cwd) {
        Some(git) => {
            println!("git worktree: {}", git.worktree_path.display());
            println!("git branch: {}", git.branch);
            println!("git branch label: {}", git.branch_label);
            println!("git commit: {}", git.commit);
        }
        None => println!("git worktree: none"),
    }
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

fn print_allocation_diagnostics(
    config: &ResolvedConfig,
    identity: &ServiceIdentity,
    registry: Option<&mut Registry>,
) -> bool {
    let mut active_ports = Vec::new();
    let mut previous_port = None;
    let registry_available = registry.is_some();
    let mut active_ports_available = registry_available;
    let mut previous_port_available = registry_available;

    match registry {
        Some(registry) => {
            match registry.active_ports() {
                Ok(ports) => active_ports = ports,
                Err(error) => {
                    println!("registry active ports in range: unavailable ({error})");
                    active_ports_available = false;
                }
            }

            match registry.previous_identity_port(&identity.identity_key) {
                Ok(port) => previous_port = port,
                Err(error) => {
                    println!("previous identity port: unavailable ({error})");
                    previous_port_available = false;
                }
            }
        }
        None => {
            println!("registry active ports in range: unavailable");
            active_ports_available = false;
            previous_port_available = false;
        }
    }

    if active_ports_available {
        let active_in_range = ports_in_range(&active_ports, config.port_range);
        println!(
            "registry active ports in range: {}",
            format_limited_ports(&active_in_range)
        );
    }

    if previous_port_available {
        print_previous_port_diagnostics(previous_port, config, &active_ports);
    }
    let listener_conflicts = listener_conflicts(config.port_range);
    println!(
        "os listener conflicts in range: {}",
        format_listener_conflict_scan(&listener_conflicts)
    );

    let scan_start = identity.port_scan_start(config.port_range);
    match scan_start {
        Some(port) => println!("allocation scan start: {port}"),
        None => println!("allocation scan start: unavailable"),
    }

    let mut skip_ports = config.skip_ports.clone();
    skip_ports.extend(active_ports);
    let allocation_hints = AllocationHints {
        preferred_port: previous_port,
        scan_start,
    };

    match allocate_port_with_hints(config.port_range, &skip_ports, allocation_hints) {
        Ok(port) => {
            let source = if Some(port) == previous_port {
                "sticky"
            } else {
                "scan"
            };
            println!("next candidate port: {port} ({source})");
            true
        }
        Err(error) => {
            println!("next candidate port: unavailable ({error})");
            false
        }
    }
}

fn print_previous_port_diagnostics(
    previous_port: Option<u16>,
    config: &ResolvedConfig,
    active_ports: &[u16],
) {
    let Some(port) = previous_port else {
        println!("previous identity port: none");
        return;
    };

    let status = if !config.port_range.contains(port) {
        "outside range"
    } else if config.skip_ports.contains(&port) {
        "configured skip"
    } else if active_ports.contains(&port) {
        "active registry conflict"
    } else if is_port_available(port) {
        "free"
    } else {
        "os listener conflict"
    };

    println!("previous identity port: {port} ({status})");
}

fn ports_in_range(ports: &[u16], range: PortRange) -> Vec<u16> {
    let mut ports = ports
        .iter()
        .copied()
        .filter(|port| range.contains(*port))
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

struct ListenerConflictScan {
    conflicts: Vec<u16>,
    scanned_ports: u32,
    total_ports: u32,
}

fn listener_conflicts(range: PortRange) -> ListenerConflictScan {
    let total_ports = range.len();
    let scanned_ports = total_ports.min(DOCTOR_MAX_LISTENER_PROBES);
    let conflicts = (0..scanned_ports)
        .filter_map(|offset| {
            let port = range.start as u32 + offset;
            let port = u16::try_from(port).expect("port remains within configured range");

            if is_port_available(port) {
                None
            } else {
                Some(port)
            }
        })
        .collect();

    ListenerConflictScan {
        conflicts,
        scanned_ports,
        total_ports,
    }
}

fn format_listener_conflict_scan(scan: &ListenerConflictScan) -> String {
    let mut summary = format_limited_ports(&scan.conflicts);

    if scan.scanned_ports < scan.total_ports {
        summary.push_str(&format!(
            " (scanned first {} of {} ports)",
            scan.scanned_ports, scan.total_ports
        ));
    }

    summary
}

fn format_limited_ports(ports: &[u16]) -> String {
    if ports.is_empty() {
        return String::from("none");
    }

    let mut summary = ports
        .iter()
        .take(DOCTOR_PORT_DISPLAY_LIMIT)
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    if ports.len() > DOCTOR_PORT_DISPLAY_LIMIT {
        summary.push_str(&format!(
            " (+{} more)",
            ports.len() - DOCTOR_PORT_DISPLAY_LIMIT
        ));
    }

    summary
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

fn should_retry_allocation(status: &ExitStatus, elapsed: Duration, port: u16) -> bool {
    matches!(status.code(), Some(code) if code != 0)
        && elapsed <= ALLOCATION_RETRY_WINDOW
        && !is_port_available(port)
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

fn print_help() {
    println!("BindPort - proxy-neutral local development port registry");
    println!();
    println!("Usage:");
    println!("  bindport -- <command>        Run a command with an assigned PORT");
    println!("  bindport run [service] -- <command>");
    println!("                                  Run a command with an optional service name");
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
