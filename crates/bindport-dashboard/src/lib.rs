// SPDX-License-Identifier: MIT

use std::{
    fmt,
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    thread,
    time::Duration,
};

use bindport_core::{DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, PortRange};
use bindport_registry::Registry;

pub const DEFAULT_DASHBOARD_PORT: u16 = 27_080;
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct DashboardOptions {
    pub host: Ipv4Addr,
    pub preferred_port: u16,
    pub fallback_range: PortRange,
    pub skip_ports: Vec<u16>,
}

impl Default for DashboardOptions {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::LOCALHOST,
            preferred_port: DEFAULT_DASHBOARD_PORT,
            fallback_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
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
    host: Ipv4Addr,
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
            host: options.host,
            port,
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn serve(self) -> Result<(), DashboardError> {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    thread::spawn(move || {
                        if let Err(error) = handle_connection(stream)
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

fn fallback_ports(options: &DashboardOptions) -> impl Iterator<Item = u16> + '_ {
    let range = options.fallback_range;
    (0..range.len()).filter_map(move |offset| {
        let port = range.start as u32 + offset;
        let port = u16::try_from(port).ok()?;

        (!options.skip_ports.contains(&port)).then_some(port)
    })
}

fn handle_connection(mut stream: TcpStream) -> io::Result<()> {
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
    let response = response_for_request(&request);

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
}

fn read_request(stream: &TcpStream) -> io::Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream);
    let request_line = read_limited_line(&mut reader, MAX_REQUEST_LINE_BYTES)?;
    if request_line.is_empty() {
        return Ok(None);
    }

    let mut host = None;
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

fn response_for_request(request: &HttpRequest) -> HttpResponse {
    if !host_allowed(request.host.as_deref()) {
        return HttpResponse::forbidden();
    }

    match request_path(request) {
        Some("/") => HttpResponse::ok("text/html; charset=utf-8", DASHBOARD_HTML),
        Some("/api/status") => status_response(),
        Some("/healthz") => HttpResponse::ok("text/plain; charset=utf-8", "ok\n"),
        _ => HttpResponse::not_found(),
    }
}

fn request_path(request: &HttpRequest) -> Option<&str> {
    (request.method == "GET").then_some(request.path.as_str())
}

fn host_allowed(host: Option<&str>) -> bool {
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

    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1"
}

fn status_response() -> HttpResponse {
    match Registry::open_default().and_then(|mut registry| registry.status_snapshot()) {
        Ok(snapshot) => match serde_json::to_string_pretty(&snapshot) {
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

fn json_error_body(message: String) -> String {
    format!("{}\n", serde_json::json!({ "error": message }))
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

    fn forbidden() -> Self {
        Self {
            status: "403 Forbidden",
            content_type: "text/plain; charset=utf-8",
            body: String::from("forbidden\n"),
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

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>BindPort Dashboard</title>
  <style>
    :root { color-scheme: light dark; font-family: system-ui, sans-serif; }
    body { margin: 0; background: Canvas; color: CanvasText; }
    main { max-width: 1120px; margin: 0 auto; padding: 24px; }
    header { display: flex; align-items: baseline; justify-content: space-between; gap: 16px; margin-bottom: 20px; }
    h1 { font-size: 1.4rem; margin: 0; }
    .meta { color: color-mix(in srgb, CanvasText 68%, Canvas); font-size: 0.9rem; }
    table { width: 100%; border-collapse: collapse; font-size: 0.92rem; }
    th, td { border-bottom: 1px solid color-mix(in srgb, CanvasText 16%, Canvas); padding: 10px 8px; text-align: left; vertical-align: top; }
    th { font-size: 0.75rem; letter-spacing: 0; text-transform: uppercase; color: color-mix(in srgb, CanvasText 62%, Canvas); }
    code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 0.86rem; }
    .state { font-weight: 700; }
    .empty, .error { border: 1px solid color-mix(in srgb, CanvasText 16%, Canvas); padding: 16px; }
    .error { color: #9f1d20; }
    @media (max-width: 760px) {
      main { padding: 16px; }
      table, thead, tbody, th, td, tr { display: block; }
      thead { display: none; }
      tr { border-bottom: 1px solid color-mix(in srgb, CanvasText 16%, Canvas); padding: 8px 0; }
      td { border: 0; padding: 6px 0; }
      td::before { content: attr(data-label); display: block; font-size: 0.72rem; text-transform: uppercase; color: color-mix(in srgb, CanvasText 62%, Canvas); margin-bottom: 2px; }
    }
  </style>
</head>
<body>
  <main>
    <header>
      <h1>BindPort Dashboard</h1>
      <div id="generated-at" class="meta"></div>
    </header>
    <section id="content" class="empty">Loading...</section>
  </main>
  <script>
    const content = document.getElementById("content");
    const generatedAt = document.getElementById("generated-at");

    function text(value) {
      return value === null || value === undefined || value === "" ? "-" : String(value);
    }

    function escapeHtml(value) {
      return text(value)
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll("\"", "&quot;")
        .replaceAll("'", "&#039;");
    }

    function serviceUrl(service) {
      return service.route_url || service.url || "";
    }

    function render(snapshot) {
      generatedAt.textContent = snapshot.generated_at ? `Updated ${snapshot.generated_at}` : "";
      const services = snapshot.services || [];
      if (services.length === 0) {
        content.className = "empty";
        content.textContent = "No BindPort runs recorded yet.";
        return;
      }

      content.className = "";
      content.innerHTML = `<table>
        <thead>
          <tr>
            <th>State</th><th>Service</th><th>URL</th><th>Branch</th><th>PID</th><th>Command</th>
          </tr>
        </thead>
        <tbody></tbody>
      </table>`;
      const body = content.querySelector("tbody");
      for (const service of services) {
        const row = document.createElement("tr");
        const url = serviceUrl(service);
        row.innerHTML = `
          <td data-label="State"><span class="state">${escapeHtml(service.state)}</span></td>
          <td data-label="Service"><strong>${escapeHtml(service.project)}</strong><br>${escapeHtml(service.service)}</td>
          <td data-label="URL">${url ? `<a href="${escapeHtml(url)}">${escapeHtml(url)}</a>` : "-"}</td>
          <td data-label="Branch">${escapeHtml(service.branch_label || service.branch)}</td>
          <td data-label="PID">${escapeHtml(service.pid)}</td>
          <td data-label="Command"><code>${escapeHtml(service.command)}</code></td>`;
        body.appendChild(row);
      }
    }

    fetch("/api/status", { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw new Error(`status ${response.status}`);
        return response.json();
      })
      .then(render)
      .catch((error) => {
        content.className = "error";
        content.textContent = `Dashboard status unavailable: ${error.message}`;
      });
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn root_request_serves_dashboard_html() {
        let response = response_for_request(&test_request("/"));
        let bytes = response.into_bytes();
        let text = String::from_utf8(bytes).expect("response utf8");

        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains("BindPort Dashboard"));
        assert!(text.contains("/api/status"));
    }

    #[test]
    fn unknown_route_returns_404() {
        let response = response_for_request(&test_request("/missing"));
        let text = String::from_utf8(response.into_bytes()).expect("response utf8");

        assert!(text.starts_with("HTTP/1.1 404 Not Found"));
    }

    #[test]
    fn rejects_unknown_host_header() {
        let response = response_for_request(&HttpRequest {
            method: String::from("GET"),
            path: String::from("/api/status"),
            host: Some(String::from("example.com:27080")),
        });
        let text = String::from_utf8(response.into_bytes()).expect("response utf8");

        assert!(text.starts_with("HTTP/1.1 403 Forbidden"));
    }

    #[test]
    fn limited_line_rejects_oversized_input() {
        let mut reader = Cursor::new(vec![b'a'; MAX_REQUEST_LINE_BYTES + 1]);

        let error = read_limited_line(&mut reader, MAX_REQUEST_LINE_BYTES)
            .expect_err("oversized request line");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn json_error_body_escapes_message() {
        let body = json_error_body(String::from("registry unavailable: \"bad\"\npath"));
        let value = serde_json::from_str::<serde_json::Value>(&body).expect("json body");

        assert_eq!(value["error"], "registry unavailable: \"bad\"\npath");
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

    fn test_request(path: &str) -> HttpRequest {
        HttpRequest {
            method: String::from("GET"),
            path: path.to_string(),
            host: Some(String::from("127.0.0.1:27080")),
        }
    }
}
