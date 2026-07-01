// SPDX-License-Identifier: MIT

use std::{
    borrow::Cow,
    fmt, fs,
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use bindport_core::{DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, PortRange};
use bindport_registry::{CleanState, CleanSummary, Registry};

pub const DEFAULT_DASHBOARD_PORT: u16 = 27_080;
const DASHBOARD_APP_NAME: &str = "BindPort";
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const DASHBOARD_ACTION_HEADER: &str = "X-BindPort-Dashboard-Action";

pub type DashboardCleanCallback =
    Arc<dyn Fn(&mut Registry, CleanSummary) -> Result<(), String> + Send + Sync + 'static>;
pub type DashboardStatusCallback = Arc<dyn Fn() -> serde_json::Value + Send + Sync + 'static>;

#[derive(Clone)]
pub struct DashboardOptions {
    pub host: Ipv4Addr,
    pub preferred_port: u16,
    pub fallback_range: PortRange,
    pub skip_ports: Vec<u16>,
    pub allowed_hosts: Vec<String>,
    pub auth: DashboardAuth,
    pub static_dir: Option<PathBuf>,
    pub clean_callback: Option<DashboardCleanCallback>,
    pub status_callback: Option<DashboardStatusCallback>,
}

impl fmt::Debug for DashboardOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DashboardOptions")
            .field("host", &self.host)
            .field("preferred_port", &self.preferred_port)
            .field("fallback_range", &self.fallback_range)
            .field("skip_ports", &self.skip_ports)
            .field("allowed_hosts", &self.allowed_hosts)
            .field("auth", &self.auth)
            .field("static_dir", &self.static_dir)
            .field("clean_callback", &self.clean_callback.is_some())
            .field("status_callback", &self.status_callback.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DashboardAuth {
    pub required: bool,
    pub token: Option<String>,
}

impl Default for DashboardOptions {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::LOCALHOST,
            preferred_port: DEFAULT_DASHBOARD_PORT,
            fallback_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
            allowed_hosts: default_allowed_hosts(),
            auth: DashboardAuth::default(),
            static_dir: None,
            clean_callback: None,
            status_callback: None,
        }
    }
}

#[derive(Debug)]
pub enum DashboardError {
    NoAvailablePort { range: PortRange },
    Bind { port: u16, source: io::Error },
    LocalAddress(io::Error),
}

impl fmt::Display for DashboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAvailablePort { range } => write!(
                f,
                "no dashboard port available in range {}-{}",
                range.start, range.end
            ),
            Self::Bind { port, source } => {
                write!(f, "failed to bind dashboard port {port}: {source}")
            }
            Self::LocalAddress(source) => write!(f, "failed to read dashboard address: {source}"),
        }
    }
}

impl std::error::Error for DashboardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bind { source, .. } | Self::LocalAddress(source) => Some(source),
            Self::NoAvailablePort { .. } => None,
        }
    }
}

pub struct DashboardServer {
    listener: TcpListener,
    options: DashboardOptions,
    port: u16,
}

impl DashboardServer {
    pub fn bind(options: DashboardOptions) -> Result<Self, DashboardError> {
        let listener = bind_dashboard_listener(&options)?;
        let port = listener
            .local_addr()
            .map_err(DashboardError::LocalAddress)?
            .port();

        Ok(Self {
            listener,
            options,
            port,
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn url(&self) -> String {
        format!("http://{}:{}", self.options.host, self.port)
    }

    pub fn serve(self) -> Result<(), DashboardError> {
        let options = self.options;
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let options = options.clone();
                    thread::spawn(move || {
                        if let Err(error) = handle_connection(stream, &options)
                            && !is_routine_client_error(&error)
                        {
                            eprintln!("dashboard: request error: {error}");
                        }
                    });
                }
                Err(error) => {
                    eprintln!("dashboard: accept error: {error}");
                }
            }
        }

        Ok(())
    }
}

fn bind_dashboard_listener(options: &DashboardOptions) -> Result<TcpListener, DashboardError> {
    match TcpListener::bind(SocketAddrV4::new(options.host, options.preferred_port)) {
        Ok(listener) => return Ok(listener),
        Err(error) if error.kind() != io::ErrorKind::AddrInUse => {
            return Err(DashboardError::Bind {
                port: options.preferred_port,
                source: error,
            });
        }
        Err(_) => {}
    }

    for port in fallback_ports(options) {
        match TcpListener::bind(SocketAddrV4::new(options.host, port)) {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => continue,
            Err(error) => {
                return Err(DashboardError::Bind {
                    port,
                    source: error,
                });
            }
        }
    }

    Err(DashboardError::NoAvailablePort {
        range: options.fallback_range,
    })
}

