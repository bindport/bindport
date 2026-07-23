// SPDX-License-Identifier: MIT

mod support;

use std::sync::{Arc, Barrier, mpsc};

use bindport_core::PortRange;
use support::*;

const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

struct TestPortRange {
    listeners: Vec<TcpListener>,
    start: u16,
    end: u16,
}

impl TestPortRange {
    fn claim(size: u16) -> Self {
        for _ in 0..1_000 {
            let first = TcpListener::bind(("127.0.0.1", 0)).expect("bind first test port");
            let start = first.local_addr().expect("first test address").port();
            let Some(end) = start.checked_add(size - 1) else {
                continue;
            };
            let mut listeners = vec![first];
            for port in (start + 1)..=end {
                let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) else {
                    break;
                };
                listeners.push(listener);
            }
            if listeners.len() == usize::from(size) {
                return Self {
                    listeners,
                    start,
                    end,
                };
            }
        }

        panic!("could not claim {size} contiguous loopback ports");
    }

    fn config_range(&self) -> String {
        format!("{}-{}", self.start, self.end)
    }
}

struct Client {
    child: Child,
}

impl Client {
    fn release(&mut self) {
        let mut stdin = self.child.stdin.take().expect("client stdin");
        stdin.write_all(b"\n").expect("release wrapped child");
        stdin.flush().expect("flush wrapped child release");
    }

    fn finish(mut self) -> ClientOutput {
        let status = match wait_for_child(&mut self.child, CLIENT_TIMEOUT) {
            Some(status) => status,
            None => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("concurrent bindport client did not exit within {CLIENT_TIMEOUT:?}");
            }
        };
        let mut stdout = Vec::new();
        if let Some(mut pipe) = self.child.stdout.take() {
            pipe.read_to_end(&mut stdout).expect("read client stdout");
        }
        let mut stderr = Vec::new();
        if let Some(mut pipe) = self.child.stderr.take() {
            pipe.read_to_end(&mut stderr).expect("read client stderr");
        }

