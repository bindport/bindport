// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn json_error_body_escapes_message() {
    let body = json_error_body(String::from("registry unavailable: \"bad\"\npath"));
    let value = serde_json::from_str::<serde_json::Value>(&body).expect("json body");

    assert_eq!(value["error"], "registry unavailable: \"bad\"\npath");
}

#[test]
fn response_helpers_format_status_and_body() {
    for (response, expected_status, expected_body) in [
        (
            HttpResponse::bad_request(),
            "HTTP/1.1 400 Bad Request",
            "bad request\n",
        ),
        (
            HttpResponse::request_too_large(),
            "HTTP/1.1 431 Request Header Fields Too Large",
            "request too large\n",
        ),
        (
            HttpResponse::service_unavailable("{\"error\":\"down\"}\n"),
            "HTTP/1.1 503 Service Unavailable",
            "{\"error\":\"down\"}\n",
        ),
    ] {
        let response = String::from_utf8(response.into_bytes()).expect("response utf8");
        assert!(response.starts_with(expected_status));
        assert!(response.contains(expected_body));
    }
}
