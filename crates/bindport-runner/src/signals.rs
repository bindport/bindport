// SPDX-License-Identifier: MIT

use std::{io, process::Command};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(unix)]
static FORWARDED_CHILD_PID: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
const RESERVED_CHILD_PID: i32 = -1;

#[cfg(unix)]
pub(crate) struct SignalForwardingState {
    saved_handlers: Option<SavedSignalHandlers>,
    saved_signal_mask: Option<libc::sigset_t>,
}

#[cfg(not(unix))]
pub(crate) struct SignalForwardingState;

#[cfg(unix)]
struct SavedSignalHandlers {
    sigint: libc::sigaction,
    sigterm: libc::sigaction,
}

#[cfg(unix)]
impl SignalForwardingState {
    pub(crate) fn activate_for_pid(&mut self, pid: u32) -> io::Result<()> {
        FORWARDED_CHILD_PID.store(pid as i32, Ordering::SeqCst);
        self.restore_signal_mask()
    }

    pub(crate) fn deactivate(&mut self) -> io::Result<()> {
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
    pub(crate) fn activate_for_pid(&mut self, _pid: u32) -> io::Result<()> {
        Ok(())
    }

    pub(crate) fn deactivate(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn prepare_signal_forwarding() -> io::Result<SignalForwardingState> {
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
pub(crate) fn prepare_signal_forwarding() -> io::Result<SignalForwardingState> {
    Ok(SignalForwardingState)
}

#[cfg(unix)]
pub(crate) fn prepare_child_signal_mask(
    command: &mut Command,
    signal_forwarding: &SignalForwardingState,
) {
    if let Some(saved_signal_mask) = signal_forwarding.saved_signal_mask {
        unsafe {
            command.pre_exec(move || restore_signal_mask(&saved_signal_mask));
        }
    }
}

#[cfg(not(unix))]
pub(crate) fn prepare_child_signal_mask(
    _command: &mut Command,
    _signal_forwarding: &SignalForwardingState,
) {
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
