use super::*;

pub(super) const BACKGROUND_ENV: &str = "__BINDPORT_DASHBOARD_BACKGROUND";

pub(super) fn redirect_background_stdout() -> io::Result<()> {
    if env::var(BACKGROUND_ENV).as_deref() != Ok("1") {
        return Ok(());
    }
    // The startup line has been flushed to the parent. Future writes, including
    // inherited hook stdout, must use the log rather than its closing pipe.
    if unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
