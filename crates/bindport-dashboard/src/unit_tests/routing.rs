// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn unknown_route_returns_404() {
    let options = DashboardOptions::default();
    let response = response_for_request(&test_request("/missing"), &options);
    let text = String::from_utf8(response.into_bytes()).expect("response utf8");

    assert!(text.starts_with("HTTP/1.1 404 Not Found"));
}

#[test]
fn route_parser_maps_dashboard_endpoints() {
    assert!(matches!(
        request_route(&test_request("/")),
        Some(Route::Index)
    ));
    assert!(matches!(
        request_route(&test_request("/assets/app.css")),
        Some(Route::Css)
    ));
    assert!(matches!(
        request_route(&test_request("/assets/app.js")),
        Some(Route::Js)
    ));
    assert!(matches!(
        request_route(&test_request("/api/status")),
        Some(Route::Status)
    ));
    assert!(matches!(
        request_route(&test_request("/healthz")),
        Some(Route::Health)
    ));
    assert!(request_route(&test_request("/missing")).is_none());

    for path in ["/api/clean", "/api/clean/all"] {
        let mut request = test_request(path);
        request.method = String::from("POST");
        assert!(matches!(
            request_route(&request),
            Some(Route::Clean(states))
                if matches!(states.as_slice(), [CleanState::Stopped, CleanState::Stale])
        ));
    }

    let mut stopped = test_request("/api/clean/stopped");
    stopped.method = String::from("POST");
    assert!(matches!(
        request_route(&stopped),
        Some(Route::Clean(states)) if matches!(states.as_slice(), [CleanState::Stopped])
    ));

    let mut stale = test_request("/api/clean/stale");
    stale.method = String::from("POST");
    assert!(matches!(
        request_route(&stale),
        Some(Route::Clean(states)) if matches!(states.as_slice(), [CleanState::Stale])
    ));
}
