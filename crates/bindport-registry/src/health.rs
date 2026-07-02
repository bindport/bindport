use super::*;

pub(crate) fn health_status(state: &str, health_url: Option<&str>, run_age_ms: i64) -> String {
    if state != "active" {
        return String::from("unknown");
    }

    let Some(health_url) = health_url.filter(|url| !url.trim().is_empty()) else {
        return String::from("unknown");
    };

    if run_age_ms < HEALTH_PENDING_GRACE_MS {
        return String::from("pending");
    }

    match check_http_health(health_url) {
        HealthProbe::Healthy => String::from("healthy"),
        HealthProbe::Failing => String::from("failing"),
        HealthProbe::Unknown => String::from("unknown"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealthProbe {
    Healthy,
    Failing,
    Unknown,
}

pub(crate) fn check_http_health(url: &str) -> HealthProbe {
    let target = match http_health_target(url) {
        Ok(Some(target)) => target,
        Ok(None) => return HealthProbe::Unknown,
        Err(()) => return HealthProbe::Failing,
    };

    match probe_http_target(&target) {
        Ok(status) if (200..400).contains(&status) => HealthProbe::Healthy,
        Ok(_) | Err(_) => HealthProbe::Failing,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpHealthTarget {
    pub(crate) address: SocketAddr,
    pub(crate) path: String,
    pub(crate) authority: String,
}

pub(crate) fn http_health_target(url: &str) -> Result<Option<HttpHealthTarget>, ()> {
    if url.bytes().any(is_http_request_unsafe_byte) {
        return Err(());
    }
    let url = url.trim();
    let Some(rest) = url.strip_prefix("http://") else {
        return if url.starts_with("https://") {
            Ok(None)
        } else {
            Err(())
        };
    };
    let (authority, path) = rest
        .split_once('/')
        .map(|(authority, path)| (authority, format!("/{path}")))
        .unwrap_or((rest, String::from("/")));
    if authority.bytes().any(is_http_request_unsafe_byte)
        || path.bytes().any(is_http_request_unsafe_byte)
    {
        return Err(());
    }
    let (host, port) = parse_http_authority(authority).ok_or(())?;
    let Some(address) = loopback_socket_addr(&host, port) else {
        return Ok(None);
    };

    Ok(Some(HttpHealthTarget {
        address,
        path,
        authority: authority.to_string(),
    }))
}

pub(crate) fn is_http_request_unsafe_byte(byte: u8) -> bool {
    byte <= 0x20 || byte == 0x7f
}

pub(crate) fn parse_http_authority(authority: &str) -> Option<(String, u16)> {
    if authority.is_empty() || authority.contains('@') {
        return None;
    }

    if let Some(rest) = authority.strip_prefix('[') {
        let (host, remainder) = rest.split_once(']')?;
        if host.is_empty() {
            return None;
        }
        let port = match remainder.strip_prefix(':') {
            Some(port) if !port.is_empty() => port.parse().ok()?,
            Some(_) => return None,
            None if remainder.is_empty() => 80,
            None => return None,
        };

        return Some((host.to_string(), port));
    }

    match authority.matches(':').count() {
        0 => Some((authority.to_string(), 80)),
        1 => {
            let (host, port) = authority.rsplit_once(':')?;
            if host.is_empty() || port.is_empty() {
                return None;
            }

            Some((host.to_string(), port.parse().ok()?))
        }
        _ => None,
    }
}

pub(crate) fn loopback_socket_addr(host: &str, port: u16) -> Option<SocketAddr> {
    if let Ok(address) = host.parse::<IpAddr>() {
        return address
            .is_loopback()
            .then_some(SocketAddr::new(address, port));
    }

    let normalized = host.trim_end_matches('.');
    let lower = normalized.to_ascii_lowercase();
    (lower == "localhost" || lower.ends_with(".localhost"))
        .then_some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

pub(crate) fn probe_http_target(target: &HttpHealthTarget) -> io::Result<u16> {
    let mut stream = TcpStream::connect_timeout(&target.address, HEALTH_CHECK_TIMEOUT)?;
    stream.set_read_timeout(Some(HEALTH_CHECK_TIMEOUT))?;
    stream.set_write_timeout(Some(HEALTH_CHECK_TIMEOUT))?;
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.path, target.authority
    )?;

    let mut response = Vec::new();
    let mut buffer = [0_u8; 128];
    while response.len() < 1024 {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => {
                response.extend_from_slice(&buffer[..bytes]);
                if response.contains(&b'\n') {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) && !response.is_empty() =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }

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
