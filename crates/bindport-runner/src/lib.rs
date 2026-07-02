// SPDX-License-Identifier: MIT

mod child;
mod error;
mod ports;
mod signals;

pub const PORT_ENV_VAR: &str = "PORT";

pub use child::{
    RunningChild, run_child, spawn_child, spawn_child_on_port, spawn_child_with_hints,
};
pub use error::RunnerError;
pub use ports::{AllocationHints, allocate_port, allocate_port_with_hints, is_port_available};

#[cfg(test)]
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
};

#[cfg(test)]
use bindport_core::PortRange;
#[cfg(test)]
use ports::{allocate_port_with_hints_and_availability, bind_error_leaves_port_available};

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::{
        fs,
        sync::{Mutex, MutexGuard},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    static SIGNAL_FORWARDING_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn allocate_port_skips_reserved_ports() {
        let range = PortRange {
            start: 29_000,
            end: 29_001,
        };

        assert_eq!(
            allocate_port_with_hints_and_availability(
                range,
                &[29_000],
                AllocationHints::default(),
                |_| true
            )
            .expect("port"),
            29_001
        );
    }

    #[test]
    fn allocate_port_prefers_available_prior_port() {
        let range = PortRange {
            start: 29_000,
            end: 29_002,
        };
        let hints = AllocationHints {
            preferred_port: Some(29_002),
            scan_start: None,
        };

        assert_eq!(
            allocate_port_with_hints_and_availability(range, &[], hints, |_| true).expect("port"),
            29_002
        );
    }

    #[test]
    fn allocate_port_scans_from_hint_with_wraparound() {
        let range = PortRange {
            start: 29_000,
            end: 29_003,
        };
        let hints = AllocationHints {
            preferred_port: None,
            scan_start: Some(29_002),
        };

        assert_eq!(
            allocate_port_with_hints_and_availability(
                range,
                &[29_002, 29_003, 29_000],
                hints,
                |_| { true }
            )
            .expect("port"),
            29_001
        );
    }

    #[test]
    fn allocate_port_reports_exhausted_range() {
        let range = PortRange {
            start: 29_000,
            end: 29_000,
        };

        let error = allocate_port_with_hints_and_availability(
            range,
            &[29_000],
            AllocationHints::default(),
            |_| true,
        )
        .expect_err("range should be exhausted");
        assert!(matches!(error, RunnerError::NoAvailablePort { range: _ }));
    }

    #[test]
    fn allocate_port_wrapper_uses_real_loopback_availability() {
        let listener =
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let port = listener.local_addr().expect("listener address").port();
        let range = PortRange {
            start: port,
            end: port,
        };

        assert!(!is_port_available(port));
        assert!(matches!(
            allocate_port(range, &[]),
            Err(RunnerError::NoAvailablePort { .. })
        ));

        assert!(matches!(
            spawn_child_on_port(&[], port, &[]),
            Err(RunnerError::NoCommand)
        ));
    }

    #[test]
    fn bind_errors_only_conflict_when_address_is_in_use() {
        assert!(!bind_error_leaves_port_available(io::ErrorKind::AddrInUse));
        assert!(bind_error_leaves_port_available(
            io::ErrorKind::AddrNotAvailable
        ));
        assert!(bind_error_leaves_port_available(io::ErrorKind::Unsupported));
    }

    #[test]
    fn runner_errors_format_user_facing_messages() {
        let errors = vec![
            RunnerError::NoCommand,
            RunnerError::NoAvailablePort {
                range: PortRange {
                    start: 29_000,
                    end: 29_010,
                },
            },
            RunnerError::SignalForwarding {
                source: io::Error::other("signals blocked"),
            },
            RunnerError::Spawn {
                command: String::from("missing-command"),
                source: io::Error::new(io::ErrorKind::NotFound, "not found"),
            },
            RunnerError::Wait {
                command: String::from("sleep"),
                source: io::Error::other("wait failed"),
            },
        ];

        let messages = errors
            .into_iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>();

        assert_eq!(messages[0], "no command provided after `--`");
        assert_eq!(messages[1], "no available port found in range 29000-29010");
        assert_eq!(
            messages[2],
            "failed to install signal forwarding: signals blocked"
        );
        assert_eq!(messages[3], "failed to spawn `missing-command`: not found");
        assert_eq!(messages[4], "failed waiting for `sleep`: wait failed");
    }

    #[cfg(unix)]
    #[test]
    fn signal_forwarding_rejects_concurrent_children_and_restores_handlers() {
        let _lock = signal_forwarding_test_lock();
        let before_int = current_signal_action(libc::SIGINT);
        let before_term = current_signal_action(libc::SIGTERM);
        let before_mask = current_signal_mask();
        let command = vec!["sleep".to_string(), "5".to_string()];
        let range = PortRange {
            start: 29_000,
            end: 29_010,
        };

        let mut first = spawn_child(&command, range, &[]).expect("first child");
        assert!((range.start..=range.end).contains(&first.port()));
        let error = match spawn_child(&command, range, &[]) {
            Ok(mut second) => {
                let _ = second.kill();
                let _ = second.wait();
                panic!("second child was not rejected");
            }
            Err(error) => error,
        };

        assert!(
            matches!(error, RunnerError::SignalForwarding { source } if source.kind() == io::ErrorKind::AlreadyExists)
        );

        first.kill().expect("kill first child");
        first.wait().expect("wait for first child");

        assert_signal_action_matches(libc::SIGINT, &before_int);
        assert_signal_action_matches(libc::SIGTERM, &before_term);
        assert_signal_mask_matches(libc::SIGINT, &before_mask);
        assert_signal_mask_matches(libc::SIGTERM, &before_mask);
    }

    #[cfg(unix)]
    #[test]
    fn dropping_running_child_kills_and_reaps_process() {
        let _lock = signal_forwarding_test_lock();
        let pid_path = temp_test_path("drop-child-pid");
        let pid_path_arg = pid_path.display().to_string();
        let command = vec![
            String::from("sh"),
            String::from("-c"),
            String::from("printf '%s' \"$$\" > \"$1\"; while :; do sleep 1; done"),
            String::from("bindport-drop-test"),
            pid_path_arg,
        ];

        let child_pid;
        {
            let child = spawn_child_on_port(&command, 29_000, &[]).expect("spawn child");
            assert!(child.pid() > 0);
            child_pid = read_pid_file(&pid_path);
        }

        for _ in 0..100 {
            if !test_process_is_running(child_pid) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("dropped child process {child_pid} is still running");
    }

    #[cfg(unix)]
    fn read_pid_file(path: &std::path::Path) -> u32 {
        for _ in 0..100 {
            if let Ok(contents) = fs::read_to_string(path)
                && let Ok(pid) = contents.trim().parse::<u32>()
            {
                return pid;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("pid file was not written: {}", path.display());
    }

    #[cfg(unix)]
    fn test_process_is_running(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || matches!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
    }

    #[cfg(unix)]
    fn temp_test_path(name: &str) -> std::path::PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "bindport-runner-{name}-{}-{now}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn signal_forwarding_test_lock() -> MutexGuard<'static, ()> {
        SIGNAL_FORWARDING_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(unix)]
    fn current_signal_action(signal: libc::c_int) -> libc::sigaction {
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        let result = unsafe { libc::sigaction(signal, std::ptr::null(), &mut action) };
        assert_eq!(result, 0, "read signal action for {signal}");
        action
    }

    #[cfg(unix)]
    fn assert_signal_action_matches(signal: libc::c_int, expected: &libc::sigaction) {
        let actual = current_signal_action(signal);
        assert_eq!(actual.sa_sigaction, expected.sa_sigaction);
    }

    #[cfg(unix)]
    fn current_signal_mask() -> libc::sigset_t {
        let mut mask = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        let result = unsafe { libc::sigprocmask(libc::SIG_BLOCK, std::ptr::null(), &mut mask) };
        assert_eq!(result, 0, "read signal mask");
        mask
    }

    #[cfg(unix)]
    fn assert_signal_mask_matches(signal: libc::c_int, expected: &libc::sigset_t) {
        let actual = current_signal_mask();
        let actual_member = unsafe { libc::sigismember(&actual, signal) };
        let expected_member = unsafe { libc::sigismember(expected, signal) };

        assert_eq!(actual_member, expected_member);
    }
}
