// SPDX-License-Identifier: MIT

#![allow(dead_code, unused_imports)]

pub use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub use bindport_core::{
    BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS,
    FALLBACK_CONFIG_FILE, SERVICE_NAME, ServiceIdentity,
};
pub use bindport_registry::{REGISTRY_PATH_ENV, Registry, RunStart};
pub use serde_json::Value;

pub fn bindport() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bindport"))
}

pub fn bindport_with_registry(registry_path: &Path) -> Command {
    let mut command = bindport();
    command.env(REGISTRY_PATH_ENV, registry_path);
    command.env("XDG_CONFIG_HOME", config_home_for_registry(registry_path));
    command.env("XDG_STATE_HOME", state_home_for_registry(registry_path));
    command.env_remove(BINDPORT_PROJECT_ENV);
    command.env_remove(BINDPORT_SERVICE_ENV);
    command
}

pub fn bindport_without_registry_path() -> Command {
    let mut command = bindport();
    command.env_remove(REGISTRY_PATH_ENV);
    command.env_remove("XDG_CONFIG_HOME");
    command.env_remove("XDG_STATE_HOME");
    command.env_remove("HOME");
    command.env_remove("APPDATA");
    command
}

pub fn config_home_for_registry(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("config-home")
}

pub fn state_home_for_registry(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("state-home")
}

#[cfg(unix)]
pub fn send_signal(pid: u32, signal: libc::c_int) {
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
    assert_eq!(result, 0, "send signal to process {pid}");
}

#[cfg(unix)]
pub fn terminate_process_from_file(path: &Path) {
    let Ok(pid) = fs::read_to_string(path) else {
        return;
    };
    let Ok(pid) = pid.trim().parse::<libc::pid_t>() else {
        return;
    };

    let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
}

#[cfg(unix)]
pub fn write_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents).expect("write executable fixture");
    let mut permissions = fs::metadata(path)
        .expect("executable fixture metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mark executable fixture");
}

#[cfg(unix)]
pub fn prepend_path(path: &Path) -> String {
    let existing_path = std::env::var_os("PATH").unwrap_or_default();

    format!("{}:{}", path.display(), existing_path.to_string_lossy())
}

pub fn wait_for_child(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(status) = child.try_wait().expect("poll child status") {
            return Some(status);
        }

        if Instant::now() >= deadline {
            return None;
        }

        thread::sleep(Duration::from_millis(25));
    }
}

pub fn wait_for_file_contains(path: &Path, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && contents.contains(needle)
        {
            return contents;
        }

        if Instant::now() >= deadline {
            panic!(
                "{} did not contain `{needle}` within {timeout:?}",
                path.display()
            );
        }

        thread::sleep(Duration::from_millis(25));
    }
}

pub fn wait_for_open_url(registry_path: &Path, args: &[&str], timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;

    loop {
        let output = bindport_with_registry(registry_path)
            .args(args)
            .output()
            .expect("run bindport open");

        if output.status.success() {
            return String::from_utf8(output.stdout).expect("open stdout");
        }

        if Instant::now() >= deadline {
            panic!(
                "bindport open did not succeed within {timeout:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        thread::sleep(Duration::from_millis(25));
    }
}

pub fn object_keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("json object")
        .keys()
        .map(String::as_str)
        .collect()
}

pub fn temp_registry_path(name: &str) -> PathBuf {
    temp_path(name).with_extension("sqlite")
}

pub fn temp_test_dir(name: &str) -> PathBuf {
    let path = temp_path(name);
    fs::create_dir_all(&path).expect("temp test dir");
    path
}

pub fn temp_path(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    std::env::temp_dir().join(format!("bindport-{name}-{}-{now}", std::process::id()))
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

pub fn run_print_port(registry_path: &Path, cwd: &Path) -> u16 {
    let output = bindport_with_registry(registry_path)
        .current_dir(cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    String::from_utf8(output.stdout)
        .expect("stdout is utf8")
        .parse::<u16>()
        .expect("stdout is a port number")
}

pub fn doctor_candidate_port(stdout: &str) -> u16 {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("next candidate port: "))
        .and_then(|value| value.split_whitespace().next())
        .expect("next candidate port line")
        .parse::<u16>()
        .expect("candidate is a port")
}

pub fn reserve_registry_port(registry_path: &Path, port: u16) {
    let mut registry = Registry::open(registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("busy-project"),
        service: String::from("busy-service"),
        git: None,
        identity_key: String::from("v1:busy"),
    };

    registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port,
            hostname: None,
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("busy fixture"),
            cwd: PathBuf::from("/tmp/bindport-busy-fixture"),
        })
        .expect("reserve registry port");
}

pub fn record_registry_service(registry_path: &Path, service: &str, port: u16) {
    let mut registry = Registry::open(registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("doctor-output-project"),
        service: service.to_string(),
        git: None,
        identity_key: format!("v1:doctor-output-project:{service}"),
    };

    registry
        .record_run_started(&RunStart {
            project: identity.project.clone(),
            service: identity.service.clone(),
            identity: Some(identity),
            host: String::from("127.0.0.1"),
            port,
            hostname: Some(format!("{service}.localhost")),
            route_url: None,
            health_url: None,
            pid: std::process::id(),
            command: String::from("doctor output fixture"),
            cwd: std::env::temp_dir().join("bindport-doctor-output-fixture"),
        })
        .expect("record registry service");
}

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

pub fn dotenv_value<'a>(contents: &'a str, name: &str) -> Option<&'a str> {
    contents
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

pub fn toml_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    format!("\"{escaped}\"")
}

pub fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

pub fn init_git_repo(root: &Path, branch: &str) {
    run_git(root, ["init"]);
    run_git(root, ["config", "user.email", "bindport@example.invalid"]);
    run_git(root, ["config", "user.name", "BindPort Test"]);
    run_git(root, ["config", "commit.gpgsign", "false"]);
    fs::write(root.join("README.md"), "test\n").expect("write git fixture");
    run_git(root, ["add", "README.md"]);
    run_git(root, ["commit", "-m", "initial"]);
    run_git(root, ["checkout", "-B", branch]);
}

pub fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
