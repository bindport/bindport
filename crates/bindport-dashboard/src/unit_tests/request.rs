// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn handle_connection_serves_valid_and_invalid_http_requests() {
    let ok = dashboard_round_trip(
        b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:27080\r\n\r\n",
        DashboardOptions::default(),
    );
    assert!(ok.starts_with("HTTP/1.1 200 OK"));
    assert!(ok.ends_with("ok\n"));

    let bad = dashboard_round_trip(
        b"GET\r\nHost: 127.0.0.1:27080\r\n\r\n",
        DashboardOptions::default(),
    );
    assert!(bad.starts_with("HTTP/1.1 400 Bad Request"));

    let large_header = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:27080\r\nX-Fill: {}\r\n\r\n",
        "a".repeat(MAX_HEADER_LINE_BYTES + 1)
    );
    let large = dashboard_round_trip(large_header.as_bytes(), DashboardOptions::default());
    assert!(large.starts_with("HTTP/1.1 431 Request Header Fields Too Large"));
}

#[test]
fn read_request_parses_first_headers_and_empty_streams() {
    let request = read_raw_dashboard_request(
        b"GET /api/status HTTP/1.1\r\nHost: first.local\r\nHost: second.local\r\nAuthorization: Bearer secret\r\nX-BindPort-Dashboard-Action: clean\r\n\r\n",
    )
    .expect("read request")
    .expect("request present");

    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/status");
    assert_eq!(request.host.as_deref(), Some("first.local"));
    assert_eq!(request.authorization.as_deref(), Some("Bearer secret"));
    assert_eq!(request.dashboard_action.as_deref(), Some("clean"));

    let empty = read_raw_dashboard_request(b"").expect("read empty request");
    assert_eq!(empty, None);
}

#[test]
fn limited_line_rejects_oversized_input() {
    let mut reader = Cursor::new(vec![b'a'; MAX_REQUEST_LINE_BYTES + 1]);

    let error =
        read_limited_line(&mut reader, MAX_REQUEST_LINE_BYTES).expect_err("oversized request line");

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn limited_line_reads_partial_lines_and_rejects_invalid_utf8() {
    let mut partial = Cursor::new(b"GET / HTTP/1.1".to_vec());
    assert_eq!(
        read_limited_line(&mut partial, MAX_REQUEST_LINE_BYTES).expect("partial line"),
        "GET / HTTP/1.1"
    );

    let mut invalid = Cursor::new(vec![0xff, b'\n']);
    let error = read_limited_line(&mut invalid, MAX_REQUEST_LINE_BYTES).expect_err("invalid utf8");

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