        ClientOutput {
            status,
            stdout,
            stderr,
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct ClientOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[test]
fn concurrent_reserve_all_clients_are_idempotent_and_worktree_isolated() {
    let registry_path = temp_registry_path("concurrent-reserve-all");
    let port_range = TestPortRange::claim(12);
    let roots = (0..3)
        .map(|index| {
            let root = temp_test_dir(&format!("concurrent-reserve-root-{index}"));
            write_project_config(&root, &port_range.config_range(), &["web", "api"]);
            root.canonicalize().expect("canonical reservation root")
        })
        .collect::<Vec<_>>();
    drop(port_range.listeners);

    let commands = roots
        .iter()
        .flat_map(|root| {
            (0..2).map(|_| client_command(&registry_path, root, &["reserve", "--all"]))
        })
        .collect::<Vec<_>>();
    let outputs = run_concurrent_outputs(commands);

    for output in &outputs {
        assert_client_succeeded(output, "reserve --all");
    }
    for pair in outputs.chunks_exact(2) {
        assert_eq!(pair[0].stdout, pair[1].stdout);
    }

    let registry = Registry::open(&registry_path).expect("final reservation registry");
    let export = registry.export_snapshot().expect("reservation export");
    assert_eq!(export.leases.len(), 6);
    assert!(export.runs.is_empty());
    assert!(export.leases.iter().all(|lease| lease.state == "reserved"));
    assert_eq!(
        export
            .leases
            .iter()
            .map(|lease| lease.port)
            .collect::<BTreeSet<_>>()
            .len(),
        6
    );
    assert_eq!(
        export
            .leases
            .iter()
            .map(|lease| lease.identity_key.as_deref().expect("identity key"))
            .collect::<BTreeSet<_>>()
            .len(),
        6
    );

    let scoped_ports = roots
        .iter()
        .flat_map(|root| {
            [
                lookup_port(&registry_path, root, "web"),
                lookup_port(&registry_path, root, "api"),
            ]
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(scoped_ports.len(), 6);
}

#[test]
fn concurrent_run_clients_get_unique_stable_ports_without_lost_records() {
    let registry_path = temp_registry_path("concurrent-run-allocation");
    let port_range = TestPortRange::claim(16);
    let roots = distinct_run_roots(
        "concurrent-run-root",
        PortRange {
            start: port_range.start,
            end: port_range.end,
        },
        4,
    );
    drop(port_range.listeners);

    let commands = roots
        .iter()
        .map(|root| blocking_run_command(&registry_path, root))
        .collect::<Vec<_>>();
    let mut clients = spawn_concurrently(commands);
    let first_ports = read_ready_ports(&mut clients);
    assert_eq!(
        first_ports.iter().copied().collect::<BTreeSet<_>>().len(),
        4
    );
    wait_for_active_services(&registry_path, 4);

    let registry = Registry::open(&registry_path).expect("active allocation registry");
    let active = registry
        .export_snapshot()
        .expect("active allocation export");
    assert_complete_registry(&active, 4, "active");

    for client in &mut clients {
        client.release();
    }
    for output in clients.into_iter().map(Client::finish) {
        assert_client_succeeded(&output, "concurrent run");
    }

    let registry = Registry::open(&registry_path).expect("stopped allocation registry");
    let stopped = registry
        .export_snapshot()
        .expect("stopped allocation export");
    assert_complete_registry(&stopped, 4, "stopped");

    let commands = roots
        .iter()
        .map(|root| {
            client_command(
                &registry_path,
                root,
                &["run", "web", "--", "sh", "-c", "printf '%s' \"$PORT\""],
            )
        })
        .collect::<Vec<_>>();
    let outputs = run_concurrent_outputs(commands);
    let second_ports = outputs
        .iter()
        .map(|output| {
            assert_client_succeeded(output, "stable concurrent rerun");
            String::from_utf8(output.stdout.clone())
                .expect("port stdout")
                .parse::<u16>()
                .expect("decimal port")
        })
        .collect::<Vec<_>>();
    assert_eq!(second_ports, first_ports);

    let registry = Registry::open(&registry_path).expect("final allocation registry");
    let final_export = registry.export_snapshot().expect("final allocation export");
    assert_eq!(final_export.leases.len(), 8);
    assert_eq!(final_export.runs.len(), 8);
    assert!(
        final_export
            .leases
            .iter()
            .all(|lease| lease.state == "stopped")
    );
    assert!(
        final_export
            .runs
            .iter()
            .all(|run| run.exited_at.is_some() && run.exit_code == Some(0))
    );
}

#[test]
fn concurrent_reserved_runs_promote_original_leases_without_duplicates() {
    let registry_path = temp_registry_path("concurrent-promotion");
    let port_range = TestPortRange::claim(12);
    let range = port_range.config_range();
    let roots = (0..4)
        .map(|index| {
            let root = temp_test_dir(&format!("concurrent-promotion-root-{index}"));
            write_project_config(&root, &range, &["web"]);
            root.canonicalize().expect("canonical promotion root")
        })
        .collect::<Vec<_>>();
    drop(port_range.listeners);

    let reservations = run_concurrent_outputs(
        roots
            .iter()
            .map(|root| client_command(&registry_path, root, &["reserve", "--all"]))
            .collect(),
    );
    for output in &reservations {
        assert_client_succeeded(output, "concurrent reservation");
    }
    let reserved_ports = roots
        .iter()
        .map(|root| lookup_port(&registry_path, root, "web"))
        .collect::<Vec<_>>();
    let registry = Registry::open(&registry_path).expect("reserved registry");
    let reserved = registry.export_snapshot().expect("reserved export");
    assert_eq!(reserved.leases.len(), 4);
    let reserved_ids = reserved
        .leases
        .iter()
        .map(|lease| lease.id)
        .collect::<BTreeSet<_>>();

    let mut clients = spawn_concurrently(
        roots
            .iter()
            .map(|root| blocking_run_command(&registry_path, root))
            .collect(),
    );
    let promoted_ports = read_ready_ports(&mut clients);
    assert_eq!(promoted_ports, reserved_ports);
    wait_for_active_services(&registry_path, 4);

    let registry = Registry::open(&registry_path).expect("promoted registry");
    let promoted = registry.export_snapshot().expect("promoted export");
    assert_eq!(
        promoted
            .leases
            .iter()
            .map(|lease| lease.id)
            .collect::<BTreeSet<_>>(),
        reserved_ids
    );
    assert_complete_registry(&promoted, 4, "active");

    for client in &mut clients {
        client.release();
    }
    for output in clients.into_iter().map(Client::finish) {
        assert_client_succeeded(&output, "reserved concurrent run");
    }

    let registry = Registry::open(&registry_path).expect("final promotion registry");
    let final_export = registry.export_snapshot().expect("final promotion export");
    assert_eq!(
        final_export
            .leases
            .iter()
            .map(|lease| lease.id)
            .collect::<BTreeSet<_>>(),
        reserved_ids
    );
    assert_complete_registry(&final_export, 4, "stopped");
}

fn distinct_run_roots(name: &str, range: PortRange, count: usize) -> Vec<PathBuf> {
    let mut scan_starts = BTreeSet::new();
    let mut roots = Vec::new();
    for index in 0..100 {
        let root = temp_test_dir(&format!("{name}-{index}"))
            .canonicalize()
            .expect("canonical run root");
        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &[],
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: Some("concurrent-project"),
            config_service: Some("web"),
        });
        let scan_start = identity.port_scan_start(range).expect("port scan start");
        if scan_starts.insert(scan_start) {
            write_project_config(&root, &format!("{}-{}", range.start, range.end), &["web"]);
            roots.push(root);
            if roots.len() == count {
                return roots;
            }
        }
    }

    panic!("could not create {count} roots with distinct port scan starts");
}

fn write_project_config(root: &Path, range: &str, services: &[&str]) {
    let mut config =
        format!("project = \"concurrent-project\"\ndefault_range = \"{range}\"\nskip_ports = []\n");
    for service in services {
        config.push_str(&format!("\n[[services]]\nname = \"{service}\"\n"));
    }
    fs::write(root.join(".bindport.toml"), config).expect("write concurrent project config");
}

fn client_command(registry_path: &Path, root: &Path, args: &[&str]) -> Command {
    let mut command = bindport_with_registry(registry_path);
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn blocking_run_command(registry_path: &Path, root: &Path) -> Command {
    client_command(
        registry_path,
        root,
        &[
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf '%s\\n' \"$PORT\"; IFS= read -r release || :",
        ],
    )
}

fn spawn_concurrently(commands: Vec<Command>) -> Vec<Client> {
    let count = commands.len();
    let barrier = Arc::new(Barrier::new(count));
    let (sender, receiver) = mpsc::channel();
    let handles = commands
        .into_iter()
        .enumerate()
        .map(|(index, mut command)| {
            let barrier = Arc::clone(&barrier);
            let sender = sender.clone();
            thread::spawn(move || {
                barrier.wait();
                sender
                    .send((index, command.spawn()))
                    .expect("send spawned client");
            })
        })
        .collect::<Vec<_>>();
    drop(sender);

    let deadline = Instant::now() + CLIENT_TIMEOUT;
    let mut clients = (0..count).map(|_| None).collect::<Vec<_>>();
    for _ in 0..count {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (index, child) = receiver
            .recv_timeout(remaining)
            .expect("concurrent clients did not spawn before timeout");
        clients[index] = Some(Client {
            child: child.expect("spawn concurrent bindport client"),
        });
    }
    for handle in handles {
        handle.join().expect("client spawn thread");
    }

    clients
        .into_iter()
        .map(|client| client.expect("spawned client slot"))
        .collect()
}

fn run_concurrent_outputs(commands: Vec<Command>) -> Vec<ClientOutput> {
    spawn_concurrently(commands)
        .into_iter()
        .map(Client::finish)
        .collect()
}

fn read_ready_ports(clients: &mut [Client]) -> Vec<u16> {
    let (sender, receiver) = mpsc::channel();
    let handles = clients
        .iter_mut()
        .enumerate()
        .map(|(index, client)| {
            let stdout = client.child.stdout.take().expect("client stdout");
            let sender = sender.clone();
            thread::spawn(move || {
                let mut line = String::new();
                let result = BufReader::new(stdout)
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        if bytes == 0 {
                            Err(String::from("client exited before reporting its port"))
                        } else {
                            line.trim()
                                .parse::<u16>()
                                .map_err(|error| error.to_string())
                        }
                    });
                sender.send((index, result)).expect("send client port");
            })
        })
        .collect::<Vec<_>>();
    drop(sender);

    let deadline = Instant::now() + CLIENT_TIMEOUT;
    let mut ports = vec![None; clients.len()];
    for _ in 0..clients.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (index, result) = receiver
            .recv_timeout(remaining)
            .expect("wrapped children did not report ports before timeout");
        ports[index] = Some(result.expect("wrapped child port"));
    }
    for handle in handles {
        handle.join().expect("client stdout thread");
    }

