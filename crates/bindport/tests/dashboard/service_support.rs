// SPDX-License-Identifier: MIT

use std::{collections::BTreeMap, process::Output, sync::Mutex};

use crate::support::*;

pub(super) struct ServiceFixture {
    pub(super) root: PathBuf,
    pub(super) registry: PathBuf,
    pub(super) state_home: PathBuf,
    pub(super) servers: Mutex<BTreeMap<u32, u16>>,
}

impl ServiceFixture {
    pub(super) fn new(name: &str) -> Self {
        let root = temp_test_dir(name).canonicalize().expect("canonical root");
        let state_home = root.join("state");
        fs::create_dir_all(state_home.join(SERVICE_NAME)).expect("state directory");
        fs::write(
            root.join(".bindport.toml"),
            "project = \"dashboard-lifecycle\"\n",
        )
        .expect("project config");
        Self {
            registry: root.join("registry.sqlite"),
            root,
            state_home,
            servers: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn command(&self, operation: &str) -> Command {
        let mut command = bindport_with_registry(&self.registry);
        command
            .current_dir(&self.root)
            .env("XDG_STATE_HOME", &self.state_home)
            .args(["dashboard", operation]);
        if operation == "start" {
            command.args([
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--auth",
                "disabled",
                "--no-register-service",
            ]);
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command
    }

    pub(super) fn finish(&self, mut child: Child) -> Output {
        if wait_for_child(&mut child, Duration::from_secs(10)).is_none() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("dashboard lifecycle command timed out");
        }
        let output = child.wait_with_output().expect("command output");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.starts_with("dashboard started:") {
            let (pid, port) = endpoint(&stdout);
            self.servers.lock().expect("server list").insert(pid, port);
        } else if output.status.success()
            && let Some(pid) = stdout
                .trim()
                .strip_prefix("dashboard stopped: pid ")
                .and_then(|pid| pid.parse::<u32>().ok())
        {
            self.servers.lock().expect("server list").remove(&pid);
        }
        output
    }

    pub(super) fn run(&self, operation: &str) -> Output {
        self.finish(
            self.command(operation)
                .spawn()
                .expect("spawn lifecycle command"),
        )
    }

    pub(super) fn lock_path(&self) -> PathBuf {
        self.state_home.join(SERVICE_NAME).join("dashboard.lock")
    }
}

impl Drop for ServiceFixture {
    fn drop(&mut self) {
        let servers = self.servers.get_mut().expect("server list");
        for &pid in servers.keys() {
            // SAFETY: these PIDs were returned by this fixture's successful starts.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let listening = servers
                .values()
                .filter(|&&port| TcpStream::connect(("127.0.0.1", port)).is_ok())
                .collect::<Vec<_>>();
            if listening.is_empty() {
                return;
            }
            if Instant::now() >= deadline {
                if thread::panicking() {
                    eprintln!("dashboard cleanup left listeners open: {listening:?}");
                    return;
                }
                panic!("dashboard cleanup left listeners open: {listening:?}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

pub(super) fn endpoint(stdout: &str) -> (u32, u16) {
    let (prefix, pid) = stdout.trim().rsplit_once(" pid ").expect("dashboard PID");
    let port = prefix
        .rsplit_once(':')
        .expect("dashboard port")
        .1
        .parse()
        .expect("port number");
    (pid.parse().expect("PID number"), port)
}

pub(super) fn open_lock(path: &Path) -> fs::File {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .expect("lock file")
}

pub(super) fn start_foreground(mut command: Command, stdout: &Path) -> DashboardProcess {
    let mut child = command
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--auth",
            "disabled",
            "--register-service",
        ])
        .stdout(fs::File::create(stdout).expect("foreground stdout"))
        .stderr(fs::File::create(stdout.with_extension("stderr")).expect("foreground stderr"))
        .spawn()
        .expect("foreground dashboard");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = fs::read_to_string(stdout).expect("read foreground stdout");
        if let Some(port) = output.lines().find_map(|line| {
            line.strip_prefix("dashboard: http://127.0.0.1:")
                .and_then(|port| port.parse().ok())
        }) {
            return DashboardProcess { child, port };
        }
        if Instant::now() >= deadline || child.try_wait().expect("foreground status").is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("foreground dashboard did not start: {output}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
