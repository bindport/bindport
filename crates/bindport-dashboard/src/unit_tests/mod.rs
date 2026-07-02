// SPDX-License-Identifier: MIT

use super::*;
use std::{
    io::{Cursor, Read},
    sync::Mutex,
};

mod assets;
mod auth;
mod clean;
mod request;
mod response;
mod routing;
mod server;

static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

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

fn temp_registry_path(name: &str) -> PathBuf {
    temp_test_dir(name).join("registry.sqlite")
}

fn with_default_registry_path<T>(path: &Path, callback: impl FnOnce() -> T) -> T {
    let _guard = TEST_ENV_LOCK.lock().expect("test env lock");
    let previous = std::env::var_os(bindport_registry::REGISTRY_PATH_ENV);

    // SAFETY: these unit tests serialize process environment mutation with
    // TEST_ENV_LOCK and restore the previous value before returning.
    unsafe {
        std::env::set_var(bindport_registry::REGISTRY_PATH_ENV, path);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
    match previous {
        Some(previous) => unsafe {
            std::env::set_var(bindport_registry::REGISTRY_PATH_ENV, previous);
        },
        None => unsafe {
            std::env::remove_var(bindport_registry::REGISTRY_PATH_ENV);
        },
    }

    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
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
