use super::*;
use std::time::Instant;

pub(crate) fn probe_http_target(target: &HttpHealthTarget) -> io::Result<u16> {
    let deadline = Instant::now() + HEALTH_CHECK_TIMEOUT;
    let mut stream = TcpStream::connect_timeout(&target.address, remaining_time(deadline)?)?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.path, target.authority
    );
    let mut pending = request.as_bytes();
    while !pending.is_empty() {
        stream.set_write_timeout(Some(remaining_time(deadline)?))?;
        match stream.write(pending) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "empty health request write",
                ));
            }
            Ok(bytes) => pending = &pending[bytes..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }

    let mut response = Vec::new();
    let mut buffer = [0_u8; 128];
    while response.len() < 1024 {
        stream.set_read_timeout(Some(remaining_time(deadline)?))?;
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => {
                response.extend_from_slice(&buffer[..bytes]);
                if response.contains(&b'\n') {
                    break;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }

    remaining_time(deadline)?;
    if response.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty health response",
        ));
    }

    let response = std::str::from_utf8(&response)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "missing HTTP status in `{}`",
                    response.lines().next().unwrap_or_default()
                ),
            )
        })?;

    Ok(status)
}

fn remaining_time(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "health probe deadline elapsed"))
}
