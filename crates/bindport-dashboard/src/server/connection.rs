use super::*;

pub(crate) fn handle_connection(
    mut stream: TcpStream,
    options: &DashboardOptions,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let request = match read_request(&stream) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) if is_routine_client_error(&error) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            let response = if error.to_string().contains("too large") {
                HttpResponse::request_too_large()
            } else {
                HttpResponse::bad_request()
            };
            write_response(&mut stream, response)?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let response = response_for_request(&request, options);

    write_response(&mut stream, response)
}

pub(crate) fn write_response(stream: &mut TcpStream, response: HttpResponse) -> io::Result<()> {
    stream.write_all(&response.into_bytes())?;
    stream.flush()
}

pub(crate) fn is_routine_client_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::WouldBlock
    )
}
