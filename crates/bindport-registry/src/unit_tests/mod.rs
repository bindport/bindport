// SPDX-License-Identifier: MIT

use super::*;
use std::{
    net::{Shutdown, TcpListener},
    sync::Mutex,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

mod cleanup;
mod export;
mod health;
mod leases;
mod outputs;
mod registry;
mod status;

static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_env_overrides<T>(updates: &[(&str, Option<&Path>)], callback: impl FnOnce() -> T) -> T {
    let _guard = TEST_ENV_LOCK.lock().expect("test env lock");
    let previous = updates
        .iter()
        .map(|(name, _)| (*name, env::var_os(name)))
        .collect::<Vec<_>>();

    // SAFETY: these unit tests serialize process environment mutation with
    // TEST_ENV_LOCK and restore previous values before returning.
    unsafe {
        for (name, value) in updates {
            match value {
                Some(value) => env::set_var(*name, *value),
                None => env::remove_var(*name),
            }
        }
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
    unsafe {
        for (name, value) in previous {
            match value {
                Some(value) => env::set_var(name, value),
                None => env::remove_var(name),
            }
        }
    }

    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

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
        command: current_process_command(),
        cwd: PathBuf::from("/tmp/bindport"),
    }
}

fn current_process_command() -> String {
    env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| String::from("bindport test"))
}

fn test_output_scope(root: &str) -> OutputFileScope {
    OutputFileScope::new(PathBuf::from(root), PathBuf::from(root), None, None)
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

fn start_raw_health_server(response: Vec<u8>) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind raw health server");
    let port = listener
        .local_addr()
        .expect("raw health server addr")
        .port();

    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut buffer = [0_u8; 128];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) if buffer.windows(4).any(|window| window == b"\r\n\r\n") => break,
                Ok(_) => continue,
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
        let _ = stream.write_all(&response);
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
