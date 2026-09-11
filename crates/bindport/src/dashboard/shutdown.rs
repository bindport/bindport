use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

static REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) struct DashboardShutdown {
    sigint: libc::sigaction,
    sigterm: libc::sigaction,
}

impl DashboardShutdown {
    pub(super) fn install() -> io::Result<Self> {
        REQUESTED.store(false, Ordering::Relaxed);
        let sigint = install_handler(libc::SIGINT)?;
        match install_handler(libc::SIGTERM) {
            Ok(sigterm) => Ok(Self { sigint, sigterm }),
            Err(error) => {
                restore_handler(libc::SIGINT, &sigint);
                Err(error)
            }
        }
    }

    pub(super) fn requested(&self) -> bool {
        REQUESTED.load(Ordering::Relaxed)
    }
}

impl Drop for DashboardShutdown {
    fn drop(&mut self) {
        restore_handler(libc::SIGTERM, &self.sigterm);
        restore_handler(libc::SIGINT, &self.sigint);
    }
}

extern "C" fn request_shutdown(_signal: libc::c_int) {
    REQUESTED.store(true, Ordering::Relaxed);
}

fn install_handler(signal: libc::c_int) -> io::Result<libc::sigaction> {
    // SAFETY: both actions are initialized storage; the handler only stores a
    // lock-free atomic flag and leaves registry work to the serving thread.
    unsafe {
        let mut action = std::mem::zeroed::<libc::sigaction>();
        let mut previous = std::mem::zeroed::<libc::sigaction>();
        action.sa_sigaction = request_shutdown as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(signal, &action, &mut previous) == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(previous)
    }
}

fn restore_handler(signal: libc::c_int, previous: &libc::sigaction) {
    // SAFETY: previous was obtained from sigaction for this signal.
    unsafe {
        libc::sigaction(signal, previous, std::ptr::null_mut());
    }
}
