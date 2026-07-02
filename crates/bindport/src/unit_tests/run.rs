// SPDX-License-Identifier: MIT

use super::*;

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
