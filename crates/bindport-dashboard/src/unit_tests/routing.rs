// SPDX-License-Identifier: MIT

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[test]
fn response_routes_static_assets_health_and_host_rejections() {
    let options = DashboardOptions::default();

    let health = response_for_request(&test_request("/healthz"), &options);
    let health = String::from_utf8(health.into_bytes()).expect("health utf8");
    assert!(health.starts_with("HTTP/1.1 200 OK"));
    assert!(health.contains("Content-Type: text/plain; charset=utf-8"));
    assert!(health.ends_with("ok\n"));

    for (path, content_type) in [
        ("/", "text/html; charset=utf-8"),
        ("/assets/app.css", "text/css; charset=utf-8"),
        ("/assets/app.js", "text/javascript; charset=utf-8"),
    ] {
        let response = response_for_request(&test_request(path), &options);
        let response = String::from_utf8(response.into_bytes()).expect("asset utf8");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{path}");
        assert!(response.contains(content_type), "{path}");
    }

    let mut missing_host = test_request("/healthz");
    missing_host.host = None;
    let response = response_for_request(&missing_host, &options);
    let response = String::from_utf8(response.into_bytes()).expect("forbidden utf8");
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
}

#[test]
fn status_response_uses_registry_and_includes_callback_payload() {
    let registry_path = temp_registry_path("status-callback");
    with_default_registry_path(&registry_path, || {
        let options = DashboardOptions {
            status_callback: Some(Arc::new(|| {
                serde_json::json!({
                    "trusted": 2,
                    "pending": 1,
                })
            })),
            ..DashboardOptions::default()
        };
        let response = response_for_request(&test_request("/api/status"), &options);
        let response = String::from_utf8(response.into_bytes()).expect("status utf8");

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let body = response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("status body");
        let json = serde_json::from_str::<serde_json::Value>(body).expect("status json");
        assert_eq!(json["hooks"]["trusted"], 2);
        assert_eq!(json["hooks"]["pending"], 1);
        assert!(json["services"].as_array().expect("services").is_empty());
    });
}

#[test]
fn status_response_rejects_missing_auth_before_opening_registry() {
    let registry_path = temp_registry_path("status-auth-ordering");
    with_default_registry_path(&registry_path, || {
        let options = DashboardOptions {
            auth: DashboardAuth {
                required: true,
                token: Some(String::from("secret")),
            },
            ..DashboardOptions::default()
        };
        let response = response_for_request(&test_request("/api/status"), &options);
        let response = String::from_utf8(response.into_bytes()).expect("unauthorized utf8");

        assert!(response.starts_with("HTTP/1.1 401 Unauthorized"));
    });
    assert!(!registry_path.exists());
}

#[test]
fn status_and_clean_report_registry_open_errors() {
    let blocked_parent = temp_test_dir("blocked-registry-parent").join("parent-file");
    fs::write(&blocked_parent, "not a directory").expect("blocked parent");
    let registry_path = blocked_parent.join("registry.sqlite");

    with_default_registry_path(&registry_path, || {
        let status =
            response_for_request(&test_request("/api/status"), &DashboardOptions::default());
        let status = String::from_utf8(status.into_bytes()).expect("status utf8");
        assert!(status.starts_with("HTTP/1.1 503 Service Unavailable"));
        assert!(status.contains("registry unavailable"));

        let mut clean = test_request("/api/clean/stopped");
        clean.method = String::from("POST");
        clean.dashboard_action = Some(String::from("clean"));
        let clean = response_for_request(&clean, &DashboardOptions::default());
        let clean = String::from_utf8(clean.into_bytes()).expect("clean utf8");
        assert!(clean.starts_with("HTTP/1.1 503 Service Unavailable"));
        assert!(clean.contains("registry unavailable"));
    });
}

#[test]
fn clean_response_removes_registry_entries_and_runs_callback() {
    let registry_path = temp_registry_path("clean-callback");
    let callback_count = Arc::new(AtomicUsize::new(0));
    with_default_registry_path(&registry_path, || {
        let mut registry = Registry::open(&registry_path).expect("registry");
        let started = registry
            .record_run_started(&bindport_registry::RunStart {
                project: String::from("demo"),
                service: String::from("web"),
                identity: None,
                host: String::from("127.0.0.1"),
                port: 29_100,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/workspace/demo"),
            })
            .expect("record run");
        registry
            .record_run_finished(started, Some(0))
            .expect("finish run");
        drop(registry);

        let callback_count = Arc::clone(&callback_count);
        let options = DashboardOptions {
            clean_callback: Some(Arc::new(move |_, summary| {
                assert_eq!(summary.stopped_leases, 1);
                callback_count.fetch_add(1, Ordering::SeqCst);
                Err(String::from("callback warning"))
            })),
            ..DashboardOptions::default()
        };
        let mut request = test_request("/api/clean/stopped");
        request.method = String::from("POST");
        request.dashboard_action = Some(String::from("clean"));
        let response = response_for_request(&request, &options);
        let response = String::from_utf8(response.into_bytes()).expect("clean utf8");

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let body = response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("clean body");
        let json = serde_json::from_str::<serde_json::Value>(body).expect("clean json");
        assert_eq!(json["leases"], 1);
        assert_eq!(json["runs"], 1);
        assert_eq!(json["states"]["stopped"], 1);
    });

    assert_eq!(callback_count.load(Ordering::SeqCst), 1);
}

#[test]
fn run_clean_callback_skips_empty_summary() {
    let registry_path = temp_registry_path("clean-empty-callback");
    let mut registry = Registry::open(registry_path).expect("registry");
    let callback_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&callback_count);
    let options = DashboardOptions {
        clean_callback: Some(Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
        ..DashboardOptions::default()
    };

    run_clean_callback(&options, &mut registry, CleanSummary::default());

    assert_eq!(callback_count.load(Ordering::SeqCst), 0);
    assert_eq!(
        clean_summary_json(CleanSummary {
            stopped_leases: 2,
            stale_leases: 3,
            runs: 4,
        })["leases"],
        5
    );
}
