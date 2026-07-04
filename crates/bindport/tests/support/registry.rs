// SPDX-License-Identifier: MIT

use super::*;

pub fn doctor_candidate_port(stdout: &str) -> u16 {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("next candidate port: "))
        .and_then(|value| value.split_whitespace().next())
        .expect("next candidate port line")
        .parse::<u16>()
        .expect("candidate is a port")
}

pub fn current_process_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| String::from("bindport test"))
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
            command: current_process_command(),
            cwd: PathBuf::from("/tmp/bindport-busy-fixture"),
        })
        .expect("reserve registry port");
}

#[cfg(unix)]
pub fn record_stale_registry_service(registry_path: &Path, service: &str, port: u16) {
    let mut registry = Registry::open(registry_path).expect("registry");
    let identity = ServiceIdentity {
        project: String::from("pressure-project"),
        service: service.to_string(),
        git: None,
        identity_key: format!("v1:pressure-project:{service}"),
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
            pid: 2_000_000_000,
            command: String::from("stale fixture"),
            cwd: PathBuf::from("/tmp/bindport-stale-fixture"),
        })
        .expect("record stale registry service");
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
            command: current_process_command(),
            cwd: std::env::temp_dir().join("bindport-doctor-output-fixture"),
        })
        .expect("record registry service");
}

pub fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}
