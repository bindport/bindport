// SPDX-License-Identifier: MIT

use std::{
    collections::HashSet,
    fmt, io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener},
    process::{Command, ExitStatus, Stdio},
};

use bindport_core::PortRange;

pub const PORT_ENV_VAR: &str = "PORT";

#[derive(Debug)]
pub enum RunnerError {
    NoCommand,
    NoAvailablePort { range: PortRange },
    Spawn { command: String, source: io::Error },
    Wait { command: String, source: io::Error },
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCommand => write!(f, "no command provided after `--`"),
            Self::NoAvailablePort { range } => {
                write!(
                    f,
                    "no available port found in range {}-{}",
                    range.start, range.end
                )
            }
            Self::Spawn { command, source } => {
                write!(f, "failed to spawn `{command}`: {source}")
            }
            Self::Wait { command, source } => {
                write!(f, "failed waiting for `{command}`: {source}")
            }
        }
    }
}

impl std::error::Error for RunnerError {}

/// Scans the configured TCP loopback range and returns the first available port.
///
/// This bootstrap runner drops the probe listener before spawning the child, so
/// another process can still claim the port before the child binds. The
/// registry/lease slice must close that gap.
pub fn allocate_port(range: PortRange, skip_ports: &[u16]) -> Result<u16, RunnerError> {
    let skip_ports = skip_ports.iter().copied().collect::<HashSet<_>>();

    for port in range.start..=range.end {
        if skip_ports.contains(&port) {
            continue;
        }

        if is_port_available(port) {
            return Ok(port);
        }
    }

    Err(RunnerError::NoAvailablePort { range })
}

/// Returns true when no supported TCP loopback family reports `port` in use.
///
/// Missing address families are not conflicts, so IPv4-only hosts can still
/// allocate a loopback port. UDP availability is outside the current runner
/// scope.
pub fn is_port_available(port: u16) -> bool {
    let v4 = loopback_free(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let v6 = loopback_free(SocketAddrV6::new(Ipv6Addr::LOCALHOST, port, 0, 0));

    v4 && v6
}

fn loopback_free(addr: impl Into<SocketAddr>) -> bool {
    match TcpListener::bind(addr.into()) {
        Ok(_) => true,
        Err(error) => bind_error_leaves_port_available(error.kind()),
    }
}

fn bind_error_leaves_port_available(kind: io::ErrorKind) -> bool {
    kind != io::ErrorKind::AddrInUse
}

pub fn run_child(
    command: &[String],
    range: PortRange,
    skip_ports: &[u16],
) -> Result<ExitStatus, RunnerError> {
    let (program, args) = command.split_first().ok_or(RunnerError::NoCommand)?;
    let port = allocate_port(range, skip_ports)?;

    let mut child = Command::new(program)
        .args(args)
        .env(PORT_ENV_VAR, port.to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|source| RunnerError::Spawn {
            command: program.clone(),
            source,
        })?;

    child.wait().map_err(|source| RunnerError::Wait {
        command: program.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_port_skips_reserved_ports() {
        let range = PortRange {
            start: 29_000,
            end: 29_001,
        };

        assert_eq!(allocate_port(range, &[29_000]).expect("port"), 29_001);
    }

    #[test]
    fn allocate_port_reports_exhausted_range() {
        let range = PortRange {
            start: 29_000,
            end: 29_000,
        };

        let error = allocate_port(range, &[29_000]).expect_err("range should be exhausted");
        assert!(matches!(error, RunnerError::NoAvailablePort { range: _ }));
    }

    #[test]
    fn bind_errors_only_conflict_when_address_is_in_use() {
        assert!(!bind_error_leaves_port_available(io::ErrorKind::AddrInUse));
        assert!(bind_error_leaves_port_available(
            io::ErrorKind::AddrNotAvailable
        ));
        assert!(bind_error_leaves_port_available(io::ErrorKind::Unsupported));
    }
}
