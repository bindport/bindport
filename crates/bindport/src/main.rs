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
    APPLIED_CONFIG_KEYS, BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, BindPortConfig, ConfigError,
    ConfigSource, ConfiguredServiceSource, DEFAULT_HOOK_TIMEOUT_MS, DEFAULT_PORT_RANGE,
    DEFAULT_SKIP_PORTS, EffectiveOutputConfig, FALLBACK_CONFIG_FILE, HookCommandConfig, HookEvent,
    IdentitySources, LoadedConfig, OutputConfigError, OutputDeleteState, OutputFailurePolicy,
    PortRange, SERVICE_NAME, ServiceConfig, ServiceIdentity, default_fallback_config,
    detect_git_identity, discover_config, is_restricted_service_env_name, normalize_branch_label,
    resolve_identity,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bindport_core::HooksConfig;
    use std::collections::BTreeMap;

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

    #[test]
    fn route_event_collector_retains_source_and_kind() {
        let empty = RouteEventCollector::default();
        assert!(empty.is_empty());
        assert_eq!(empty.warning_context(), "route event");

        let mut collector =
            RouteEventCollector::single(RouteEventSource::CliRunner, RouteEventKind::RouteStarted);
        collector.record(
            RouteEventSource::StaleReconcile,
            RouteEventKind::RoutesMarkedStale,
        );

        assert_eq!(
            collector.events(),
            &[
                RouteEvent::new(RouteEventSource::CliRunner, RouteEventKind::RouteStarted),
                RouteEvent::new(
                    RouteEventSource::StaleReconcile,
                    RouteEventKind::RoutesMarkedStale,
                )
            ]
        );
        assert_eq!(
            collector.warning_context(),
            "route events from cli_runner,stale_reconcile"
        );
        assert_eq!(collector.hook_sources(), "cli_runner,stale_reconcile");
        assert_eq!(
            collector.hook_events(false),
            BTreeSet::from([HookEvent::RouteStarted, HookEvent::RoutesMarkedStale,])
        );
        assert_eq!(
            collector.hook_events(true),
            BTreeSet::from([
                HookEvent::RouteStarted,
                HookEvent::RoutesMarkedStale,
                HookEvent::OutputRendered,
            ])
        );

        let single = RouteEventCollector::single(
            RouteEventSource::DashboardClean,
            RouteEventKind::RoutesRemoved,
        );
        assert_eq!(single.warning_context(), "dashboard_clean routes_removed");
        assert_eq!(RouteEventSource::CliClean.as_str(), "cli_clean");
        assert_eq!(RouteEventSource::ManualRender.as_str(), "manual_render");
        assert_eq!(RouteEventKind::RouteFinished.as_str(), "route_finished");
        assert_eq!(RouteEventKind::RenderRequested.as_str(), "render_requested");
    }

    #[test]
    fn hook_plan_reports_source_without_granting_trust() {
        let project_hooks = HooksConfig {
            commands: Some(vec![hook_command("reload")]),
            ..HooksConfig::default()
        };
        let cwd = Path::new("/workspace/demo");
        let project_plan = configured_hook_plan(
            cwd,
            &hook_resolved_config(ConfigSource::Project, project_hooks.clone(), None),
        )
        .expect("project hook plan");
        assert_eq!(
            project_plan.hooks[0].source,
            "project config `/workspace/demo/bindport.toml`"
        );

        let local_command = hook_command("local-reload");
        let local_commands = configured_hook_plan(
            cwd,
            &hook_resolved_config(
                ConfigSource::Project,
                HooksConfig {
                    commands: Some(vec![hook_command("project-reload")]),
                    ..HooksConfig::default()
                },
                Some(HooksConfig {
                    commands: Some(vec![local_command]),
                    ..HooksConfig::default()
                }),
            ),
        )
        .expect("local hook plan");
        assert_eq!(local_commands.hooks[0].name, "local-reload");
        assert_eq!(
            local_commands.hooks[0].source,
            "local override config `/workspace/demo/.bindport.local.toml`"
        );

        let fallback = configured_hook_plan(
            cwd,
            &hook_resolved_config(
                ConfigSource::Fallback,
                HooksConfig {
                    commands: Some(vec![hook_command("fallback-reload")]),
                    ..HooksConfig::default()
                },
                None,
            ),
        )
        .expect("fallback hook plan");
        assert_eq!(
            fallback.hooks[0].source,
            "fallback config `/home/user/.config/bindport/config.toml`"
        );
    }

    #[test]
    fn hook_trust_status_requires_exact_user_scoped_match() {
        let cwd = Path::new("/workspace/demo");
        let plan = configured_hook_plan(
            cwd,
            &hook_resolved_config(
                ConfigSource::Project,
                HooksConfig {
                    commands: Some(vec![hook_command("reload")]),
                    ..HooksConfig::default()
                },
                None,
            ),
        )
        .expect("hook plan");
        let hook = &plan.hooks[0];
        let subjects = hook_trust_subjects(cwd);
        let mut store = HookTrustStore::default();

        assert_eq!(
            hook_trust_status(hook, &store, &subjects),
            HookTrustStatus::Pending
        );

        upsert_hook_trust_entry(
            &mut store,
            &subjects,
            HookTrustScope::Worktree,
            hook,
            HookDecision::Approved,
        )
        .expect("approve hook");
        assert_eq!(
            hook_trust_status(hook, &store, &subjects),
            HookTrustStatus::Approved {
                scope: HookTrustScope::Worktree
            }
        );

        let mut changed_hook = hook.clone();
        changed_hook.definition.push_str("changed\n");
        assert_eq!(
            hook_trust_status(&changed_hook, &store, &subjects),
            HookTrustStatus::Changed
        );

        upsert_hook_trust_entry(
            &mut store,
            &subjects,
            HookTrustScope::Worktree,
            hook,
            HookDecision::Denied,
        )
        .expect("deny hook");
        assert_eq!(
            hook_trust_status(hook, &store, &subjects),
            HookTrustStatus::Denied {
                scope: HookTrustScope::Worktree
            }
        );
    }

    #[test]
    fn run_option_parser_accepts_valid_options_and_rejects_bad_env_names() {
        let args = strings([
            "web",
            "--env",
            "NEXT_PUBLIC_URL={route_url}",
            "--hostname",
            "{branch}.{project}.localhost",
            "--route-url",
            "https://{hostname}",
            "--health-url",
            "{route_url}/health",
            "--",
            "next",
            "dev",
        ]);
        let (options, command) = parse_run_options(&args).expect("run options");

        assert_eq!(options.service.as_deref(), Some("web"));
        assert_eq!(
            options.hostname.as_deref(),
            Some("{branch}.{project}.localhost")
        );
        assert_eq!(options.route_url.as_deref(), Some("https://{hostname}"));
        assert_eq!(options.health_url.as_deref(), Some("{route_url}/health"));
        assert_eq!(
            options.env,
            vec![(String::from("NEXT_PUBLIC_URL"), String::from("{route_url}"))]
        );
        assert_eq!(command, strings(["next", "dev"]).as_slice());
        let service_only = strings(["web"]);
        let (options, command) = parse_run_options(&service_only).expect("service-only options");
        assert_eq!(options.service.as_deref(), Some("web"));
        assert!(command.is_empty());

        assert_eq!(
            parse_env_assignment("PORT").expect_err("missing assignment"),
            "invalid env assignment `PORT`; expected NAME=VALUE"
        );
        assert_eq!(
            parse_env_assignment("1PORT=3000").expect_err("bad name"),
            "invalid env variable name `1PORT`"
        );
        assert!(valid_env_name("_PORT"));
        assert!(valid_env_name("NEXT_PUBLIC_URL"));
        assert!(!valid_env_name(""));
        assert!(!valid_env_name("PORT-NAME"));
    }

    #[test]
    fn run_metadata_expands_route_and_env_templates() {
        let identity = ServiceIdentity {
            project: String::from("example-app"),
            service: String::from("web"),
            git: None,
            identity_key: String::from("v1:test"),
        };
        let templates = RunTemplates {
            command: Some(vec![
                String::from("storybook"),
                String::from("--port"),
                String::from("{port}"),
            ]),
            hostname: Some(String::from("{service}.{project}.localhost")),
            route_url: Some(String::from("https://{hostname}")),
            health_url: Some(String::from("{route_url}/health")),
            env: vec![
                (String::from("URL"), String::from("{route_url}")),
                (String::from("HEALTH"), String::from("{health_url}")),
                (String::from("JSON"), String::from(r#"{{"port":{port}}}"#)),
            ],
        };

        let metadata = resolve_run_metadata(&identity, 29100, &templates).expect("metadata");

        assert_eq!(
            metadata.hostname.as_deref(),
            Some("web.example-app.localhost")
        );
        assert_eq!(
            metadata.route_url.as_deref(),
            Some("https://web.example-app.localhost")
        );
        assert_eq!(
            metadata.health_url.as_deref(),
            Some("https://web.example-app.localhost/health")
        );
        assert_eq!(
            metadata.env,
            vec![
                (
                    String::from("URL"),
                    String::from("https://web.example-app.localhost")
                ),
                (
                    String::from("HEALTH"),
                    String::from("https://web.example-app.localhost/health")
                ),
                (String::from("JSON"), String::from(r#"{"port":29100}"#)),
            ]
        );
        assert_eq!(
            metadata.command,
            Some(vec![
                String::from("storybook"),
                String::from("--port"),
                String::from("29100"),
            ])
        );
    }

    #[test]
    fn template_expansion_reports_syntax_errors() {
        let identity = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("web"),
            git: None,
            identity_key: String::from("v1:test"),
        };
        let values = TemplateValues::new(&identity, 29100, None, None, None);

        assert!(matches!(
            expand_template("{project", &values),
            Err(TemplateError::Unclosed { .. })
        ));
        assert_eq!(
            expand_template("{project", &values)
                .expect_err("unclosed")
                .to_string(),
            "unclosed template placeholder in `{project`"
        );
        assert!(matches!(
            expand_template("project}", &values),
            Err(TemplateError::Unopened { .. })
        ));
        assert_eq!(
            expand_template("project}", &values)
                .expect_err("unopened")
                .to_string(),
            "unmatched `}` in template `project}`"
        );
        assert!(matches!(
            expand_template("{missing}", &values),
            Err(TemplateError::UnknownPlaceholder { .. })
        ));
        assert_eq!(
            expand_template("{missing}", &values)
                .expect_err("unknown placeholder")
                .to_string(),
            "unknown or unavailable template placeholder `missing` in `{missing}`"
        );
    }

    #[test]
    fn template_values_include_git_and_fallback_context() {
        let identity = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("web"),
            git: Some(bindport_core::GitIdentity {
                worktree_path: PathBuf::from("/workspace/demo-feature-tree"),
                worktree_hash: String::from("abc123456789"),
                git_common_dir: PathBuf::from("/workspace/demo/.git"),
                branch: String::from("feature/tree"),
                branch_label: String::from("feature-tree"),
                commit: String::from("0123456789abcdef"),
            }),
            identity_key: String::from("v1:test"),
        };
        let values = TemplateValues::new(
            &identity,
            29_100,
            Some("feature-tree.demo.localhost"),
            Some("https://feature-tree.demo.localhost"),
            Some("https://feature-tree.demo.localhost/health"),
        );

        assert_eq!(values.value("port").as_deref(), Some("29100"));
        assert_eq!(values.value("host").as_deref(), Some("127.0.0.1"));
        assert_eq!(
            values.value("url").as_deref(),
            Some("http://127.0.0.1:29100")
        );
        assert_eq!(values.value("project").as_deref(), Some("demo"));
        assert_eq!(values.value("service").as_deref(), Some("web"));
        assert_eq!(
            values.value("hostname").as_deref(),
            Some("feature-tree.demo.localhost")
        );
        assert_eq!(
            values.value("route_url").as_deref(),
            Some("https://feature-tree.demo.localhost")
        );
        assert_eq!(
            values.value("health_url").as_deref(),
            Some("https://feature-tree.demo.localhost/health")
        );
        assert_eq!(values.value("branch").as_deref(), Some("feature-tree"));
        assert_eq!(
            values.value("branch_label").as_deref(),
            Some("feature-tree")
        );
        assert_eq!(values.value("git_branch").as_deref(), Some("feature/tree"));
        assert_eq!(
            values.value("worktree").as_deref(),
            Some("demo-feature-tree")
        );
        assert_eq!(
            values.value("worktree_label").as_deref(),
            Some("demo-feature-tree")
        );
        assert_eq!(
            values.value("worktree_hash").as_deref(),
            Some("abc123456789")
        );
        assert_eq!(values.value("missing"), None);

        let no_git = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("api"),
            git: None,
            identity_key: String::from("v1:no-git"),
        };
        let values = TemplateValues::new(&no_git, 29_101, None, None, None);
        assert_eq!(
            values.value("route_url").as_deref(),
            Some("http://127.0.0.1:29101")
        );
        assert_eq!(values.value("branch").as_deref(), Some("no-branch"));
        assert_eq!(values.value("git_branch").as_deref(), Some("no-branch"));
        assert_eq!(values.value("worktree").as_deref(), Some("demo"));
        assert_eq!(values.value("worktree_hash").as_deref(), Some("no-git"));
    }

    #[test]
    fn dashboard_command_parser_preserves_serve_args_and_modes() {
        let args = strings([
            "start",
            "--host",
            "0.0.0.0",
            "--port",
            "27081",
            "--auth",
            "required",
            "--register-service",
            "--token",
            "secret",
            "--token-env",
            "CUSTOM_TOKEN",
            "--allowed-host",
            "devbox.test",
            "--static-dir",
            "static",
        ]);
        let (command, options) = parse_dashboard_command(&args).expect("dashboard command");

        assert_eq!(command, DashboardCommand::Start);
        assert_eq!(options.host, Some(Ipv4Addr::UNSPECIFIED));
        assert_eq!(options.port, Some(27081));
        assert_eq!(options.auth_required, Some(true));
        assert_eq!(options.register_service, Some(true));
        assert_eq!(options.token.as_deref(), Some("secret"));
        assert_eq!(options.token_env.as_deref(), Some("CUSTOM_TOKEN"));
        assert_eq!(options.allowed_hosts, vec![String::from("devbox.test")]);
        assert_eq!(options.static_dir, Some(PathBuf::from("static")));
        assert_eq!(
            options.serve_args,
            strings([
                "--host",
                "0.0.0.0",
                "--port",
                "27081",
                "--auth",
                "required",
                "--register-service",
                "--token-env",
                "CUSTOM_TOKEN",
                "--allowed-host",
                "devbox.test",
                "--static-dir",
                "static",
            ])
        );

        let (command, options) =
            parse_dashboard_command(&strings(["--help"])).expect("dashboard help");
        assert_eq!(command, DashboardCommand::Help);
        assert_eq!(options.token_env_name(), DASHBOARD_TOKEN_ENV);
        assert!(parse_dashboard_command(&strings(["--port", "bad"])).is_err());
        assert!(parse_dashboard_command(&strings(["--host", "bad"])).is_err());
        assert!(parse_dashboard_command(&strings(["--auth", "maybe"])).is_err());
        assert!(parse_dashboard_command(&strings(["--missing"])).is_err());
        assert!(parse_dashboard_command(&strings(["--token"])).is_err());

        let (command, _) = parse_dashboard_command(&[]).expect("default serve");
        assert_eq!(command, DashboardCommand::Serve);
        let (command, _) = parse_dashboard_command(&strings(["serve"])).expect("serve");
        assert_eq!(command, DashboardCommand::Serve);
        let (command, _) = parse_dashboard_command(&strings(["status"])).expect("status");
        assert_eq!(command, DashboardCommand::Status);
        let (command, _) = parse_dashboard_command(&strings(["stop"])).expect("stop");
        assert_eq!(command, DashboardCommand::Stop);

        let (_, options) =
            parse_dashboard_command(&strings(["--auth-required", "--no-register-service"]))
                .expect("boolean dashboard flags");
        assert_eq!(options.auth_required, Some(true));
        assert_eq!(options.register_service, Some(false));
        assert_eq!(
            options.serve_args,
            strings(["--auth-required", "--no-register-service"])
        );

        assert!(parse_dashboard_auth_mode("enabled").expect("enabled"));
        assert!(!parse_dashboard_auth_mode("disabled").expect("disabled"));
        assert!(parse_dashboard_bool("yes", "setting").expect("yes"));
        assert!(!parse_dashboard_bool("no", "setting").expect("no"));
    }

    #[test]
    fn dashboard_option_resolution_enforces_auth_and_precedence() {
        let config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: PathBuf::from("/workspace/demo/bindport.toml"),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Project,
                local_override: None,
                config: BindPortConfig {
                    dashboard: Some(bindport_core::DashboardConfig {
                        host: Some(String::from("127.0.0.2")),
                        port: Some(27_081),
                        register_service: Some(true),
                        allowed_hosts: Some(vec![
                            String::from("config.test"),
                            String::from("localhost"),
                        ]),
                        auth: Some(bindport_core::DashboardAuthConfig {
                            required: Some(true),
                            token: Some(String::from("config-token")),
                            token_env: Some(String::from("CONFIG_DASHBOARD_TOKEN")),
                        }),
                    }),
                    ..BindPortConfig::default()
                },
                unknown_keys: Vec::new(),
            }),
            fallback_path: None,
            port_range: PortRange {
                start: 29_100,
                end: 29_110,
            },
            skip_ports: vec![29_101],
        };
        let cli = DashboardCliOptions {
            host: Some(Ipv4Addr::LOCALHOST),
            port: Some(27_082),
            auth_required: Some(true),
            register_service: Some(false),
            token: Some(String::from("cli-token")),
            allowed_hosts: vec![String::from("cli.test")],
            static_dir: Some(PathBuf::from("dashboard/static")),
            ..DashboardCliOptions::default()
        };

        let options =
            resolve_dashboard_options(&config, &cli, vec![29_102]).expect("dashboard options");

        assert_eq!(options.host, Ipv4Addr::LOCALHOST);
        assert_eq!(options.preferred_port, 27_082);
        assert_eq!(options.fallback_range.start, 29_100);
        assert_eq!(options.skip_ports, vec![29_102]);
        assert!(options.allowed_hosts.contains(&String::from("127.0.0.1")));
        assert!(options.allowed_hosts.contains(&String::from("cli.test")));
        assert!(options.allowed_hosts.contains(&String::from("config.test")));
        assert!(options.auth.required);
        assert_eq!(options.auth.token.as_deref(), Some("cli-token"));
        assert_eq!(options.static_dir, Some(PathBuf::from("dashboard/static")));
        assert!(!resolve_dashboard_registration(&config, &cli).expect("registration"));

        let non_loopback_without_auth = DashboardCliOptions {
            host: Some(Ipv4Addr::UNSPECIFIED),
            auth_required: Some(false),
            ..DashboardCliOptions::default()
        };
        assert!(matches!(
            resolve_dashboard_options(&config, &non_loopback_without_auth, Vec::new()),
            Err(DashboardCommandError::InvalidArgument(message))
                if message.contains("requires auth")
        ));

        let missing_token = DashboardCliOptions {
            auth_required: Some(true),
            token_env: Some(String::from("BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST")),
            ..DashboardCliOptions::default()
        };
        let config_without_token = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };
        assert!(matches!(
            resolve_dashboard_options(&config_without_token, &missing_token, Vec::new()),
            Err(DashboardCommandError::MissingToken { source_name })
                if source_name == "BINDPORT_COVERAGE_TOKEN_DOES_NOT_EXIST"
        ));
    }

    #[test]
    fn render_command_parser_and_output_selection_validate_combinations() {
        let (command, options) =
            parse_render_command(&strings(["traefik", "--dry-run"])).expect("render command");
        assert_eq!(command, RenderCommand::Render);
        assert_eq!(options.output.as_deref(), Some("traefik"));
        assert!(options.dry_run);

        let (command, _) = parse_render_command(&strings(["--help"])).expect("render help");
        assert_eq!(command, RenderCommand::Help);
        assert!(parse_render_command(&strings(["--all", "traefik"])).is_err());
        assert!(parse_render_command(&strings(["--dry-run", "--repair"])).is_err());
        assert!(parse_render_command(&strings(["traefik", "debug"])).is_err());

        let outputs = vec![EffectiveOutputConfig {
            name: String::from("traefik"),
            template: String::from("bindport-traefik"),
            root: None,
            target: String::from("{{ route.slug }}.yml"),
            target_host: String::from("127.0.0.1"),
            target_scheme: String::from("http"),
            auto_render: true,
            delete_on: Vec::new(),
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 0,
            vars: BTreeMap::new(),
        }];
        let selected = selected_outputs(outputs.clone(), Some("traefik")).expect("selected");
        assert_eq!(selected.len(), 1);
        assert!(selected_outputs(outputs, Some("missing")).is_err());
    }

    #[test]
    fn template_command_parser_validates_sources_and_names() {
        let (command, options) = parse_template_command(&strings([
            "show",
            "--source",
            "built-in",
            "bindport-traefik",
        ]))
        .expect("template command");
        assert_eq!(command, TemplateCommand::Show);
        assert_eq!(options.source, Some(TemplateSource::BuiltIn));
        assert_eq!(options.name.as_deref(), Some("bindport-traefik"));

        let (command, _) = parse_template_command(&strings(["-h"])).expect("template help");
        assert_eq!(command, TemplateCommand::Help);
        assert_eq!(
            parse_template_source("builtin").expect("builtin alias"),
            TemplateSource::BuiltIn
        );
        assert!(parse_template_command(&strings(["list", "extra"])).is_err());
        assert!(parse_template_command(&strings(["show"])).is_err());
        assert!(parse_template_command(&strings(["show", "a", "b"])).is_err());
        assert!(parse_template_command(&strings(["show", "--source"])).is_err());
        assert!(parse_template_command(&strings(["show", "--source", "bad", "name"])).is_err());
        assert!(parse_template_command(&strings(["bad"])).is_err());
        assert!(parse_template_command(&strings(["show", "--bad", "name"])).is_err());
    }

    #[test]
    fn clean_option_parser_defaults_and_validates_states() {
        let options = parse_clean_options(&[]).expect("default clean options");
        assert_eq!(
            options.states(),
            vec![CleanState::Stopped, CleanState::Stale]
        );
        assert!(!options.dry_run);
        assert!(!options.json);

        let options =
            parse_clean_options(&strings(["--dry-run", "--json", "--stopped"])).expect("clean");
        assert_eq!(options.states(), vec![CleanState::Stopped]);
        assert!(options.dry_run);
        assert!(options.json);
        assert!(!options.yes);

        let options = parse_clean_options(&strings(["--stale", "--yes"])).expect("stale clean");
        assert_eq!(options.states(), vec![CleanState::Stale]);
        assert!(options.yes);

        let options = parse_clean_options(&strings(["--help"])).expect("help clean");
        assert!(options.help);
        assert_eq!(
            options.states(),
            vec![CleanState::Stopped, CleanState::Stale]
        );
        assert!(parse_clean_options(&strings(["--bad"])).is_err());
    }

    #[test]
    fn open_option_parser_and_selection_handle_agent_url_lookup() {
        let options = parse_open_options(&strings(["web", "--project", "demo", "--print"]))
            .expect("open options");
        assert_eq!(options.service.as_deref(), Some("web"));
        assert_eq!(options.project.as_deref(), Some("demo"));
        assert!(!options.browser);

        let options = parse_open_options(&strings(["api", "--browser"])).expect("browser open");
        assert_eq!(options.service.as_deref(), Some("api"));
        assert!(options.browser);

        assert!(parse_open_options(&strings(["web", "api"])).is_err());
        assert!(parse_open_options(&strings(["--project"])).is_err());

        let web = status_service("open-web", "active", None);
        let mut api = status_service("open-api", "active", None);
        api.service = String::from("api");
        api.route_url = None;

        assert_eq!(
            best_service_url(&web),
            "https://feature-tree.demo.localhost"
        );
        assert_eq!(best_service_url(&api), "http://127.0.0.1:29100");
        assert_eq!(
            validate_browser_url(" https://feature-tree.demo.localhost/path ").expect("https"),
            "https://feature-tree.demo.localhost/path"
        );
        assert_eq!(
            validate_browser_url("HTTP://127.0.0.1:29100").expect("http"),
            "HTTP://127.0.0.1:29100"
        );
        assert!(validate_browser_url("file:///tmp/bindport").is_err());
        assert!(validate_browser_url("-psn_0_123").is_err());
        assert!(validate_browser_url("http:example.com").is_err());
        assert!(validate_browser_url("https:///missing-host").is_err());

        let services = vec![web, api];
        let selected = select_open_service(
            &services,
            &OpenOptions {
                service: Some(String::from("api")),
                ..OpenOptions::default()
            },
        )
        .expect("select api");
        assert_eq!(selected.service, "api");

        assert!(select_open_service(&services, &OpenOptions::default()).is_err());

        let stopped_web = status_service("open-stopped", "stopped", Some("2026-06-29T00:01:00Z"));
        assert!(
            select_open_service(
                &[stopped_web],
                &OpenOptions {
                    service: Some(String::from("web")),
                    ..OpenOptions::default()
                },
            )
            .is_err()
        );
    }

    #[test]
    fn config_and_doctor_formatting_helpers_are_stable() {
        assert_eq!(optional_config_value(Some(" value ")), "value");
        assert_eq!(optional_config_value(Some("  ")), "<unset>");
        assert_eq!(optional_config_value(None), "<unset>");
        assert_eq!(configured_value(true), "configured");
        assert_eq!(configured_value(false), "<unset>");
        assert_eq!(list_config_value(Some(1), "entry"), "1 entry");
        assert_eq!(list_config_value(Some(2), "entry"), "2 entries");
        assert_eq!(list_config_value(Some(3), "port"), "3 ports");
        assert_eq!(list_config_value(None, "port"), "<unset>");
        assert_eq!(plural(1, "route"), "route");
        assert_eq!(plural(2, "route"), "routes");
        assert_eq!(source_config_label(ConfigSource::Project), "project config");
        assert_eq!(
            source_config_label(ConfigSource::Fallback),
            "fallback config"
        );
        assert_eq!(non_empty_value(Some("  web  ")), Some("web"));
        assert_eq!(non_empty_value(Some("  ")), None);

        let range = PortRange {
            start: 29_100,
            end: 29_105,
        };
        assert_eq!(
            ports_in_range(&[29_102, 29_102, 29_000, 29_101], range),
            vec![29_101, 29_102]
        );
        assert_eq!(format_limited_ports(&[]), "none");
        assert_eq!(format_limited_ports(&[1, 2, 3]), "1, 2, 3");
        let many_ports = (1..=12).collect::<Vec<u16>>();
        assert_eq!(
            format_limited_ports(&many_ports),
            "1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (+2 more)"
        );
        assert_eq!(
            stale_lease_prune_limit(
                PortRange {
                    start: 29_100,
                    end: 29_110,
                },
                &[29_100, 29_101, 30_000],
            ),
            4
        );
        let scan = ListenerConflictScan {
            known_registry: vec![29_100],
            unknown: vec![29_101],
            scanned_ports: 2,
            total_ports: 10,
        };
        assert_eq!(
            format_listener_conflict_scan(&scan),
            "29101 (scanned first 2 of 10 ports)"
        );
    }

    #[test]
    fn config_and_doctor_diagnostics_render_constructed_state() {
        let cwd = Path::new("/workspace/demo/apps/web");
        let local_override = bindport_core::LoadedLocalConfig {
            path: PathBuf::from("/workspace/demo/.bindport.local.toml"),
            format: bindport_core::ConfigFormat::Toml,
            git_tracked: false,
            config: BindPortConfig {
                project: Some(String::from("local-demo")),
                service: Some(String::from("web")),
                services: Some(vec![ServiceConfig {
                    name: Some(String::from("web")),
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                }]),
                outputs: Some(vec![bindport_core::OutputConfig {
                    name: Some(String::from("local-output")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("routes/{{ route.slug }}.yml")),
                    ..bindport_core::OutputConfig::default()
                }]),
                ..BindPortConfig::default()
            },
            unknown_keys: vec![String::from("local_unknown")],
        };
        let config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: PathBuf::from("/workspace/demo/bindport.toml"),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Project,
                local_override: Some(local_override),
                config: BindPortConfig {
                    project: Some(String::from("demo")),
                    service: Some(String::from("web")),
                    default_range: Some(String::from("29100-29110")),
                    skip_ports: Some(vec![29_101, 29_102]),
                    services: Some(vec![ServiceConfig {
                        name: Some(String::from("web")),
                        path: Some(String::from("apps/web")),
                        command: Some(vec![String::from("next"), String::from("dev")]),
                        ..ServiceConfig::default()
                    }]),
                    dashboard: Some(bindport_core::DashboardConfig::default()),
                    output_defaults: Some(bindport_core::OutputDefaultsConfig {
                        root: Some(String::from(".bindport/out")),
                        ..bindport_core::OutputDefaultsConfig::default()
                    }),
                    outputs: Some(vec![bindport_core::OutputConfig {
                        name: Some(String::from("traefik")),
                        template: Some(String::from("bindport-traefik")),
                        target: Some(String::from("routes/{{ route.slug }}.yml")),
                        auto_render: Some(true),
                        delete_on: Some(vec![OutputDeleteState::Removed]),
                        ..bindport_core::OutputConfig::default()
                    }]),
                    ..BindPortConfig::default()
                },
                unknown_keys: vec![String::from("mystery")],
            }),
            fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
            port_range: PortRange {
                start: 29_100,
                end: 29_110,
            },
            skip_ports: vec![29_101, 29_102],
        };
        let no_config = ResolvedConfig {
            loaded: None,
            fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };

        print_config_source_explanation(&config);
        print_config_source_explanation(&no_config);
        print_config_field_explanations(&config);
        print_config_field_explanations(&no_config);
        print_doctor_output_config(&config);
        print_doctor_output_config(&no_config);
        print_config_diagnostics(&config);
        print_config_diagnostics(&no_config);

        let identity = explain_run_identity(
            cwd,
            &strings(["next", "dev"]),
            &RunOptions::default(),
            &config,
        );
        print_identity_diagnostics(&identity.identity);
        assert_eq!(identity.project_source, "local override config `project`");
        assert_eq!(identity.service_source, "local override config `service`");

        let resolver = TemplateResolver::new(None, None);
        let routes = route_records(vec![
            status_service("route-1", "active", None),
            status_service("route-2", "active", None),
            status_service("route-3", "active", None),
            status_service("route-4", "active", None),
            status_service("route-5", "active", None),
            status_service("route-6", "active", None),
        ]);
        let mut output = test_output_config("traefik");
        output.target = String::from("routes/{{ route.key }}.yml");
        assert!(print_doctor_output(
            &output,
            &resolver,
            &routes,
            &temp_test_dir("doctor-output")
        ));

        let empty_range_config = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: PortRange { start: 1, end: 0 },
            skip_ports: Vec::new(),
        };
        assert!(!print_allocation_diagnostics(
            &empty_range_config,
            &identity.identity,
            None
        ));

        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let held_port = listener.local_addr().expect("listener address").port();
        let allocation_config = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: PortRange {
                start: held_port,
                end: held_port,
            },
            skip_ports: Vec::new(),
        };
        print_previous_port_diagnostics(None, &allocation_config, &[]);
        print_previous_port_diagnostics(Some(held_port.saturating_sub(1)), &allocation_config, &[]);
        print_previous_port_diagnostics(Some(held_port), &allocation_config, &[held_port]);
        let skipped_allocation_config = ResolvedConfig {
            loaded: None,
            fallback_path: None,
            port_range: PortRange {
                start: held_port,
                end: held_port,
            },
            skip_ports: vec![held_port],
        };
        print_previous_port_diagnostics(Some(held_port), &skipped_allocation_config, &[]);
        print_previous_port_diagnostics(Some(held_port), &allocation_config, &[]);

        print_git_diagnostics(Path::new("/definitely/not-a-bindport-git-worktree"));
        print_git_diagnostics(Path::new(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn command_error_conversions_format_underlying_errors() {
        let render_errors = vec![
            RenderCommandError::from(ConfigError::UnknownFormat {
                path: PathBuf::from("bindport.txt"),
            }),
            RenderCommandError::from(OutputConfigError::MissingName { index: 0 }),
            RenderCommandError::InvalidArgument(String::from("bad render arg")),
            RenderCommandError::from(RegistryError::MissingStateDirectory),
            RenderCommandError::from(AdapterTemplateError::InvalidName(String::from("../bad"))),
            RenderCommandError::from(RenderError::TargetCollision {
                target: String::from("routes/demo.yml"),
                route_keys: vec![String::from("a"), String::from("b")],
            }),
            RenderCommandError::from(OutputFileError::UnsafeRoot {
                root: String::from("../out"),
            }),
        ];

        for error in render_errors {
            assert!(!error.to_string().is_empty());
        }

        let template_config_error = TemplateCommandError::from(ConfigError::UnknownFormat {
            path: PathBuf::from("bindport.txt"),
        });
        assert!(matches!(
            template_config_error,
            TemplateCommandError::Config(_)
        ));
        let template_error =
            TemplateCommandError::from(AdapterTemplateError::InvalidName(String::from("../bad")));
        assert!(matches!(template_error, TemplateCommandError::Template(_)));
        let template_invalid = TemplateCommandError::InvalidArgument(String::from("bad template"));
        assert!(matches!(
            template_invalid,
            TemplateCommandError::InvalidArgument(_)
        ));

        let clean_registry_error = CleanCommandError::from(RegistryError::MissingStateDirectory);
        assert!(matches!(
            clean_registry_error,
            CleanCommandError::Registry(_)
        ));
        let json_error = serde_json::from_str::<serde_json::Value>("{").expect_err("json error");
        let clean_json_error = CleanCommandError::from(json_error);
        assert!(matches!(clean_json_error, CleanCommandError::Json(_)));
    }

    #[test]
    fn render_route_helpers_preserve_status_and_pending_metadata() {
        let services = vec![
            status_service("route-1", "active", Some("2026-06-29T00:05:00Z")),
            status_service("route-2", "stopped", None),
            status_service("route-3", "stale", None),
        ];

        let routes = route_records(services);

        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].key, "route-1");
        assert_eq!(routes[0].updated_at, "2026-06-29T00:05:00Z");
        assert_eq!(routes[1].state, "stopped");
        assert_eq!(routes[1].updated_at, "2026-06-29T00:00:00Z");
        assert_eq!(routes[2].state, "stale");
        assert_eq!(
            route_delete_state(&routes[0]),
            None,
            "active routes are not lifecycle delete candidates"
        );
        assert_eq!(
            route_delete_state(&routes[1]),
            Some(OutputDeleteState::Stopped)
        );
        assert_eq!(
            route_delete_state(&routes[2]),
            Some(OutputDeleteState::Stale)
        );

        let mut output = test_output_config("debug");
        output.delete_on = vec![OutputDeleteState::Stopped];
        assert_eq!(
            delete_route_keys(&output, &routes),
            BTreeSet::from([String::from("route-2")])
        );
        output.delete_on = vec![OutputDeleteState::Stopped, OutputDeleteState::Stale];
        assert_eq!(
            delete_route_keys(&output, &routes),
            BTreeSet::from([String::from("route-2"), String::from("route-3")])
        );

        let identity = ServiceIdentity {
            project: String::from("demo"),
            service: String::from("web"),
            git: Some(bindport_core::GitIdentity {
                worktree_path: PathBuf::from("/workspace/demo-feature-tree"),
                worktree_hash: String::from("abc123456789"),
                git_common_dir: PathBuf::from("/workspace/demo/.git"),
                branch: String::from("feature/tree"),
                branch_label: String::from("feature-tree"),
                commit: String::from("0123456789abcdef"),
            }),
            identity_key: String::from("v1:demo:web"),
        };
        let metadata = RunMetadata {
            command: None,
            hostname: Some(String::from("feature-tree.demo.localhost")),
            route_url: Some(String::from("https://feature-tree.demo.localhost")),
            health_url: Some(String::from("https://feature-tree.demo.localhost/health")),
            env: Vec::new(),
        };
        let pending = pending_route_record(
            &identity,
            29_100,
            &metadata,
            "next dev",
            Path::new("/workspace/demo"),
        );

        assert_eq!(pending.key, "v1:demo:web");
        assert_eq!(pending.state, "active");
        assert_eq!(pending.url, "http://127.0.0.1:29100");
        assert_eq!(
            pending.hostname.as_deref(),
            Some("feature-tree.demo.localhost")
        );
        assert_eq!(pending.branch.as_deref(), Some("feature/tree"));
        assert_eq!(pending.branch_label.as_deref(), Some("feature-tree"));
        assert_eq!(
            pending.worktree_path.as_deref(),
            Some("/workspace/demo-feature-tree")
        );
        assert_eq!(pending.pid, None);
        assert_eq!(pending.started_at, "pending");

        let cwd = Path::new("/workspace/demo/apps/web");
        let config_path = PathBuf::from("/workspace/demo/bindport.toml");
        let project_config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: config_path.clone(),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Project,
                local_override: None,
                config: BindPortConfig::default(),
                unknown_keys: Vec::new(),
            }),
            fallback_path: None,
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };
        assert_eq!(
            output_base_dir(cwd, &project_config),
            Path::new("/workspace/demo")
        );

        let fallback_config = ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: PathBuf::from("/home/user/.config/bindport/config.toml"),
                format: bindport_core::ConfigFormat::Toml,
                source: ConfigSource::Fallback,
                local_override: None,
                config: BindPortConfig::default(),
                unknown_keys: Vec::new(),
            }),
            fallback_path: Some(PathBuf::from("/home/user/.config/bindport/config.toml")),
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        };
        assert_eq!(output_base_dir(cwd, &fallback_config), cwd);
    }

    #[cfg(unix)]
    #[test]
    fn exit_status_helpers_preserve_process_codes_and_retry_conditions() {
        let success = ExitStatus::from_raw(0);
        let failure = ExitStatus::from_raw(7 << 8);

        assert_eq!(status_registry_exit_code(&success), Some(0));
        assert_eq!(status_to_exit_code(&success), ExitCode::SUCCESS);
        assert_eq!(status_registry_exit_code(&failure), Some(7));
        assert_eq!(status_to_exit_code(&failure), ExitCode::from(7));

        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let port = listener.local_addr().expect("listener address").port();
        assert!(should_retry_allocation(
            &ExitStatus::from_raw(1 << 8),
            Duration::from_millis(1),
            port
        ));
        assert!(!should_retry_allocation(
            &ExitStatus::from_raw(0),
            Duration::from_millis(1),
            port
        ));
        assert!(!should_retry_allocation(
            &ExitStatus::from_raw(1 << 8),
            ALLOCATION_RETRY_WINDOW + Duration::from_millis(1),
            port
        ));
    }

    fn hook_command(name: &str) -> HookCommandConfig {
        HookCommandConfig {
            name: Some(name.to_string()),
            events: Some(vec![HookEvent::RouteStarted]),
            command: Some(vec![String::from("true")]),
            ..HookCommandConfig::default()
        }
    }

    fn hook_resolved_config(
        source: ConfigSource,
        hooks: HooksConfig,
        local_hooks: Option<HooksConfig>,
    ) -> ResolvedConfig {
        let mut config = BindPortConfig {
            hooks: Some(hooks),
            ..BindPortConfig::default()
        };
        let local_override = local_hooks.map(|hooks| {
            let local_config = BindPortConfig {
                hooks: Some(hooks),
                ..BindPortConfig::default()
            };
            config.merge_local_override(local_config.clone());

            bindport_core::LoadedLocalConfig {
                path: PathBuf::from("/workspace/demo/.bindport.local.toml"),
                format: bindport_core::ConfigFormat::Toml,
                git_tracked: false,
                config: local_config,
                unknown_keys: Vec::new(),
            }
        });

        ResolvedConfig {
            loaded: Some(bindport_core::LoadedConfig {
                path: match source {
                    ConfigSource::Project => PathBuf::from("/workspace/demo/bindport.toml"),
                    ConfigSource::Fallback => {
                        PathBuf::from("/home/user/.config/bindport/config.toml")
                    }
                },
                format: bindport_core::ConfigFormat::Toml,
                source,
                local_override,
                config,
                unknown_keys: Vec::new(),
            }),
            fallback_path: None,
            port_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
        }
    }

    fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
        values.into_iter().map(String::from).collect()
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bindport-cli-{name}-{}-{now}", std::process::id()));
        fs::create_dir_all(&path).expect("temp test dir");
        path
    }

    fn test_output_config(name: &str) -> EffectiveOutputConfig {
        EffectiveOutputConfig {
            name: name.to_string(),
            template: String::from("bindport-traefik"),
            root: None,
            target: String::from("{{ route.slug }}.yml"),
            target_host: String::from("127.0.0.1"),
            target_scheme: String::from("http"),
            auto_render: true,
            delete_on: Vec::new(),
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 0,
            vars: BTreeMap::new(),
        }
    }

    fn status_service(identity_key: &str, state: &str, exited_at: Option<&str>) -> StatusService {
        StatusService {
            project: String::from("demo"),
            service: String::from("web"),
            state: state.to_string(),
            port: 29_100,
            host: String::from("127.0.0.1"),
            url: String::from("http://127.0.0.1:29100"),
            hostname: Some(String::from("feature-tree.demo.localhost")),
            route_url: Some(String::from("https://feature-tree.demo.localhost")),
            health_url: Some(String::from("https://feature-tree.demo.localhost/health")),
            worktree_path: Some(String::from("/workspace/demo-feature-tree")),
            worktree_hash: Some(String::from("abc123456789")),
            git_common_dir: Some(String::from("/workspace/demo/.git")),
            branch: Some(String::from("feature/tree")),
            branch_label: Some(String::from("feature-tree")),
            commit: Some(String::from("0123456789abcdef")),
            identity_key: Some(identity_key.to_string()),
            pid: Some(12_345),
            command: String::from("next dev"),
            cwd: String::from("/workspace/demo"),
            started_at: String::from("2026-06-29T00:00:00Z"),
            exited_at: exited_at.map(str::to_string),
            exit_code: None,
            health: String::from("unknown"),
            outputs: Vec::new(),
            proxy: None,
        }
    }
}
