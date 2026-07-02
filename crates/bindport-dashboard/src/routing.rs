use super::*;

pub(crate) fn response_for_request(
    request: &HttpRequest,
    options: &DashboardOptions,
) -> HttpResponse {
    if !host_allowed(request.host.as_deref(), options) {
        return HttpResponse::forbidden();
    }

    match request_route(request) {
        Some(Route::Index) => dashboard_index_response(options),
        Some(Route::Css) => {
            static_asset_response("app.css", APP_CSS, "text/css; charset=utf-8", options)
        }
        Some(Route::Js) => {
            static_asset_response("app.js", APP_JS, "text/javascript; charset=utf-8", options)
        }
        #[cfg(debug_assertions)]
        Some(Route::DevReload) => static_asset_response(
            "dev-reload.js",
            DEV_RELOAD_JS,
            "text/javascript; charset=utf-8",
            options,
        ),
        #[cfg(debug_assertions)]
        Some(Route::DevVersion) => dev_version_response(options),
        Some(Route::Status) if request_authorized(request, options) => status_response(options),
        Some(Route::Status) => HttpResponse::unauthorized(),
        Some(Route::Clean(states)) => clean_response(request, options, &states),
        Some(Route::Health) => HttpResponse::ok("text/plain; charset=utf-8", "ok\n"),
        _ => HttpResponse::not_found(),
    }
}

pub(crate) enum Route {
    Index,
    Css,
    Js,
    #[cfg(debug_assertions)]
    DevReload,
    #[cfg(debug_assertions)]
    DevVersion,
    Status,
    Clean(Vec<CleanState>),
    Health,
}

pub(crate) fn request_route(request: &HttpRequest) -> Option<Route> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => Some(Route::Index),
        ("GET", "/assets/app.css") => Some(Route::Css),
        ("GET", "/assets/app.js") => Some(Route::Js),
        #[cfg(debug_assertions)]
        ("GET", "/assets/dev-reload.js") => Some(Route::DevReload),
        #[cfg(debug_assertions)]
        ("GET", "/assets/dev-version") => Some(Route::DevVersion),
        ("GET", "/api/status") => Some(Route::Status),
        ("POST", "/api/clean" | "/api/clean/all") => {
            Some(Route::Clean(vec![CleanState::Stopped, CleanState::Stale]))
        }
        ("POST", "/api/clean/stopped") => Some(Route::Clean(vec![CleanState::Stopped])),
        ("POST", "/api/clean/stale") => Some(Route::Clean(vec![CleanState::Stale])),
        ("GET", "/healthz") => Some(Route::Health),
        _ => None,
    }
}
pub(crate) fn status_response(options: &DashboardOptions) -> HttpResponse {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => match serde_json::to_value(&snapshot).and_then(|mut value| {
            if let Some(callback) = options.status_callback.as_ref()
                && let Some(object) = value.as_object_mut()
            {
                object.insert(String::from("hooks"), callback());
            }
            serde_json::to_string_pretty(&value)
        }) {
            Ok(json) => HttpResponse::ok("application/json; charset=utf-8", &json),
            Err(error) => HttpResponse::internal_error(&json_error_body(format!(
                "failed to serialize status JSON: {error}"
            ))),
        },
        Err(error) => HttpResponse::service_unavailable(&json_error_body(format!(
            "registry unavailable: {error}"
        ))),
    }
}

pub(crate) fn clean_response(
    request: &HttpRequest,
    options: &DashboardOptions,
    states: &[CleanState],
) -> HttpResponse {
    if !request_authorized(request, options) {
        return HttpResponse::unauthorized();
    }
    if !request_dashboard_action(request, "clean") {
        return HttpResponse::bad_json_request(&json_error_body(format!(
            "{DASHBOARD_ACTION_HEADER}: clean is required"
        )));
    }

    match Registry::open_default() {
        Ok(mut registry) => match registry.clean_leases(states, false) {
            Ok(summary) => {
                run_clean_callback(options, &mut registry, summary);

                match serde_json::to_string_pretty(&clean_summary_json(summary)) {
                    Ok(json) => HttpResponse::ok("application/json; charset=utf-8", &json),
                    Err(error) => HttpResponse::internal_error(&json_error_body(format!(
                        "failed to serialize clean JSON: {error}"
                    ))),
                }
            }
            Err(error) => HttpResponse::service_unavailable(&json_error_body(format!(
                "registry unavailable: {error}"
            ))),
        },
        Err(error) => HttpResponse::service_unavailable(&json_error_body(format!(
            "registry unavailable: {error}"
        ))),
    }
}

pub(crate) fn run_clean_callback(
    options: &DashboardOptions,
    registry: &mut Registry,
    summary: CleanSummary,
) {
    if summary.total_leases() == 0 {
        return;
    }

    if let Some(callback) = &options.clean_callback
        && let Err(error) = callback(registry, summary)
    {
        eprintln!("dashboard: warning: cleanup callback failed: {error}");
    }
}

pub(crate) fn clean_summary_json(summary: CleanSummary) -> serde_json::Value {
    serde_json::json!({
        "leases": summary.total_leases(),
        "runs": summary.runs,
        "states": {
            "stopped": summary.stopped_leases,
            "stale": summary.stale_leases,
        },
    })
}
