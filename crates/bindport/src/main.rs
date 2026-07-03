// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};

use bindport_adapters::{
    AdapterKind, OutputFileError, OutputFileOwnership as AdapterOutputFileOwnership,
    OutputFileRemovalStatus as AdapterOutputFileRemovalStatus, OutputRenderConfig,
    RemovableOutputFile as AdapterRemovableOutputFile, RenderError, RenderPlan, RouteRecord,
    TemplateError as AdapterTemplateError, TemplateResolver, TemplateSource,
    remove_owned_output_files, render_output_routes, render_plan_paths, verify_render_plan_targets,
    write_render_plan,
};
use bindport_core::{
    APPLIED_CONFIG_KEYS, BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, BindPortConfig,
    CONFIG_FILENAMES, ConfigError, ConfigSource, ConfiguredServiceSource, DEFAULT_HOOK_TIMEOUT_MS,
    DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, EffectiveOutputConfig, FALLBACK_CONFIG_FILE,
    HookCommandConfig, HookEvent, IdentitySources, LoadedConfig, OutputConfigError,
    OutputDeleteState, OutputFailurePolicy, PortRange, SERVICE_NAME, ServiceConfig,
    ServiceIdentity, default_fallback_config, detect_git_identity, discover_config,
    is_restricted_service_env_name, normalize_branch_label, resolve_identity,
};
use bindport_dashboard::{
    DashboardCleanCallback, DashboardOptions, DashboardServer, DashboardStatusCallback,
};
use bindport_registry::{
    CleanState, CleanSummary, OutputFileRecord, OutputFileStatus, REGISTRY_PATH_ENV, Registry,
    RegistryError, RunStart, StartedRun, StatusService, default_registry_path,
    status_service_route_key,
};
use bindport_runner::{
    AllocationHints, RunnerError, allocate_port_with_hints, is_port_available, spawn_child_on_port,
};
use sha2::{Digest, Sha256};

const DOCTOR_PORT_DISPLAY_LIMIT: usize = 10;
const DOCTOR_MAX_LISTENER_PROBES: u32 = 1024;
const ALLOCATION_RETRY_WINDOW: Duration = Duration::from_secs(2);
const MAX_ALLOCATION_RETRIES: usize = 1;
const DASHBOARD_HOST_ENV: &str = "BINDPORT_DASHBOARD_HOST";
const DASHBOARD_PORT_ENV: &str = "BINDPORT_DASHBOARD_PORT";
const DASHBOARD_AUTH_REQUIRED_ENV: &str = "BINDPORT_DASHBOARD_AUTH_REQUIRED";
const DASHBOARD_REGISTER_SERVICE_ENV: &str = "BINDPORT_DASHBOARD_REGISTER_SERVICE";
const DASHBOARD_TOKEN_ENV: &str = "BINDPORT_DASHBOARD_TOKEN";
const DASHBOARD_STATIC_DIR_ENV: &str = "BINDPORT_DASHBOARD_STATIC_DIR";
const BINDPORT_HOSTNAME_ENV: &str = "BINDPORT_HOSTNAME";
const BINDPORT_ROUTE_URL_ENV: &str = "BINDPORT_ROUTE_URL";
const BINDPORT_HEALTH_URL_ENV: &str = "BINDPORT_HEALTH_URL";
const DASHBOARD_STATE_FILE: &str = "dashboard.state";
const DASHBOARD_LOG_FILE: &str = "dashboard.log";
const HOOK_TRUST_FILE: &str = "hooks-trust.json";
const HOOK_TRUST_SCHEMA_VERSION: &str = "0.1";

mod clean;
mod config;
mod dashboard;
mod doctor;
mod errors;
mod help;
mod hooks;
mod open;
mod paths;
mod ports;
mod render;
mod route_events;
mod run;
mod status;
mod templates;

pub(crate) use clean::*;
pub(crate) use config::*;
pub(crate) use dashboard::*;
pub(crate) use doctor::*;
pub(crate) use errors::*;
pub(crate) use help::*;
pub(crate) use hooks::*;
pub(crate) use open::*;
pub(crate) use paths::*;
pub(crate) use ports::*;
pub(crate) use render::*;
pub(crate) use route_events::*;
pub(crate) use run::*;
pub(crate) use status::*;
pub(crate) use templates::*;

fn main() -> ExitCode {
    dispatch(env::args().skip(1))
}

fn dispatch(args: impl IntoIterator<Item = String>) -> ExitCode {
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
        Some("open") => run_open_command(&args[1..]),
        Some("clean") => clean_registry(&args[1..]),
        Some("config") => run_config_command(&args[1..]),
        Some("doctor") => run_doctor_command(&args[1..]),
        Some("dashboard") => run_dashboard(&args[1..]),
        Some("hooks") => run_hooks_command(&args[1..]),
        Some("render") => run_render_command(&args[1..]),
        Some("templates") => run_template_command(&args[1..]),
        Some("init") => run_init_command(&args[1..]),
        Some("--") => run_wrapped_command(&args[1..], RunOptions::default()),
        Some("run") => run_subcommand(&args[1..]),
        Some(command) => {
            eprintln!("unknown bindport command: {command}");
            eprintln!("run `bindport --help` for available bootstrap commands");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "unit_tests/mod.rs"]
mod tests;
