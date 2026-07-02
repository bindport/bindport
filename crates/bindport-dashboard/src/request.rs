use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) host: Option<String>,
    pub(crate) authorization: Option<String>,
    pub(crate) dashboard_action: Option<String>,
}

pub(crate) fn read_request(stream: &TcpStream) -> io::Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream);
    let request_line = read_limited_line(&mut reader, MAX_REQUEST_LINE_BYTES)?;
    if request_line.is_empty() {
        return Ok(None);
    }

    let mut host = None;
    let mut authorization = None;
    let mut dashboard_action = None;
    let mut header_bytes = 0;
    loop {
        let header = read_limited_line(&mut reader, MAX_HEADER_LINE_BYTES)?;
        if header.is_empty() || header == "\r\n" || header == "\n" {
            break;
        }
        header_bytes += header.len();
        if header_bytes > MAX_HEADER_BYTES {
            return Err(request_too_large_error());
        }

        if let Some((name, value)) = header.trim_end().split_once(':')
            && name.eq_ignore_ascii_case("host")
            && host.is_none()
        {
            host = Some(value.trim().to_string());
        }
        if let Some((name, value)) = header.trim_end().split_once(':')
            && name.eq_ignore_ascii_case("authorization")
            && authorization.is_none()
        {
            authorization = Some(value.trim().to_string());
        }
        if let Some((name, value)) = header.trim_end().split_once(':')
            && name.eq_ignore_ascii_case(DASHBOARD_ACTION_HEADER)
            && dashboard_action.is_none()
        {
            dashboard_action = Some(value.trim().to_string());
        }
    }

    let mut parts = request_line.split_whitespace();
    let Some(method) = parts.next() else {
        return Err(invalid_request_error());
    };
    let Some(path) = parts.next() else {
        return Err(invalid_request_error());
    };

    Ok(Some(HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        host,
        authorization,
        dashboard_action,
    }))
}

pub(crate) fn read_limited_line(reader: &mut impl BufRead, limit: usize) -> io::Result<String> {
    let mut bytes = Vec::new();

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);

        if bytes.len() + length > limit {
            return Err(request_too_large_error());
        }

        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);

        if bytes.last() == Some(&b'\n') {
            break;
        }
    }

    String::from_utf8(bytes).map_err(|_| invalid_request_error())
}

pub(crate) fn request_too_large_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "dashboard request too large")
}

pub(crate) fn invalid_request_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid dashboard request")
}
