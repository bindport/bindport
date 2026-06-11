// SPDX-License-Identifier: MIT

use std::{
    collections::HashSet,
    fmt, io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener},
    process::{Child, Command, ExitStatus, Stdio},
};

#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

use bindport_core::PortRange;

pub const PORT_ENV_VAR: &str = "PORT";

#[cfg(unix)]
static FORWARDED_CHILD_PID: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
const RESERVED_CHILD_PID: i32 = -1;

#[derive(Debug)]
pub enum RunnerError {
    NoCommand,
    NoAvailablePort { range: PortRange },
    SignalForwarding { source: io::Error },
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
            Self::SignalForwarding { source } => {
                write!(f, "failed to install signal forwarding: {source}")
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

pub struct RunningChild {
    child: Child,
    port: u16,
    program: String,
    signal_forwarding: SignalForwardingState,
}

impl RunningChild {
    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    pub fn wait(&mut self) -> Result<ExitStatus, RunnerError> {
        let status = self.child.wait().map_err(|source| RunnerError::Wait {
            command: self.program.clone(),
            source,
        });
        let signal_forwarding = self
            .signal_forwarding
            .deactivate()
            .map_err(|source| RunnerError::SignalForwarding { source });

        match (status, signal_forwarding) {
            (Ok(status), Ok(())) => Ok(status),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.signal_forwarding.deactivate();
    }
}

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
    let mut child = spawn_child(command, range, skip_ports)?;

    child.wait()
}

/// Spawns a wrapped command with the selected port in its environment.
///
/// On Unix, SIGINT/SIGTERM forwarding uses process-global signal handlers while
/// the returned child is active. A second concurrent forwarded child is rejected.
pub fn spawn_child(
    command: &[String],
    range: PortRange,
    skip_ports: &[u16],
) -> Result<RunningChild, RunnerError> {
    let (program, args) = command.split_first().ok_or(RunnerError::NoCommand)?;
    let port = allocate_port(range, skip_ports)?;

    let mut signal_forwarding =
        prepare_signal_forwarding().map_err(|source| RunnerError::SignalForwarding { source })?;

    let child = match Command::new(program)
        .args(args)
        .env(PORT_ENV_VAR, port.to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(source) => {
            if let Err(source) = signal_forwarding.deactivate() {
                return Err(RunnerError::SignalForwarding { source });
            }

            return Err(RunnerError::Spawn {
                command: program.clone(),
                source,
            });
        }
    };
    activate_signal_forwarding_for_pid(child.id());

    Ok(RunningChild {
        child,
        port,
        program: program.clone(),
        signal_forwarding,
    })
}

#[cfg(unix)]
struct SignalForwardingState {
    saved_handlers: Option<SavedSignalHandlers>,
}

#[cfg(not(unix))]
struct SignalForwardingState;

#[cfg(unix)]
struct SavedSignalHandlers {
    sigint: libc::sigaction,
    sigterm: libc::sigaction,
}

#[cfg(unix)]
impl SignalForwardingState {
    fn deactivate(&mut self) -> io::Result<()> {
        FORWARDED_CHILD_PID.store(0, Ordering::SeqCst);

        if let Some(saved_handlers) = self.saved_handlers.take() {
            restore_signal_forwarding_handlers(&saved_handlers)?;
        }

        Ok(())
    }
}

#[cfg(not(unix))]
impl SignalForwardingState {
    fn deactivate(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
fn prepare_signal_forwarding() -> io::Result<SignalForwardingState> {
    reserve_signal_forwarding()?;

    match install_signal_forwarding_handlers() {
        Ok(saved_handlers) => Ok(SignalForwardingState {
            saved_handlers: Some(saved_handlers),
        }),
        Err(error) => {
            FORWARDED_CHILD_PID.store(0, Ordering::SeqCst);
            Err(error)
        }
    }
}

#[cfg(not(unix))]
fn prepare_signal_forwarding() -> io::Result<SignalForwardingState> {
    Ok(SignalForwardingState)
}

#[cfg(unix)]
fn reserve_signal_forwarding() -> io::Result<()> {
    FORWARDED_CHILD_PID
        .compare_exchange(0, RESERVED_CHILD_PID, Ordering::SeqCst, Ordering::SeqCst)
        .map(|_| ())
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "signal forwarding is already active",
            )
        })
}

#[cfg(unix)]
fn activate_signal_forwarding_for_pid(pid: u32) {
    FORWARDED_CHILD_PID.store(pid as i32, Ordering::SeqCst);
}

#[cfg(not(unix))]
fn activate_signal_forwarding_for_pid(_pid: u32) {}

#[cfg(unix)]
fn install_signal_forwarding_handlers() -> io::Result<SavedSignalHandlers> {
    let sigint = install_signal_forwarding_handler(libc::SIGINT)?;

    match install_signal_forwarding_handler(libc::SIGTERM) {
        Ok(sigterm) => Ok(SavedSignalHandlers { sigint, sigterm }),
        Err(error) => {
            let _ = restore_signal_handler(libc::SIGINT, &sigint);
            Err(error)
        }
    }
}

#[cfg(unix)]
fn install_signal_forwarding_handler(signal: libc::c_int) -> io::Result<libc::sigaction> {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = forward_signal_to_child as *const () as usize;
    action.sa_flags = 0;

    let mask_result = unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if mask_result == -1 {
        return Err(io::Error::last_os_error());
    }

    let action_result = unsafe { libc::sigaction(signal, &action, &mut previous) };
    if action_result == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(previous)
}

#[cfg(unix)]
fn restore_signal_forwarding_handlers(saved_handlers: &SavedSignalHandlers) -> io::Result<()> {
    restore_signal_handler(libc::SIGINT, &saved_handlers.sigint)?;
    restore_signal_handler(libc::SIGTERM, &saved_handlers.sigterm)
}

#[cfg(unix)]
fn restore_signal_handler(signal: libc::c_int, action: &libc::sigaction) -> io::Result<()> {
    let result = unsafe { libc::sigaction(signal, action, std::ptr::null_mut()) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(unix)]
extern "C" fn forward_signal_to_child(signal: libc::c_int) {
    let pid = FORWARDED_CHILD_PID.load(Ordering::SeqCst);

    if pid > 0 {
        unsafe {
            libc::kill(pid, signal);
        }
    }
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

    #[cfg(unix)]
    #[test]
    fn signal_forwarding_rejects_concurrent_children_and_restores_handlers() {
        let before_int = current_signal_action(libc::SIGINT);
        let before_term = current_signal_action(libc::SIGTERM);
        let command = vec!["sleep".to_string(), "5".to_string()];
        let range = PortRange {
            start: 29_000,
            end: 29_010,
        };

        let mut first = spawn_child(&command, range, &[]).expect("first child");
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
}
