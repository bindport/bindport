use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DashboardServiceState {
    pub(crate) pid: u32,
    pub(crate) url: String,
    pub(crate) process_start_time: Option<u64>,
}

pub(crate) fn read_dashboard_state() -> Result<Option<DashboardServiceState>, DashboardCommandError>
{
    let path = dashboard_state_path()?;
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)?;
    let mut pid = None;
    let mut url = None;
    let mut process_start_time = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("pid=") {
            pid = value.trim().parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("url=") {
            url = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("process_start_time=") {
            process_start_time = value.trim().parse::<u64>().ok();
        }
    }

    Ok(pid.zip(url).map(|(pid, url)| DashboardServiceState {
        pid,
        url,
        process_start_time,
    }))
}

pub(crate) fn write_dashboard_state(
    state: &DashboardServiceState,
) -> Result<(), DashboardCommandError> {
    let path = dashboard_state_path()?;
    create_dashboard_state_dir()?;
    let mut contents = format!("pid={}\nurl={}\n", state.pid, state.url);
    if let Some(process_start_time) = state.process_start_time {
        contents.push_str(&format!("process_start_time={process_start_time}\n"));
    }
    fs::write(path, contents)?;
    Ok(())
}

pub(crate) fn remove_dashboard_state() -> io::Result<()> {
    match fs::remove_file(dashboard_state_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn dashboard_process_is_running(state: &DashboardServiceState) -> bool {
    process_is_running(state.pid) && dashboard_process_matches_state(state)
}

#[cfg(target_os = "linux")]
pub(crate) fn dashboard_process_matches_state(state: &DashboardServiceState) -> bool {
    match state.process_start_time {
        Some(expected) => {
            process_start_time(state.pid) == Some(expected)
                && process_cmdline_is_dashboard(state.pid)
        }
        None => process_cmdline_is_dashboard(state.pid),
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn dashboard_process_matches_state(_state: &DashboardServiceState) -> bool {
    // Non-Linux targets do not have the /proc fields used above. Dashboard stop
    // falls back to PID liveness there, which can be fooled by PID reuse.
    true
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(19)?.parse::<u64>().ok()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn process_start_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
pub(crate) fn process_cmdline_is_dashboard(pid: u32) -> bool {
    let Ok(cmdline) = fs::read(Path::new("/proc").join(pid.to_string()).join("cmdline")) else {
        return false;
    };
    let args = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .collect::<Vec<_>>();

    args.windows(2)
        .any(|window| window[0] == b"dashboard" && window[1] == b"serve")
}

#[cfg(unix)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
pub(crate) fn process_is_running(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn terminate_process(pid: u32) -> io::Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn dashboard_clean_callback(
    cwd: PathBuf,
    config: ResolvedConfig,
) -> DashboardCleanCallback {
    Arc::new(move |registry, _summary| {
        let events = RouteEventCollector::single(
            RouteEventSource::DashboardClean,
            RouteEventKind::RoutesRemoved,
        );

        auto_render_outputs_for_events(&cwd, &config, registry, &events)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn dashboard_status_callback(cwd: PathBuf) -> DashboardStatusCallback {
    Arc::new(move || match resolve_config(&cwd) {
        Ok(config) => hooks_status_json(&cwd, &config),
        Err(error) => serde_json::json!({
            "error": error.to_string(),
            "items": [],
        }),
    })
}

#[cfg(not(unix))]
pub(crate) fn terminate_process(_pid: u32) -> io::Result<()> {
    Err(io::Error::other(
        "dashboard stop is not implemented on this platform",
    ))
}
