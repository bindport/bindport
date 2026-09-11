// SPDX-License-Identifier: MIT

use super::*;
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::Instant,
};

struct ProbeServer {
    target: HttpHealthTarget,
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl ProbeServer {
    fn start(serve: impl FnOnce(&mut TcpStream, &Receiver<()>) + Send + 'static) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("health listener");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let address = listener.local_addr().expect("health address");
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            let accept_deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("blocking accepted stream");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .expect("server read timeout");
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .expect("server write timeout");
                        serve(&mut stream, &stopped);
                        return;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if stopped.recv_timeout(Duration::from_millis(5)).is_ok() {
                            return;
                        }
                        assert!(Instant::now() < accept_deadline, "probe did not connect");
                    }
                    Err(error) => panic!("health accept failed: {error}"),
                }
            }
        });
        Self {
            target: HttpHealthTarget {
                address,
                path: String::from("/health"),
                authority: address.to_string(),
            },
            stop,
            worker: Some(worker),
        }
    }

    fn finish(mut self) {
        let _ = self.stop.send(());
        self.worker
            .take()
            .expect("server worker")
            .join()
            .expect("server panicked");
    }
}

impl Drop for ProbeServer {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn trickling_status_line_cannot_extend_the_probe_deadline() {
    let server = ProbeServer::start(|stream, stopped| {
        for byte in b"HTTP/1.1 200 OK\r\n" {
            if stopped.recv_timeout(Duration::from_millis(100)).is_ok()
                || stream.write_all(&[*byte]).is_err()
            {
                return;
            }
        }
        let _ = stopped.recv_timeout(Duration::from_secs(2));
    });
    let started = Instant::now();
    let result = probe_http_target(&server.target);
    let elapsed = started.elapsed();
    server.finish();

    assert!(
        elapsed < Duration::from_secs(1),
        "trickle exceeded total budget: {elapsed:?}"
    );
    let error = result.expect_err("trickled status line must time out");
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ),
        "{error}"
    );
}

#[test]
fn timed_out_partial_success_status_is_failing_in_registry_snapshot() {
    let server = ProbeServer::start(|stream, stopped| {
        stream.write_all(b"HTTP/1.1 200").expect("partial status");
        let _ = stopped.recv_timeout(Duration::from_secs(2));
    });
    let mut registry =
        Registry::open(temp_registry_path("health-partial-timeout")).expect("registry");
    let mut run = test_run_start(
        "health-deadline",
        "web",
        server.target.address.port(),
        std::process::id(),
    );
    run.health_url = Some(format!("http://{}/health", server.target.address));
    registry.record_run_started(&run).expect("record start");
    mark_latest_run_started_before_grace(&registry);

    let snapshot = registry.status_snapshot().expect("status");
    server.finish();
    assert_eq!(snapshot.services[0].health, "failing");
}

#[test]
fn fragmented_complete_status_within_budget_remains_healthy() {
    let server = ProbeServer::start(|stream, stopped| {
        stream.write_all(b"HTTP/1.1 ").expect("status prefix");
        if stopped.recv_timeout(Duration::from_millis(20)).is_ok() {
            return;
        }
        stream
            .write_all(b"204 No Content\r\n\r\n")
            .expect("status remainder");
        let _ = stopped.recv_timeout(Duration::from_secs(2));
    });
    let result = probe_http_target(&server.target);
    server.finish();
    assert_eq!(result.expect("complete response"), 204);
}

#[test]
fn request_write_and_response_read_share_the_probe_budget() {
    let mut server = ProbeServer::start(|stream, stopped| {
        if stopped.recv_timeout(Duration::from_millis(200)).is_ok() {
            return;
        }
        let mut tail = Vec::new();
        let mut buffer = [0_u8; 65_536];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => return,
                Ok(bytes) => {
                    tail.extend_from_slice(&buffer[..bytes]);
                    if tail.ends_with(b"\r\n\r\n") {
                        break;
                    }
                    if tail.len() > 3 {
                        tail.drain(..tail.len() - 3);
                    }
                }
                Err(_) => return,
            }
        }
        if stopped.recv_timeout(Duration::from_millis(200)).is_ok() {
            return;
        }
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n");
        let _ = stopped.recv_timeout(Duration::from_secs(2));
    });
    server.target.path = format!("/{}", "a".repeat(8 * 1024 * 1024));
    let started = Instant::now();
    let result = probe_http_target(&server.target);
    let elapsed = started.elapsed();
    server.finish();

    let error = result.expect_err("write time must not reset the response budget");
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ),
        "{error}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "probe exceeded total budget: {elapsed:?}"
    );
}

#[test]
fn eof_before_deadline_preserves_existing_status_parsing() {
    let server = ProbeServer::start(|stream, stopped| {
        stream
            .write_all(b"HTTP/1.1 200")
            .expect("status without newline");
        stream.shutdown(Shutdown::Write).expect("response EOF");
        let _ = stopped.recv_timeout(Duration::from_secs(2));
    });
    let result = probe_http_target(&server.target);
    server.finish();
    assert_eq!(result.expect("existing EOF parsing"), 200);
}

#[test]
fn probe_http_target_reports_empty_invalid_and_malformed_responses() {
    let empty = start_raw_health_server(Vec::new());
    let target = http_health_target(&format!("http://127.0.0.1:{empty}/health"))
        .expect("empty target")
        .expect("loopback");
    let error = probe_http_target(&target).expect_err("empty response");
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);

    let invalid_utf8 = start_raw_health_server(vec![0xff, b'\n']);
    let target = http_health_target(&format!("http://127.0.0.1:{invalid_utf8}/health"))
        .expect("invalid target")
        .expect("loopback");
    let error = probe_http_target(&target).expect_err("invalid utf8");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let malformed = start_raw_health_server(b"HTTP/1.1 OK\r\n\r\n".to_vec());
    let target = http_health_target(&format!("http://127.0.0.1:{malformed}/health"))
        .expect("malformed target")
        .expect("loopback");
    let error = probe_http_target(&target).expect_err("malformed status");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("missing HTTP status"));
}