    ports
        .into_iter()
        .map(|port| port.expect("reported client port"))
        .collect()
}

fn wait_for_active_services(registry_path: &Path, expected: usize) {
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    loop {
        let mut registry = Registry::open(registry_path).expect("open active registry");
        let snapshot = registry.status_snapshot().expect("active status");
        let active = snapshot
            .services
            .iter()
            .filter(|service| service.state == "active")
            .count();
        if active == expected && snapshot.runs.len() == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "registry did not reach {expected} active services before timeout; active={active}, runs={}",
            snapshot.runs.len()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_complete_registry(
    export: &bindport_registry::RegistryExportSnapshot,
    expected: usize,
    lease_state: &str,
) {
    assert_eq!(export.leases.len(), expected);
    assert_eq!(export.runs.len(), expected);
    let states = export
        .leases
        .iter()
        .map(|lease| lease.state.as_str())
        .collect::<Vec<_>>();
    assert!(
        states.iter().all(|state| *state == lease_state),
        "expected every lease to be {lease_state}, got {states:?}"
    );
    assert_eq!(
        export
            .leases
            .iter()
            .map(|lease| lease.port)
            .collect::<BTreeSet<_>>()
            .len(),
        expected
    );
    let lease_ids = export
        .leases
        .iter()
        .map(|lease| lease.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        export
            .runs
            .iter()
            .map(|run| run.lease_id)
            .collect::<BTreeSet<_>>(),
        lease_ids
    );
    if lease_state == "active" {
        assert!(
            export
                .runs
                .iter()
                .all(|run| run.exited_at.is_none() && run.exit_code.is_none())
        );
    } else if lease_state == "stopped" {
        assert!(
            export
                .runs
                .iter()
                .all(|run| run.exited_at.is_some() && run.exit_code == Some(0))
        );
    }
}

fn lookup_port(registry_path: &Path, root: &Path, service: &str) -> u16 {
    let output = bindport_with_registry(registry_path)
        .current_dir(root)
        .args(["port", service])
        .output()
        .expect("port lookup");
    assert_client_succeeded(
        &ClientOutput {
            status: output.status,
            stdout: output.stdout.clone(),
            stderr: output.stderr,
        },
        "port lookup",
    );
    String::from_utf8(output.stdout)
        .expect("port stdout")
        .trim()
        .parse()
        .expect("decimal port")
}

fn assert_client_succeeded(output: &ClientOutput, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
