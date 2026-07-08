// SPDX-License-Identifier: MIT

use super::*;

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
fn open_command_errors_preserve_error_variants() {
    assert!(matches!(
        OpenCommandError::from(RegistryError::MissingStateDirectory),
        OpenCommandError::Registry(_)
    ));
    assert!(matches!(
        OpenCommandError::from(io::Error::other("browser")),
        OpenCommandError::Browser(_)
    ));
}

#[test]
fn open_command_result_handles_help_registry_errors_and_success() {
    assert!(run_open_command_result(&strings(["--help"])).is_ok());

    let blocked_parent = temp_test_dir("open-registry-blocked").join("parent-file");
    fs::write(&blocked_parent, "not a directory").expect("blocked parent");
    let registry_path = blocked_parent.join("registry.sqlite");
    with_default_registry_path(&registry_path, || {
        assert!(matches!(
            run_open_command_result(&strings(["web", "--print"])),
            Err(OpenCommandError::Registry(_))
        ));
    });

    let registry_path = temp_registry_path("open-success");
    let state_home = temp_test_dir("open-success-state");
    let config_home = temp_test_dir("open-success-config");
    let registry_value = registry_path.to_string_lossy().to_string();
    let state_value = state_home.to_string_lossy().to_string();
    let config_value = config_home.to_string_lossy().to_string();
    let mut registry = Registry::open(&registry_path).expect("registry");
    registry
        .record_run_started(&RunStart {
            project: String::from("demo"),
            service: String::from("web"),
            identity: None,
            host: String::from("127.0.0.1"),
            port: 29_100,
            hostname: Some(String::from("feature.demo.localhost")),
            route_url: Some(String::from("https://feature.demo.localhost")),
            health_url: None,
            pid: std::process::id(),
            command: current_process_command(),
            cwd: PathBuf::from("/workspace/demo"),
        })
        .expect("record service");
    drop(registry);

    with_env_values(
        &[
            (REGISTRY_PATH_ENV, Some(registry_value.as_str())),
            ("XDG_STATE_HOME", Some(state_value.as_str())),
            ("XDG_CONFIG_HOME", Some(config_value.as_str())),
        ],
        || {
            assert!(run_open_command_result(&strings(["web", "--print"])).is_ok());
            assert_eq!(print_status(), ExitCode::SUCCESS);
            assert_eq!(print_status_json(), ExitCode::SUCCESS);
        },
    );
}

#[test]
fn status_and_clean_commands_handle_empty_registry_and_help() {
    let registry_path = temp_registry_path("status-clean-empty");
    let state_home = temp_test_dir("status-clean-empty-state");
    let config_home = temp_test_dir("status-clean-empty-config");
    let registry_value = registry_path.to_string_lossy().to_string();
    let state_value = state_home.to_string_lossy().to_string();
    let config_value = config_home.to_string_lossy().to_string();

    with_env_values(
        &[
            (REGISTRY_PATH_ENV, Some(registry_value.as_str())),
            ("XDG_STATE_HOME", Some(state_value.as_str())),
            ("XDG_CONFIG_HOME", Some(config_value.as_str())),
        ],
        || {
            assert_eq!(print_status(), ExitCode::SUCCESS);
            assert_eq!(print_status_json(), ExitCode::SUCCESS);
            assert!(run_list_command_result(&strings(["--json"])).is_ok());
            assert!(clean_registry_result(&strings(["--help"])).is_ok());
            assert!(clean_registry_result(&strings(["--dry-run", "--json"])).is_ok());
        },
    );
}