fn default_allowed_hosts() -> Vec<String> {
    vec![String::from("localhost"), Ipv4Addr::LOCALHOST.to_string()]
}

fn fallback_ports(options: &DashboardOptions) -> impl Iterator<Item = u16> + '_ {
    let range = options.fallback_range;
    (0..range.len()).filter_map(move |offset| {
        let port = range.start as u32 + offset;
        let port = u16::try_from(port).ok()?;

        (!options.skip_ports.contains(&port)).then_some(port)
    })
}

fn handle_connection(mut stream: TcpStream, options: &DashboardOptions) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let request = match read_request(&stream) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) if is_routine_client_error(&error) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            let response = if error.to_string().contains("too large") {
                HttpResponse::request_too_large()
            } else {
                HttpResponse::bad_request()
            };
            write_response(&mut stream, response)?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let response = response_for_request(&request, options);

    write_response(&mut stream, response)
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> io::Result<()> {
    stream.write_all(&response.into_bytes())?;
    stream.flush()
}

fn is_routine_client_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::WouldBlock
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    path: String,
    host: Option<String>,
    authorization: Option<String>,
    dashboard_action: Option<String>,
}

fn read_request(stream: &TcpStream) -> io::Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream);
    let request_line = read_limited_line(&mut reader, MAX_REQUEST_LINE_BYTES)?;
    if request_line.is_empty() {
        return Ok(None);
    }

    let mut host = None;
    let mut authorization = None;
    let mut dashboard_action = None;
    let mut header_bytes = 0;
    loop {
        let header = read_limited_line(&mut reader, MAX_HEADER_LINE_BYTES)?;
        if header.is_empty() || header == "\r\n" || header == "\n" {
            break;
        }
        header_bytes += header.len();
        if header_bytes > MAX_HEADER_BYTES {
            return Err(request_too_large_error());
        }

        if let Some((name, value)) = header.trim_end().split_once(':')
            && name.eq_ignore_ascii_case("host")
            && host.is_none()
        {
            host = Some(value.trim().to_string());
        }
        if let Some((name, value)) = header.trim_end().split_once(':')
            && name.eq_ignore_ascii_case("authorization")
            && authorization.is_none()
        {
            authorization = Some(value.trim().to_string());
        }
        if let Some((name, value)) = header.trim_end().split_once(':')
            && name.eq_ignore_ascii_case(DASHBOARD_ACTION_HEADER)
            && dashboard_action.is_none()
        {
            dashboard_action = Some(value.trim().to_string());
        }
    }

    let mut parts = request_line.split_whitespace();
    let Some(method) = parts.next() else {
        return Err(invalid_request_error());
    };
    let Some(path) = parts.next() else {
        return Err(invalid_request_error());
    };

    Ok(Some(HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        host,
        authorization,
        dashboard_action,
    }))
}

fn read_limited_line(reader: &mut impl BufRead, limit: usize) -> io::Result<String> {
    let mut bytes = Vec::new();

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);

        if bytes.len() + length > limit {
            return Err(request_too_large_error());
        }

        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);

        if bytes.last() == Some(&b'\n') {
            break;
        }
    }

    String::from_utf8(bytes).map_err(|_| invalid_request_error())
}

fn request_too_large_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "dashboard request too large")
}

fn invalid_request_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid dashboard request")
}

