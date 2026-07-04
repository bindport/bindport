use super::*;

pub(crate) fn active_run_process_matches(run: &ActiveRun) -> bool {
    match run.process_start_time {
        Some(expected) => process_start_time(run.pid) == Some(expected),
        None => {
            process_is_running(run.pid)
                && process_command_line_matches(run.pid, run.command.as_str()).unwrap_or(true)
        }
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

pub(crate) fn process_command_line_matches(pid: u32, expected_command: &str) -> Option<bool> {
    let command_line = process_command_line(pid)?;
    Some(command_line_contains_recorded_command(
        command_line.as_str(),
        expected_command,
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn process_command_line(pid: u32) -> Option<String> {
    let bytes = fs::read(Path::new("/proc").join(pid.to_string()).join("cmdline")).ok()?;
    let command = bytes
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg))
        .collect::<Vec<_>>()
        .join(" ");
    (!command.is_empty()).then_some(command)
}

#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn process_command_line(pid: u32) -> Option<String> {
    let pid = pid.to_string();
    let output = std::process::Command::new("ps")
        .args(["-p", pid.as_str(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!command.is_empty()).then_some(command)
}

#[cfg(not(unix))]
pub(crate) fn process_command_line(_pid: u32) -> Option<String> {
    None
}

pub(crate) fn command_line_contains_recorded_command(
    command_line: &str,
    expected_command: &str,
) -> bool {
    let command_line = normalize_command_text(command_line);
    let expected_command = normalize_command_text(expected_command);
    !expected_command.is_empty() && command_line.contains(expected_command.as_str())
}

fn normalize_command_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
