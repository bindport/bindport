use super::*;

pub(crate) fn host_allowed(host: Option<&str>, options: &DashboardOptions) -> bool {
    let Some(host) = host.map(str::trim).filter(|host| !host.is_empty()) else {
        return false;
    };
    let (name, port) = match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') => (name, Some(port)),
        _ => (host, None),
    };

    if let Some(port) = port
        && (port.is_empty() || !port.chars().all(|character| character.is_ascii_digit()))
    {
        return false;
    }

    if options.auth.required && options.host.is_unspecified() {
        return true;
    }

    name.eq_ignore_ascii_case("localhost")
        || name == "127.0.0.1"
        || name == options.host.to_string()
        || options
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(name))
}
pub(crate) fn request_authorized(request: &HttpRequest, options: &DashboardOptions) -> bool {
    if !options.auth.required {
        return true;
    }

    let Some(expected) = options.auth.token.as_deref() else {
        return false;
    };
    let Some(actual) = request
        .authorization
        .as_deref()
        .and_then(authorization_bearer_token)
    else {
        return false;
    };

    constant_time_eq(actual.as_bytes(), expected.as_bytes())
}

pub(crate) fn request_dashboard_action(request: &HttpRequest, expected: &str) -> bool {
    request
        .dashboard_action
        .as_deref()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
}

pub(crate) fn authorization_bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.trim().split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
        .filter(|token| !token.is_empty())
}

pub(crate) fn constant_time_eq(actual: &[u8], expected: &[u8]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }

    actual
        .iter()
        .zip(expected)
        .fold(0, |diff, (actual, expected)| diff | (actual ^ expected))
        == 0
}
