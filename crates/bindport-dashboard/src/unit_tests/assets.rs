// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn root_request_serves_dashboard_html() {
    let options = DashboardOptions::default();
    let response = response_for_request(&test_request("/"), &options);
    let bytes = response.into_bytes();
    let text = String::from_utf8(bytes).expect("response utf8");

    assert!(text.starts_with("HTTP/1.1 200 OK"));
    assert!(text.contains("BindPort Dashboard"));
    assert!(text.contains("/assets/app.css"));
    assert!(text.contains("/assets/app.js"));
    assert!(text.contains("service-search"));
    assert!(text.contains("data-state-filter=\"active\""));
    assert!(text.contains("auth-token"));
    assert!(text.contains("action-status"));
    assert!(text.contains("app-footer"));
    assert!(text.contains("data-state-filter=\"conflict\""));
    assert!(text.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))));
    assert!(!text.contains("{{APP_VERSION}}"));
}

#[test]
fn asset_routes_serve_embedded_dashboard_files() {
    let options = DashboardOptions::default();
    let css = response_for_request(&test_request("/assets/app.css"), &options);
    let css = String::from_utf8(css.into_bytes()).expect("css utf8");
    let js = response_for_request(&test_request("/assets/app.js"), &options);
    let js = String::from_utf8(js.into_bytes()).expect("js utf8");

    assert!(css.starts_with("HTTP/1.1 200 OK"));
    assert!(css.contains("text/css"));
    assert!(css.contains(".state-active"));
    assert!(css.contains(".state-conflict"));
    assert!(js.starts_with("HTTP/1.1 200 OK"));
    assert!(js.contains("text/javascript"));
    assert!(js.contains("REFRESH_INTERVAL_MS = 5000"));
    assert!(js.contains("refreshStatus"));
    assert!(js.contains("/api/clean/"));
    assert!(js.contains("data-clean-state"));
    assert!(js.contains("{ key: \"conflict\", label: \"Conflict\" }"));
    assert!(js.contains("<dt>Port</dt>"));
    assert!(js.contains("<dt>Health</dt>"));
    assert!(js.contains("<dt>Proxy</dt>"));
    assert!(js.contains("function proxyStatus(service)"));
    assert!(js.contains("Not rendered"));
    assert!(js.contains("No services match the current filters."));
}

#[test]
fn static_dir_serves_assets_and_injects_dev_reload() {
    let static_dir = temp_test_dir("dashboard-static");
    fs::write(
        static_dir.join("index.html"),
        "<html><body>{{APP_NAME}} {{APP_VERSION}}</body></html>",
    )
    .expect("write index");
    fs::write(static_dir.join("app.css"), ".custom { color: red; }").expect("write css");
    fs::write(static_dir.join("app.js"), "console.log('custom');").expect("write js");
    fs::write(static_dir.join("dev-reload.js"), "console.log('reload');")
        .expect("write dev reload");
    let options = DashboardOptions {
        static_dir: Some(static_dir.clone()),
        ..DashboardOptions::default()
    };

    let index = response_for_request(&test_request("/"), &options);
    let index = String::from_utf8(index.into_bytes()).expect("index utf8");
    assert!(index.starts_with("HTTP/1.1 200 OK"));
    assert!(index.contains("BindPort"));
    assert!(index.contains(env!("CARGO_PKG_VERSION")));

    #[cfg(debug_assertions)]
    assert!(index.contains("/assets/dev-reload.js"));

    let css = response_for_request(&test_request("/assets/app.css"), &options);
    let css = String::from_utf8(css.into_bytes()).expect("css utf8");
    assert!(css.contains(".custom { color: red; }"));

    let js = response_for_request(&test_request("/assets/app.js"), &options);
    let js = String::from_utf8(js.into_bytes()).expect("js utf8");
    assert!(js.contains("console.log('custom');"));

    #[cfg(debug_assertions)]
    {
        let dev_reload = response_for_request(&test_request("/assets/dev-reload.js"), &options);
        let dev_reload = String::from_utf8(dev_reload.into_bytes()).expect("reload utf8");
        assert!(dev_reload.contains("console.log('reload');"));

        let dev_version = response_for_request(&test_request("/assets/dev-version"), &options);
        let dev_version = String::from_utf8(dev_version.into_bytes()).expect("version utf8");
        assert!(dev_version.starts_with("HTTP/1.1 200 OK"));
        assert!(dev_version.contains('.'));
    }
}

#[test]
fn static_dir_reports_asset_errors() {
    let static_dir = temp_test_dir("dashboard-static-missing");
    let options = DashboardOptions {
        static_dir: Some(static_dir.clone()),
        ..DashboardOptions::default()
    };

    let missing_asset = response_for_request(&test_request("/assets/app.css"), &options);
    let missing_asset = String::from_utf8(missing_asset.into_bytes()).expect("asset utf8");
    assert!(missing_asset.starts_with("HTTP/1.1 500 Internal Server Error"));
    assert!(missing_asset.contains("failed to read dashboard asset"));

    fs::write(static_dir.join("index.html"), "<html>{{APP_NAME}}</html>")
        .expect("write index without body");
    let missing_body = response_for_request(&test_request("/"), &options);
    let missing_body = String::from_utf8(missing_body.into_bytes()).expect("index utf8");
    assert!(missing_body.starts_with("HTTP/1.1 500 Internal Server Error"));

    #[cfg(debug_assertions)]
    assert!(missing_body.contains("dashboard HTML is missing </body>"));
}
