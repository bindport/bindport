// SPDX-License-Identifier: MIT

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bindport_core::{
    BINDPORT_PROJECT_ENV, BINDPORT_SERVICE_ENV, DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS,
    FALLBACK_CONFIG_FILE, ServiceIdentity,
};
use bindport_registry::{REGISTRY_PATH_ENV, Registry, RunStart};
use serde_json::Value;

fn bindport() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bindport"))
}

fn bindport_with_registry(registry_path: &Path) -> Command {
    let mut command = bindport();
    command.env(REGISTRY_PATH_ENV, registry_path);
    command.env_remove(BINDPORT_PROJECT_ENV);
    command.env_remove(BINDPORT_SERVICE_ENV);
    command
}

fn bindport_without_registry_path() -> Command {
    let mut command = bindport();
    command.env_remove(REGISTRY_PATH_ENV);
    command.env_remove("XDG_STATE_HOME");
    command.env_remove("HOME");
    command.env_remove("APPDATA");
    command
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: libc::c_int) {
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
    assert_eq!(result, 0, "send signal to process {pid}");
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
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

#[test]
fn dash_dash_runs_child_with_assigned_port() {
    let registry_path = temp_registry_path("dash-dash");
    let output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let port = stdout.parse::<u16>().expect("stdout is a port number");

    assert!(DEFAULT_PORT_RANGE.contains(port));
    assert!(!DEFAULT_SKIP_PORTS.contains(&port));
}

#[test]
fn runner_preserves_child_exit_code() {
    let registry_path = temp_registry_path("exit-code");
    let status = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "exit 37"])
        .status()
        .expect("run bindport");

    assert_eq!(status.code(), Some(37));
}

#[test]
fn run_subcommand_accepts_dash_dash_separator() {
    let registry_path = temp_registry_path("run-subcommand");
    let output = bindport_with_registry(&registry_path)
        .args(["run", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}

#[test]
fn run_subcommand_service_argument_overrides_env_and_config() {
    let registry_path = temp_registry_path("identity-precedence");
    let root = temp_test_dir("identity-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"config-project\"\nservice = \"config-service\"\ndefault_range = \"29120-29120\"\n",
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(BINDPORT_PROJECT_ENV, "env-project")
        .env(BINDPORT_SERVICE_ENV, "env-service")
        .args([
            "run",
            "cli-service",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PORT\"",
        ])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29120");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "env-project");
    assert_eq!(status["services"][0]["service"], "cli-service");
}

#[test]
fn wrapped_command_flags_are_passed_to_child() {
    let registry_path = temp_registry_path("flags");
    let output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$1\"", "sh", "--version"])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"--version");
}

#[test]
fn status_json_starts_empty() {
    let registry_path = temp_registry_path("empty-status");
    let output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(output.status.success());

    let status = serde_json::from_slice::<Value>(&output.stdout).expect("status json");
    assert_eq!(status["schema_version"], "0.1");
    assert_eq!(status["services"].as_array().expect("services").len(), 0);
    assert_eq!(status["runs"].as_array().expect("runs").len(), 0);
}

