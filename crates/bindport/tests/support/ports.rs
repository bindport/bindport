// SPDX-License-Identifier: MIT

use super::*;

pub fn guarded_port_range() -> (Vec<TcpListener>, u16, u16) {
    const SIZE: u16 = 8;

    for _ in 0..1_000 {
        let first = TcpListener::bind(("127.0.0.1", 0)).expect("first port");
        let start = first.local_addr().expect("first address").port();
        let Some(end) = start.checked_add(SIZE - 1) else {
            continue;
        };
        let mut listeners = vec![first];
        for port in (start + 1)..=end {
            let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) else {
                break;
            };
            listeners.push(listener);
        }
        if listeners.len() == usize::from(SIZE) {
            return (listeners, start, end);
        }
    }

    panic!("could not claim {SIZE} contiguous loopback ports");
}
