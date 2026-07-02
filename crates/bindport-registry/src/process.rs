use super::*;

pub(crate) fn active_run_process_matches(run: &ActiveRun) -> bool {
    match run.process_start_time {
        Some(expected) => process_start_time(run.pid) == Some(expected),
        None => process_is_running(run.pid),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start_time(pid: u32) -> Option<i64> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(19)?.parse::<i64>().ok()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn process_start_time(_pid: u32) -> Option<i64> {
    None
}

#[cfg(unix)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };

    if result == 0 {
        return true;
    }

    matches!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
}

#[cfg(not(unix))]
pub(crate) fn process_is_running(_pid: u32) -> bool {
    true
}
