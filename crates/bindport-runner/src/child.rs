// SPDX-License-Identifier: MIT

use std::{
    io,
    process::{Child, Command, ExitStatus, Stdio},
};

use bindport_core::PortRange;

use crate::{
    AllocationHints, PORT_ENV_VAR, RunnerError, allocate_port_with_hints,
    signals::{SignalForwardingState, prepare_child_signal_mask, prepare_signal_forwarding},
};

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
        let pre_reap = wait_until_child_exits_without_reaping(self.child.id());
        let signal_forwarding = self
            .signal_forwarding
            .deactivate()
            .map_err(|source| RunnerError::SignalForwarding { source });
        let status = match pre_reap {
            Ok(()) => self.child.wait().map_err(|source| RunnerError::Wait {
                command: self.program.clone(),
                source,
            }),
            Err(source) => Err(RunnerError::Wait {
                command: self.program.clone(),
                source,
            }),
        };

        match (status, signal_forwarding) {
            (Ok(status), Ok(())) => Ok(status),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

#[cfg(unix)]
fn wait_until_child_exits_without_reaping(pid: u32) -> io::Result<()> {
    loop {
        let mut siginfo = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                siginfo.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(());
        }

        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(not(unix))]
fn wait_until_child_exits_without_reaping(_pid: u32) -> io::Result<()> {
    Ok(())
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.signal_forwarding.deactivate();
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
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
