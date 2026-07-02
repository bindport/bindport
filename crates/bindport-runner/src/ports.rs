// SPDX-License-Identifier: MIT

use std::{
    collections::HashSet,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener},
};

use bindport_core::PortRange;

use crate::RunnerError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocationHints {
    pub preferred_port: Option<u16>,
    pub scan_start: Option<u16>,
}

/// Scans the configured TCP loopback range and returns an available port.
///
/// This bootstrap runner drops the probe listener before spawning the child, so
/// another process can still claim the port before the child binds. The
/// registry/lease slice must close that gap for strong coordination.
pub fn allocate_port(range: PortRange, skip_ports: &[u16]) -> Result<u16, RunnerError> {
    allocate_port_with_hints(range, skip_ports, AllocationHints::default())
}

pub fn allocate_port_with_hints(
    range: PortRange,
    skip_ports: &[u16],
    hints: AllocationHints,
) -> Result<u16, RunnerError> {
    allocate_port_with_hints_and_availability(range, skip_ports, hints, is_port_available)
}

pub(crate) fn allocate_port_with_hints_and_availability(
    range: PortRange,
    skip_ports: &[u16],
    hints: AllocationHints,
    mut is_available: impl FnMut(u16) -> bool,
) -> Result<u16, RunnerError> {
    let skip_ports = skip_ports.iter().copied().collect::<HashSet<_>>();

    if let Some(port) = hints
        .preferred_port
        .filter(|port| range.contains(*port) && !skip_ports.contains(port))
        && is_available(port)
    {
        return Ok(port);
    }

    let range_len = range.len();
    if range_len == 0 {
        return Err(RunnerError::NoAvailablePort { range });
    }

    let scan_start = hints
        .scan_start
        .filter(|port| range.contains(*port))
        .unwrap_or(range.start);
    let scan_start_offset = scan_start as u32 - range.start as u32;

    for offset in 0..range_len {
        let port = range.start as u32 + ((scan_start_offset + offset) % range_len);
        let port = u16::try_from(port).expect("port remains within configured range");

        if skip_ports.contains(&port) {
            continue;
        }

        if is_available(port) {
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

pub(crate) fn bind_error_leaves_port_available(kind: io::ErrorKind) -> bool {
    kind != io::ErrorKind::AddrInUse
}
