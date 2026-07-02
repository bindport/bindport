use super::*;

pub(crate) struct ListenerConflictScan {
    pub(crate) known_registry: Vec<u16>,
    pub(crate) unknown: Vec<u16>,
    pub(crate) scanned_ports: u32,
    pub(crate) total_ports: u32,
}

pub(crate) fn listener_conflicts(
    range: PortRange,
    known_registry_ports: &[u16],
) -> ListenerConflictScan {
    let total_ports = range.len();
    let scanned_ports = total_ports.min(DOCTOR_MAX_LISTENER_PROBES);
    let known_registry_ports = ports_in_range(known_registry_ports, range);
    let mut known_registry = Vec::new();
    let mut unknown = Vec::new();

    for offset in 0..scanned_ports {
        let port = range.start as u32 + offset;
        let port = u16::try_from(port).expect("port remains within configured range");

        if is_port_available(port) {
            continue;
        }

        if known_registry_ports.contains(&port) {
            known_registry.push(port);
        } else {
            unknown.push(port);
        }
    }

    ListenerConflictScan {
        known_registry,
        unknown,
        scanned_ports,
        total_ports,
    }
}

pub(crate) fn format_listener_conflict_scan(scan: &ListenerConflictScan) -> String {
    let mut summary = format_limited_ports(&scan.unknown);

    if scan.scanned_ports < scan.total_ports {
        summary.push_str(&format!(
            " (scanned first {} of {} ports)",
            scan.scanned_ports, scan.total_ports
        ));
    }

    summary
}

pub(crate) fn format_limited_ports(ports: &[u16]) -> String {
    if ports.is_empty() {
        return String::from("none");
    }

    let mut summary = ports
        .iter()
        .take(DOCTOR_PORT_DISPLAY_LIMIT)
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    if ports.len() > DOCTOR_PORT_DISPLAY_LIMIT {
        summary.push_str(&format!(
            " (+{} more)",
            ports.len() - DOCTOR_PORT_DISPLAY_LIMIT
        ));
    }

    summary
}
