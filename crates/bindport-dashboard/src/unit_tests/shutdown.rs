// SPDX-License-Identifier: MIT

use super::*;
use std::sync::mpsc;

#[test]
fn serve_until_stops_without_a_wakeup_connection() {
    let server = DashboardServer::bind(DashboardOptions {
        preferred_port: 0,
        ..DashboardOptions::default()
    })
    .expect("server");
    let port = server.port();
    let (stop, stopping) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result =
            server.serve_until(|| !matches!(stopping.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let _ = done.send(result);
    });
    stop.send(()).expect("request stop");
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("idle shutdown")
        .expect("serve result");
    worker.join().expect("server thread");
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
}

#[test]
fn serve_until_handles_fragmented_requests_with_blocking_streams() {
    let server = DashboardServer::bind(DashboardOptions {
        preferred_port: 0,
        ..DashboardOptions::default()
    })
    .expect("server");
    let port = server.port();
    let (stop, stopping) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result =
            server.serve_until(|| !matches!(stopping.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let _ = done.send(result);
    });
    let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("client");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read deadline");
    client
        .write_all(b"GET /healthz HTTP/1.1\r\n")
        .expect("request line");
    thread::sleep(Duration::from_millis(75));
    write!(
        client,
        "Host: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .expect("headers");
    let mut response = String::new();
    client.read_to_string(&mut response).expect("response");
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    stop.send(()).expect("request stop");
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("server shutdown")
        .expect("serve result");
    worker.join().expect("server thread");
}
