// SPDX-License-Identifier: MIT

use super::*;

pub struct DashboardProcess {
    pub child: Child,
    pub port: u16,
}

impl Drop for DashboardProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn start_dashboard(command: Command) -> DashboardProcess {
    start_dashboard_with_args(command, &["dashboard"])
}

pub fn start_dashboard_with_args(mut command: Command, args: &[&str]) -> DashboardProcess {
    let mut child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start bindport dashboard");
    let stdout = child.stdout.take().expect("dashboard stdout");
    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read dashboard URL");

    let port = line
        .trim()
        .strip_prefix("dashboard: http://")
        .expect("dashboard URL line")
        .rsplit_once(':')
        .map(|(_, port)| port)
        .expect("dashboard URL port")
        .parse::<u16>()
        .expect("dashboard port");

    DashboardProcess { child, port }
}

pub fn http_get(port: u16, path: &str) -> String {
    http_get_with_host(port, path, &format!("127.0.0.1:{port}"))
}

pub fn http_get_with_host(port: u16, path: &str, host: &str) -> String {
    http_get_with_headers(port, path, host, &[])
}

pub fn http_get_with_auth(port: u16, path: &str, authorization: &str) -> String {
    http_get_with_headers(
        port,
        path,
        &format!("127.0.0.1:{port}"),
        &[("Authorization", authorization)],
    )
}

pub fn http_post_clean(port: u16, path: &str, authorization: Option<&str>) -> String {
    let mut headers = vec![("X-BindPort-Dashboard-Action", "clean")];
    if let Some(authorization) = authorization {
        headers.push(("Authorization", authorization));
    }

    http_request_with_headers(port, "POST", path, &format!("127.0.0.1:{port}"), &headers)
}

pub fn http_get_with_headers(
    port: u16,
    path: &str,
    host: &str,
    headers: &[(&str, &str)],
) -> String {
    http_request_with_headers(port, "GET", path, host, headers)
}

pub fn http_request_with_headers(
    port: u16,
    method: &str,
    path: &str,
    host: &str,
    headers: &[(&str, &str)],
) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect dashboard");
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\n")
        .expect("write dashboard request");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("write dashboard request header");
    }
    if method == "POST" {
        write!(stream, "Content-Length: 0\r\n").expect("write dashboard request body length");
    }
    write!(stream, "Connection: close\r\n\r\n").expect("finish dashboard request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read dashboard response");
    response
}

pub fn http_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("http body separator")
}
