// SPDX-License-Identifier: MIT

use std::{
    collections::HashSet,
    fmt, io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener},
    process::{Child, Command, ExitStatus, Stdio},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
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

fn allocate_port_with_hints_and_availability(
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
    spawn_child_with_hints(command, range, skip_ports, AllocationHints::default())
}

pub fn spawn_child_with_hints(
    command: &[String],
    range: PortRange,
    skip_ports: &[u16],
    allocation_hints: AllocationHints,
) -> Result<RunningChild, RunnerError> {
    let port = allocate_port_with_hints(range, skip_ports, allocation_hints)?;

    spawn_child_on_port(command, port, &[])
}

pub fn spawn_child_on_port(
    command: &[String],
    port: u16,
    extra_env: &[(String, String)],
) -> Result<RunningChild, RunnerError> {
    let (program, args) = command.split_first().ok_or(RunnerError::NoCommand)?;

    let mut signal_forwarding =
        prepare_signal_forwarding().map_err(|source| RunnerError::SignalForwarding { source })?;

    let mut process = Command::new(program);
    process
        .args(args)
        .env(PORT_ENV_VAR, port.to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    process.envs(extra_env.iter().map(|(name, value)| (name, value)));
    prepare_child_signal_mask(&mut process, &signal_forwarding);

    let child = match process.spawn() {
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
    let child = match signal_forwarding.activate_for_pid(child.id()) {
        Ok(()) => child,
        Err(source) => {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            let _ = signal_forwarding.deactivate();

            return Err(RunnerError::SignalForwarding { source });
        }
    };

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
    saved_signal_mask: Option<libc::sigset_t>,
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
    fn activate_for_pid(&mut self, pid: u32) -> io::Result<()> {
        FORWARDED_CHILD_PID.store(pid as i32, Ordering::SeqCst);
        self.restore_signal_mask()
    }

    fn deactivate(&mut self) -> io::Result<()> {
        FORWARDED_CHILD_PID.store(0, Ordering::SeqCst);
        let signal_mask = self.restore_signal_mask();

        let handlers = if let Some(saved_handlers) = self.saved_handlers.take() {
            restore_signal_forwarding_handlers(&saved_handlers)
        } else {
            Ok(())
        };

        signal_mask.and(handlers)
    }

    fn restore_signal_mask(&mut self) -> io::Result<()> {
        if let Some(saved_signal_mask) = self.saved_signal_mask.as_ref() {
            restore_signal_mask(saved_signal_mask)?;
            self.saved_signal_mask = None;
        }

        Ok(())
    }
}

#[cfg(not(unix))]
impl SignalForwardingState {
    fn activate_for_pid(&mut self, _pid: u32) -> io::Result<()> {
        Ok(())
    }

    fn deactivate(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
fn prepare_signal_forwarding() -> io::Result<SignalForwardingState> {
    reserve_signal_forwarding()?;
    let saved_signal_mask = match block_signal_forwarding_signals() {
        Ok(saved_signal_mask) => saved_signal_mask,
        Err(error) => {
            FORWARDED_CHILD_PID.store(0, Ordering::SeqCst);
            return Err(error);
        }
    };

    match install_signal_forwarding_handlers() {
        Ok(saved_handlers) => Ok(SignalForwardingState {
            saved_handlers: Some(saved_handlers),
            saved_signal_mask: Some(saved_signal_mask),
        }),
        Err(error) => {
            FORWARDED_CHILD_PID.store(0, Ordering::SeqCst);
            let _ = restore_signal_mask(&saved_signal_mask);
            Err(error)
        }
    }
}

#[cfg(not(unix))]
fn prepare_signal_forwarding() -> io::Result<SignalForwardingState> {
    Ok(SignalForwardingState)
}

#[cfg(unix)]
fn prepare_child_signal_mask(command: &mut Command, signal_forwarding: &SignalForwardingState) {
    if let Some(saved_signal_mask) = signal_forwarding.saved_signal_mask {
        unsafe {
            command.pre_exec(move || restore_signal_mask(&saved_signal_mask));
        }
    }
}

#[cfg(not(unix))]
fn prepare_child_signal_mask(_command: &mut Command, _signal_forwarding: &SignalForwardingState) {}

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
fn block_signal_forwarding_signals() -> io::Result<libc::sigset_t> {
    let mut mask = unsafe { std::mem::zeroed::<libc::sigset_t>() };
    let mut previous = unsafe { std::mem::zeroed::<libc::sigset_t>() };

    if unsafe { libc::sigemptyset(&mut mask) } == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::sigaddset(&mut mask, libc::SIGINT) } == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::sigaddset(&mut mask, libc::SIGTERM) } == -1 {
        return Err(io::Error::last_os_error());
    }

    let result = unsafe { libc::sigprocmask(libc::SIG_BLOCK, &mask, &mut previous) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(previous)
}

#[cfg(unix)]
fn restore_signal_mask(mask: &libc::sigset_t) -> io::Result<()> {
    let result = unsafe { libc::sigprocmask(libc::SIG_SETMASK, mask, std::ptr::null_mut()) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
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
                |_| true
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
        let before_mask = current_signal_mask();
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
        assert_signal_mask_matches(libc::SIGINT, &before_mask);
        assert_signal_mask_matches(libc::SIGTERM, &before_mask);
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