#[test]
fn status_json_reports_finished_run() {
    let registry_path = temp_registry_path("finished-status");
    let run_output = bindport_with_registry(&registry_path)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(run_output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(services.len(), 1);
    assert_eq!(runs.len(), 1);
    assert_eq!(services[0]["state"], "stopped");
    assert_eq!(services[0]["exit_code"], 0);
    assert!(services[0]["port"].as_u64().expect("port") >= DEFAULT_PORT_RANGE.start as u64);
    assert!(services[0]["port"].as_u64().expect("port") <= DEFAULT_PORT_RANGE.end as u64);
    assert_eq!(runs[0]["exit_code"], 0);
}

#[test]
fn runner_reuses_previous_identity_port_when_available() {
    let registry_path = temp_registry_path("sticky-registry");
    let root = temp_test_dir("sticky-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29300-29301\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    let second_port = run_print_port(&registry_path, &root);

    assert_eq!(second_port, first_port);
}

#[test]
fn runner_falls_back_when_previous_identity_port_is_active() {
    let registry_path = temp_registry_path("sticky-occupied-registry");
    let root = temp_test_dir("sticky-occupied-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29310-29311\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    reserve_registry_port(&registry_path, first_port);
    let second_port = run_print_port(&registry_path, &root);

    assert_ne!(second_port, first_port);
    assert!(matches!(second_port, 29_310 | 29_311));
}

#[test]
fn runner_continues_when_registry_path_is_unavailable() {
    let output = bindport_without_registry_path()
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let port = stdout.parse::<u16>().expect("stdout is a port number");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");

    assert!(DEFAULT_PORT_RANGE.contains(port));
    assert!(stderr.contains("running without registry recording"));
}

#[test]
fn parent_project_config_sets_port_range_and_project() {
    let registry_path = temp_registry_path("project-config-registry");
    let root = temp_test_dir("project-config-root");
    let nested = root.join("packages").join("web");
    fs::create_dir_all(&nested).expect("nested dir");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"configured-project\"\ndefault_range = \"29100-29101\"\nskip_ports = [29100]\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&nested)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29101");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "configured-project");
}

#[test]
fn status_json_reports_git_identity() {
    let registry_path = temp_registry_path("git-identity-registry");
    let root = temp_test_dir("git-identity-root");
    init_git_repo(&root, "feature/tree");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(
        service["project"],
        root.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(service["branch"], "feature/tree");
    assert_eq!(service["branch_label"], "feature-tree");
    assert_eq!(
        service["worktree_path"],
        root.canonicalize().unwrap().display().to_string()
    );
    assert!(service["commit"].as_str().expect("commit").len() >= 7);
    assert!(
        service["identity_key"]
            .as_str()
            .expect("identity key")
            .starts_with("v1:")
    );
}

#[test]
fn toml_config_wins_over_json_in_same_directory() {
    let registry_path = temp_registry_path("config-precedence-registry");
    let root = temp_test_dir("config-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "default_range = \"29110-29110\"\n",
    )
    .expect("write toml config");
    fs::write(
        root.join(".bindport.json"),
        r#"{"default_range":"29111-29111"}"#,
    )
    .expect("write json config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29110");
}

#[test]
fn fallback_config_next_to_registry_is_used_when_no_project_config_exists() {
    let state_dir = temp_test_dir("fallback-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let cwd = temp_test_dir("fallback-config-cwd");
    fs::write(
        state_dir.join(FALLBACK_CONFIG_FILE),
        "default_range = \"29200-29200\"\n",
    )
    .expect("write fallback config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29200");
}

#[test]
fn doctor_reports_unknown_config_keys() {
    let registry_path = temp_registry_path("doctor-unknown-config-registry");
    let root = temp_test_dir("doctor-unknown-config-root");
    fs::write(
        root.join(".bindport.toml"),
        "defaultrange = \"29100-29199\"\n[proxy.traefik]\nenabled = true\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["doctor"])
        .output()
        .expect("run bindport doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");

    assert!(stdout.contains("ignored unknown top-level keys: defaultrange, proxy"));
    assert!(stdout.contains("config applied keys: project, service, default_range, skip_ports"));
}

#[test]
fn init_creates_fallback_config_next_to_registry() {
    let state_dir = temp_test_dir("init-config-state");
    let registry_path = state_dir.join("registry.sqlite");
    let config_path = state_dir.join(FALLBACK_CONFIG_FILE);

    let output = bindport_with_registry(&registry_path)
        .args(["init"])
        .output()
        .expect("run bindport init");

    assert!(output.status.success());
    assert!(config_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let config = fs::read_to_string(&config_path).expect("fallback config");

    assert!(stdout.contains(&config_path.display().to_string()));
    assert!(config.contains("default_range = \"29000-29999\""));
}

#[cfg(unix)]
#[test]
fn forwards_sigterm_to_wrapped_child_and_records_exit() {
    let registry_path = temp_registry_path("signal-registry");
    let child_pid_path = temp_registry_path("signal-child-pid");
    let marker_path = temp_registry_path("signal-marker");
    let child_pid_path_arg = child_pid_path.display().to_string();
    let marker_path_arg = marker_path.display().to_string();

    let mut bindport = bindport_with_registry(&registry_path)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s\n' $$ > \"$1\"; trap 'printf forwarded > \"$2\"; exit 42' TERM INT; printf 'ready\n'; while :; do sleep 1; done",
            "sh",
            &child_pid_path_arg,
            &marker_path_arg,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run bindport");

    let stdout = bindport.stdout.take().expect("stdout pipe");
    let mut stdout = BufReader::new(stdout);
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read readiness line");
    assert_eq!(ready, "ready\n");

    let child_pid = fs::read_to_string(&child_pid_path)
        .expect("child pid file")
        .trim()
        .parse::<u32>()
        .expect("child pid");

    send_signal(bindport.id(), libc::SIGTERM);

    let status = match wait_for_child(&mut bindport, Duration::from_secs(5)) {
        Some(status) => status,
        None => {
            send_signal(child_pid, libc::SIGKILL);
            let _ = bindport.kill();
            panic!("bindport did not exit after SIGTERM");
        }
    };

    assert_eq!(status.code(), Some(42));
    assert_eq!(
        fs::read_to_string(&marker_path).expect("marker"),
        "forwarded"
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    assert_eq!(status["services"][0]["state"], "stopped");
    assert_eq!(status["services"][0]["exit_code"], 42);
    assert_eq!(status["runs"][0]["exit_code"], 42);
}

fn temp_registry_path(name: &str) -> PathBuf {
    temp_path(name).with_extension("sqlite")
}

fn temp_test_dir(name: &str) -> PathBuf {
    let path = temp_path(name);
    fs::create_dir_all(&path).expect("temp test dir");
    path
}

fn temp_path(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    std::env::temp_dir().join(format!("bindport-{name}-{}-{now}", std::process::id()))
}

fn run_print_port(registry_path: &Path, cwd: &Path) -> u16 {
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

fn reserve_registry_port(registry_path: &Path, port: u16) {
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
            pid: std::process::id(),
            command: String::from("busy fixture"),
            cwd: PathBuf::from("/tmp/bindport-busy-fixture"),
        })
        .expect("reserve registry port");
}

fn init_git_repo(root: &Path, branch: &str) {
    run_git(root, ["init"]);
    run_git(root, ["config", "user.email", "bindport@example.invalid"]);
    run_git(root, ["config", "user.name", "BindPort Test"]);
    run_git(root, ["config", "commit.gpgsign", "false"]);
    fs::write(root.join("README.md"), "test\n").expect("write git fixture");
    run_git(root, ["add", "README.md"]);
    run_git(root, ["commit", "-m", "initial"]);
    run_git(root, ["checkout", "-B", branch]);
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) {
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
