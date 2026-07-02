// SPDX-License-Identifier: MIT

use super::*;
use std::{
    net::{Shutdown, TcpListener},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

mod cleanup;
mod health;
mod leases;
mod outputs;
mod registry;
mod status;

fn test_run_start(project: &str, service: &str, port: u16, pid: u32) -> RunStart {
    RunStart {
        project: String::from(project),
        service: String::from(service),
        identity: None,
        host: String::from("127.0.0.1"),
        port,
        hostname: None,
        route_url: None,
        health_url: None,
        pid,
        command: String::from("next dev"),
        cwd: PathBuf::from("/tmp/bindport"),
    }
}

fn mark_latest_run_started_before_grace(registry: &Registry) {
    registry
        .connection
        .execute(
            "UPDATE runs
             SET started_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-5 seconds')",
            [],
        )
        .expect("backdate run start");
}

fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

fn start_health_server(status: &'static str) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind health server");
    let port = listener.local_addr().expect("health server addr").port();

    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 128];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes) => {
                    request.extend_from_slice(&buffer[..bytes]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(_) => return,
            }
        }
        let _ = write!(
            &mut stream,
            "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.flush();
        let _ = stream.shutdown(Shutdown::Write);
    });

    port
}

fn temp_registry_path(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    env::temp_dir().join(format!(
        "bindport-registry-{name}-{}-{now}.sqlite",
        std::process::id()
    ))
}

fn test_identity(identity_key: &str) -> ServiceIdentity {
    ServiceIdentity {
        project: String::from("bindport"),
        service: String::from("web"),
        git: None,
        identity_key: identity_key.to_owned(),
    }
}
