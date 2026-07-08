// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn list_option_parser_accepts_json_and_help_only() {
    let options = parse_list_options(&strings(["--json"])).expect("json options");
    assert!(options.json);
    assert!(!options.help);

    let options = parse_list_options(&strings(["--help"])).expect("help options");
    assert!(options.help);

    assert!(parse_list_options(&strings(["--bad"])).is_err());
    assert!(parse_list_options(&strings(["web"])).is_err());
}

#[test]
fn list_snapshot_groups_services_by_project() {
    let mut web = status_service("v1:demo:web", "active", None);
    web.service = String::from("web");
    web.project = String::from("demo");
    web.port = 29_100;

    let mut api = status_service("v1:demo:api", "stopped", Some("2026-06-29T00:01:00Z"));
    api.service = String::from("api");
    api.project = String::from("demo");
    api.port = 29_101;

    let mut admin = status_service("v1:ops:admin", "reserved", None);
    admin.service = String::from("admin");
    admin.project = String::from("ops");
    admin.port = 29_200;
    admin.pid = None;

    let snapshot = list_snapshot(&test_status_snapshot(vec![web, admin, api]));

    assert_eq!(snapshot.schema_version, "0.1");
    assert_eq!(snapshot.project_count, 2);
    assert_eq!(snapshot.service_count, 3);
    assert_eq!(snapshot.projects[0].project, "demo");
    assert_eq!(snapshot.projects[0].service_count, 2);
    assert_eq!(snapshot.projects[0].active, 1);
    assert_eq!(snapshot.projects[0].stopped, 1);
    assert_eq!(snapshot.projects[0].services[0].service, "api");
    assert_eq!(snapshot.projects[0].services[1].service, "web");
    assert_eq!(snapshot.projects[1].project, "ops");
    assert_eq!(snapshot.projects[1].reserved, 1);
}
