use super::*;

mod connection;
pub(crate) use connection::*;

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
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => self.spawn_connection(stream),
                Err(error) => {
                    eprintln!("dashboard: accept error: {error}");
                }
            }
        }

        Ok(())
    }

    /// Stops accepting connections when `should_stop` returns true, polling
    /// after a 25 ms sleep while idle. Does not wait for existing request threads.
    pub fn serve_until(self, should_stop: impl Fn() -> bool) -> io::Result<()> {
        self.listener.set_nonblocking(true)?;
        while !should_stop() {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false)?;
                    self.spawn_connection(stream);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("dashboard: accept error: {error}");
                    thread::sleep(Duration::from_millis(25));
                }
            }
        }
        Ok(())
    }

    fn spawn_connection(&self, stream: TcpStream) {
        let options = self.options.clone();
        thread::spawn(move || {
            if let Err(error) = handle_connection(stream, &options)
                && !is_routine_client_error(&error)
            {
                eprintln!("dashboard: request error: {error}");
            }
        });
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
