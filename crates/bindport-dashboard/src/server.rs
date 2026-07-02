use super::*;

pub struct DashboardServer {
    listener: TcpListener,
    options: DashboardOptions,
    port: u16,
}

impl DashboardServer {
    pub fn bind(options: DashboardOptions) -> Result<Self, DashboardError> {
        let listener = bind_dashboard_listener(&options)?;
        let port = listener
            .local_addr()
            .map_err(DashboardError::LocalAddress)?
            .port();

        Ok(Self {
            listener,
            options,
            port,
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn url(&self) -> String {
        format!("http://{}:{}", self.options.host, self.port)
    }

    pub fn serve(self) -> Result<(), DashboardError> {
        let options = self.options;
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let options = options.clone();
                    thread::spawn(move || {
                        if let Err(error) = handle_connection(stream, &options)
                            && !is_routine_client_error(&error)
                        {
                            eprintln!("dashboard: request error: {error}");
                        }
                    });
                }
                Err(error) => {
                    eprintln!("dashboard: accept error: {error}");
                }
            }
        }

        Ok(())
    }
}

pub(crate) fn bind_dashboard_listener(
    options: &DashboardOptions,
) -> Result<TcpListener, DashboardError> {
    match TcpListener::bind(SocketAddrV4::new(options.host, options.preferred_port)) {
        Ok(listener) => return Ok(listener),
        Err(error) if error.kind() != io::ErrorKind::AddrInUse => {
            return Err(DashboardError::Bind {
                port: options.preferred_port,
                source: error,
            });
        }
        Err(_) => {}
    }

    for port in fallback_ports(options) {
        match TcpListener::bind(SocketAddrV4::new(options.host, port)) {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => continue,
            Err(error) => {
                return Err(DashboardError::Bind {
                    port,
                    source: error,
                });
            }
        }
    }

    Err(DashboardError::NoAvailablePort {
        range: options.fallback_range,
    })
}

pub(crate) fn fallback_ports(options: &DashboardOptions) -> impl Iterator<Item = u16> + '_ {
    let range = options.fallback_range;
    (0..range.len()).filter_map(move |offset| {
        let port = range.start as u32 + offset;
        let port = u16::try_from(port).ok()?;

        (!options.skip_ports.contains(&port)).then_some(port)
    })
}

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