fn response_for_request(request: &HttpRequest, options: &DashboardOptions) -> HttpResponse {
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

enum Route {
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

fn request_route(request: &HttpRequest) -> Option<Route> {
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

fn host_allowed(host: Option<&str>, options: &DashboardOptions) -> bool {
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

fn status_response(options: &DashboardOptions) -> HttpResponse {
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

fn clean_response(
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

fn run_clean_callback(options: &DashboardOptions, registry: &mut Registry, summary: CleanSummary) {
    if summary.total_leases() == 0 {
        return;
    }

    if let Some(callback) = &options.clean_callback
        && let Err(error) = callback(registry, summary)
    {
        eprintln!("dashboard: warning: cleanup callback failed: {error}");
    }
}

fn clean_summary_json(summary: CleanSummary) -> serde_json::Value {
    serde_json::json!({
        "leases": summary.total_leases(),
        "runs": summary.runs,
        "states": {
            "stopped": summary.stopped_leases,
            "stale": summary.stale_leases,
        },
    })
}

fn json_error_body(message: String) -> String {
    format!("{}\n", serde_json::json!({ "error": message }))
}

fn request_authorized(request: &HttpRequest, options: &DashboardOptions) -> bool {
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

fn request_dashboard_action(request: &HttpRequest, expected: &str) -> bool {
    request
        .dashboard_action
        .as_deref()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
}

fn authorization_bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.trim().split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
        .filter(|token| !token.is_empty())
}

fn constant_time_eq(actual: &[u8], expected: &[u8]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }

    actual
        .iter()
        .zip(expected)
        .fold(0, |diff, (actual, expected)| diff | (actual ^ expected))
        == 0
}

fn dashboard_index_response(options: &DashboardOptions) -> HttpResponse {
    let body = match static_file(options.static_dir.as_deref(), "index.html", INDEX_HTML) {
        Ok(page) => {
            maybe_inject_dev_reload(inject_app_metadata(page), options.static_dir.as_deref())
        }
        Err(message) => Err(message),
    };
    static_response(body, "text/html; charset=utf-8")
}

fn inject_app_metadata(page: Cow<'static, str>) -> Cow<'static, str> {
    Cow::Owned(
        page.replace("{{APP_NAME}}", DASHBOARD_APP_NAME)
            .replace("{{APP_VERSION}}", env!("CARGO_PKG_VERSION")),
    )
}

fn static_asset_response(
    filename: &'static str,
    embedded: &'static str,
    content_type: &'static str,
    options: &DashboardOptions,
) -> HttpResponse {
    static_response(
        static_file(options.static_dir.as_deref(), filename, embedded),
        content_type,
    )
}

#[cfg(debug_assertions)]
fn dev_version_response(options: &DashboardOptions) -> HttpResponse {
    static_response(
        dev_static_version(options.static_dir.as_deref()),
        "text/plain; charset=utf-8",
    )
}

fn static_response(
    body: Result<Cow<'static, str>, &'static str>,
    content_type: &'static str,
) -> HttpResponse {
    match body {
        Ok(body) => HttpResponse::ok(content_type, &body),
        Err(message) => HttpResponse::internal_error(&json_error_body(message.to_string())),
    }
}

fn static_file(
    static_dir: Option<&Path>,
    filename: &'static str,
    embedded: &'static str,
) -> Result<Cow<'static, str>, &'static str> {
    if let Some(static_dir) = static_dir {
        return fs::read_to_string(static_dir.join(filename))
            .map(Cow::Owned)
            .map_err(|_| "failed to read dashboard asset");
    }

    Ok(Cow::Borrowed(embedded))
}

fn maybe_inject_dev_reload(
    page: Cow<'static, str>,
    static_dir: Option<&Path>,
) -> Result<Cow<'static, str>, &'static str> {
    #[cfg(debug_assertions)]
    {
        if static_dir.is_none() {
            return Ok(page);
        }

        let page = page.into_owned();
        let Some(index) = page.rfind("</body>") else {
            return Err("dashboard HTML is missing </body>");
        };
        let tag = r#"  <script src="/assets/dev-reload.js"></script>
"#;
        let mut output = String::with_capacity(page.len() + tag.len());
        output.push_str(&page[..index]);
        output.push_str(tag);
        output.push_str(&page[index..]);
        Ok(Cow::Owned(output))
    }

    #[cfg(not(debug_assertions))]
    {
        let _ = static_dir;
        Ok(page)
    }
}

#[cfg(debug_assertions)]
fn dev_static_version(static_dir: Option<&Path>) -> Result<Cow<'static, str>, &'static str> {
    let Some(static_dir) = static_dir else {
        return Err("dashboard static directory is not configured");
    };
    let version = ["index.html", "app.css", "app.js", "dev-reload.js"]
        .into_iter()
        .map(|filename| {
            fs::metadata(static_dir.join(filename))
                .and_then(|metadata| metadata.modified())
                .and_then(|modified| {
                    modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(io::Error::other)
                })
                .map(|duration| duration.as_millis().to_string())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "failed to read dashboard asset metadata")?
        .join(".");

    Ok(Cow::Owned(version))
}

struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

impl HttpResponse {
    fn ok(content_type: &'static str, body: &str) -> Self {
        Self {
            status: "200 OK",
            content_type,
            body: body.to_string(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            content_type: "text/plain; charset=utf-8",
            body: String::from("not found\n"),
        }
    }

    fn bad_request() -> Self {
        Self {
            status: "400 Bad Request",
            content_type: "text/plain; charset=utf-8",
            body: String::from("bad request\n"),
        }
    }

    fn bad_json_request(body: &str) -> Self {
        Self {
            status: "400 Bad Request",
            content_type: "application/json; charset=utf-8",
            body: body.to_string(),
        }
    }

    fn forbidden() -> Self {
        Self {
            status: "403 Forbidden",
            content_type: "text/plain; charset=utf-8",
            body: String::from("forbidden\n"),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: "401 Unauthorized",
            content_type: "application/json; charset=utf-8",
            body: json_error_body(String::from("dashboard bearer token is required")),
        }
    }

    fn request_too_large() -> Self {
        Self {
            status: "431 Request Header Fields Too Large",
            content_type: "text/plain; charset=utf-8",
            body: String::from("request too large\n"),
        }
    }

    fn service_unavailable(body: &str) -> Self {
        Self {
            status: "503 Service Unavailable",
            content_type: "application/json; charset=utf-8",
            body: body.to_string(),
        }
    }

    fn internal_error(body: &str) -> Self {
        Self {
            status: "500 Internal Server Error",
            content_type: "application/json; charset=utf-8",
            body: body.to_string(),
        }
    }

    fn into_bytes(self) -> Vec<u8> {
        let body = self.body.into_bytes();
        let headers = format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            self.status,
            self.content_type,
            body.len()
        );
        let mut response = headers.into_bytes();
        response.extend(body);
        response
    }
}

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_CSS: &str = include_str!("../static/app.css");
const APP_JS: &str = include_str!("../static/app.js");
#[cfg(debug_assertions)]
const DEV_RELOAD_JS: &str = include_str!("../static/dev-reload.js");

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    #[test]
    fn dashboard_options_debug_and_server_bind_report_runtime_details() {
        let callback: DashboardCleanCallback = Arc::new(|_, _| Ok(()));
        let options = DashboardOptions {
            preferred_port: 0,
            clean_callback: Some(callback),
            ..DashboardOptions::default()
        };
        let debug = format!("{options:?}");

        assert!(debug.contains("DashboardOptions"));
        assert!(debug.contains("clean_callback: true"));

        let server = DashboardServer::bind(DashboardOptions {
            preferred_port: 0,
            ..DashboardOptions::default()
        })
        .expect("bind dashboard");

        assert_ne!(server.port(), 0);
        assert_eq!(server.url(), format!("http://127.0.0.1:{}", server.port()));
    }

    #[test]
    fn dashboard_bind_reports_no_available_port_when_preferred_and_fallback_are_busy() {
        let held =
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).expect("held listener");
        let port = held.local_addr().expect("held address").port();
        let options = DashboardOptions {
            preferred_port: port,
            fallback_range: PortRange {
                start: port,
                end: port,
            },
            skip_ports: Vec::new(),
            ..DashboardOptions::default()
        };

        let error = match DashboardServer::bind(options) {
            Ok(_) => panic!("bind should fail"),
            Err(error) => error,
        };

        assert!(
            matches!(error, DashboardError::NoAvailablePort { range } if range.start == port && range.end == port)
        );
        assert!(error.to_string().contains("no dashboard port available"));
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn dashboard_errors_expose_display_and_sources() {
        let bind = DashboardError::Bind {
            port: 27_080,
            source: io::Error::other("blocked"),
        };
        assert_eq!(
            bind.to_string(),
            "failed to bind dashboard port 27080: blocked"
        );
        assert!(std::error::Error::source(&bind).is_some());

        let local = DashboardError::LocalAddress(io::Error::other("gone"));
        assert_eq!(local.to_string(), "failed to read dashboard address: gone");
        assert!(std::error::Error::source(&local).is_some());
    }

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
    fn unknown_route_returns_404() {
        let options = DashboardOptions::default();
        let response = response_for_request(&test_request("/missing"), &options);
        let text = String::from_utf8(response.into_bytes()).expect("response utf8");

        assert!(text.starts_with("HTTP/1.1 404 Not Found"));
    }

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
    fn handle_connection_serves_valid_and_invalid_http_requests() {
        let ok = dashboard_round_trip(
            b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:27080\r\n\r\n",
            DashboardOptions::default(),
        );
        assert!(ok.starts_with("HTTP/1.1 200 OK"));
        assert!(ok.ends_with("ok\n"));

        let bad = dashboard_round_trip(
            b"GET\r\nHost: 127.0.0.1:27080\r\n\r\n",
            DashboardOptions::default(),
        );
        assert!(bad.starts_with("HTTP/1.1 400 Bad Request"));

        let large_header = format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:27080\r\nX-Fill: {}\r\n\r\n",
            "a".repeat(MAX_HEADER_LINE_BYTES + 1)
        );
        let large = dashboard_round_trip(large_header.as_bytes(), DashboardOptions::default());
        assert!(large.starts_with("HTTP/1.1 431 Request Header Fields Too Large"));
    }

    #[test]
    fn read_request_parses_first_headers_and_empty_streams() {
        let request = read_raw_dashboard_request(
            b"GET /api/status HTTP/1.1\r\nHost: first.local\r\nHost: second.local\r\nAuthorization: Bearer secret\r\nX-BindPort-Dashboard-Action: clean\r\n\r\n",
        )
        .expect("read request")
        .expect("request present");

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/status");
        assert_eq!(request.host.as_deref(), Some("first.local"));
        assert_eq!(request.authorization.as_deref(), Some("Bearer secret"));
        assert_eq!(request.dashboard_action.as_deref(), Some("clean"));

        let empty = read_raw_dashboard_request(b"").expect("read empty request");
        assert_eq!(empty, None);
    }

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
    fn limited_line_rejects_oversized_input() {
        let mut reader = Cursor::new(vec![b'a'; MAX_REQUEST_LINE_BYTES + 1]);

        let error = read_limited_line(&mut reader, MAX_REQUEST_LINE_BYTES)
            .expect_err("oversized request line");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn limited_line_reads_partial_lines_and_rejects_invalid_utf8() {
        let mut partial = Cursor::new(b"GET / HTTP/1.1".to_vec());
        assert_eq!(
            read_limited_line(&mut partial, MAX_REQUEST_LINE_BYTES).expect("partial line"),
            "GET / HTTP/1.1"
        );

        let mut invalid = Cursor::new(vec![0xff, b'\n']);
        let error =
            read_limited_line(&mut invalid, MAX_REQUEST_LINE_BYTES).expect_err("invalid utf8");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn json_error_body_escapes_message() {
        let body = json_error_body(String::from("registry unavailable: \"bad\"\npath"));
        let value = serde_json::from_str::<serde_json::Value>(&body).expect("json body");

        assert_eq!(value["error"], "registry unavailable: \"bad\"\npath");
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

    #[test]
    fn fallback_ports_skip_configured_ports() {
        let options = DashboardOptions {
            fallback_range: PortRange {
                start: 29_100,
                end: 29_102,
            },
            skip_ports: vec![29_101],
            ..DashboardOptions::default()
        };
        let ports = fallback_ports(&options).collect::<Vec<_>>();

        assert_eq!(ports, vec![29_100, 29_102]);
    }

    fn dashboard_round_trip(raw_request: &[u8], options: DashboardOptions) -> String {
        let listener =
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).expect("test listener");
        let address = listener.local_addr().expect("listener address");
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept connection");
            handle_connection(stream, &options).expect("handle request");
        });

        let mut client = TcpStream::connect(address).expect("connect client");
        client.write_all(raw_request).expect("write request");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown write");
        let mut response = String::new();
        client.read_to_string(&mut response).expect("read response");
        handle.join().expect("handler thread");

        response
    }

    fn read_raw_dashboard_request(raw_request: &[u8]) -> io::Result<Option<HttpRequest>> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept()?;
            read_request(&stream)
        });

        let mut client = TcpStream::connect(address)?;
        client.write_all(raw_request)?;
        client.shutdown(std::net::Shutdown::Write)?;

        handle.join().expect("request reader")
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bindport-dashboard-{name}-{}-{now}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp test dir");
        path
    }

    fn test_request(path: &str) -> HttpRequest {
        HttpRequest {
            method: String::from("GET"),
            path: path.to_string(),
            host: Some(String::from("127.0.0.1:27080")),
            authorization: None,
            dashboard_action: None,
        }
    }
}
