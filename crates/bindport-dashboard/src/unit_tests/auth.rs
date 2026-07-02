// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn rejects_unknown_host_header() {
    let options = DashboardOptions::default();
    let response = response_for_request(
        &HttpRequest {
            method: String::from("GET"),
            path: String::from("/api/status"),
            host: Some(String::from("example.com:27080")),
            authorization: None,
            dashboard_action: None,
        },
        &options,
    );
    let text = String::from_utf8(response.into_bytes()).expect("response utf8");

    assert!(text.starts_with("HTTP/1.1 403 Forbidden"));
}

#[test]
fn accepts_configured_allowed_host_header() {
    let options = DashboardOptions {
        allowed_hosts: vec![String::from("devbox.test")],
        ..DashboardOptions::default()
    };

    assert!(host_allowed(Some("devbox.test:27080"), &options));
}

#[test]
fn host_validation_rejects_missing_or_invalid_hosts() {
    let options = DashboardOptions::default();

    assert!(!host_allowed(None, &options));
    assert!(!host_allowed(Some("  "), &options));
    assert!(!host_allowed(Some("localhost:"), &options));
    assert!(!host_allowed(Some("localhost:http"), &options));
    assert!(!host_allowed(Some("example.com:27080"), &options));
    assert!(host_allowed(Some("LOCALHOST:27080"), &options));
    assert!(host_allowed(Some("127.0.0.1"), &options));
}

#[test]
fn accepts_arbitrary_host_for_unspecified_bind_with_auth() {
    let options = DashboardOptions {
        host: Ipv4Addr::UNSPECIFIED,
        auth: DashboardAuth {
            required: true,
            token: Some(String::from("secret")),
        },
        ..DashboardOptions::default()
    };

    assert!(host_allowed(Some("remote.example:27080"), &options));
}

#[test]
fn accepts_host_matching_configured_bind_address() {
    let options = DashboardOptions {
        host: Ipv4Addr::new(10, 0, 2, 15),
        ..DashboardOptions::default()
    };

    assert!(host_allowed(Some("10.0.2.15:27080"), &options));
}

#[test]
fn auth_required_rejects_missing_token() {
    let options = DashboardOptions {
        auth: DashboardAuth {
            required: true,
            token: Some(String::from("secret")),
        },
        ..DashboardOptions::default()
    };
    let response = response_for_request(&test_request("/api/status"), &options);
    let text = String::from_utf8(response.into_bytes()).expect("response utf8");

    assert!(text.starts_with("HTTP/1.1 401 Unauthorized"));
}

#[test]
fn auth_required_accepts_bearer_token() {
    let options = DashboardOptions {
        auth: DashboardAuth {
            required: true,
            token: Some(String::from("secret")),
        },
        ..DashboardOptions::default()
    };
    let mut request = test_request("/api/status");
    request.authorization = Some(String::from("Bearer secret"));

    assert!(request_authorized(&request, &options));
}

#[test]
fn auth_rejects_missing_expected_or_invalid_bearer_tokens() {
    let no_expected = DashboardOptions {
        auth: DashboardAuth {
            required: true,
            token: None,
        },
        ..DashboardOptions::default()
    };
    let with_expected = DashboardOptions {
        auth: DashboardAuth {
            required: true,
            token: Some(String::from("secret")),
        },
        ..DashboardOptions::default()
    };
    let mut request = test_request("/api/status");

    request.authorization = Some(String::from("Bearer secret"));
    assert!(!request_authorized(&request, &no_expected));

    request.authorization = Some(String::from("Basic secret"));
    assert!(!request_authorized(&request, &with_expected));

    request.authorization = Some(String::from("Bearer"));
    assert!(!request_authorized(&request, &with_expected));

    request.authorization = Some(String::from("Bearer wrong"));
    assert!(!request_authorized(&request, &with_expected));

    assert!(constant_time_eq(b"secret", b"secret"));
    assert!(!constant_time_eq(b"secret", b"short"));
    assert!(!constant_time_eq(b"secret", b"secrex"));
}
