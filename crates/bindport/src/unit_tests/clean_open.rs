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
