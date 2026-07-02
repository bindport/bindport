// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn clean_rejects_missing_dashboard_action_header() {
    let options = DashboardOptions::default();
    let response = response_for_request(
        &HttpRequest {
            method: String::from("POST"),
            path: String::from("/api/clean/stopped"),
            host: Some(String::from("127.0.0.1:27080")),
            authorization: None,
            dashboard_action: None,
        },
        &options,
    );
    let text = String::from_utf8(response.into_bytes()).expect("response utf8");

    assert!(text.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(text.contains(DASHBOARD_ACTION_HEADER));
}

#[test]
fn clean_requires_auth_when_auth_is_enabled() {
    let options = DashboardOptions {
        auth: DashboardAuth {
            required: true,
            token: Some(String::from("secret")),
        },
        ..DashboardOptions::default()
    };
    let response = response_for_request(
        &HttpRequest {
            method: String::from("POST"),
            path: String::from("/api/clean/stopped"),
            host: Some(String::from("127.0.0.1:27080")),
            authorization: None,
            dashboard_action: Some(String::from("clean")),
        },
        &options,
    );
    let text = String::from_utf8(response.into_bytes()).expect("response utf8");

    assert!(text.starts_with("HTTP/1.1 401 Unauthorized"));
}
