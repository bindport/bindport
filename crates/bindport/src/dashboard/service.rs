use super::*;

pub(crate) fn start_dashboard_service(
    options: &DashboardCliOptions,
) -> Result<(), DashboardCommandError> {
    if let Some(state) = read_dashboard_state()? {
        if dashboard_process_is_running(&state) {
            println!("dashboard running: {} pid {}", state.url, state.pid);
            return Ok(());
        }
        remove_dashboard_state().ok();
    }

    let stderr = open_dashboard_log()?;
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("dashboard")
        .arg("serve")
        .args(&options.serve_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr));

    if let Some(token) = options.token.as_ref() {
        command.env(options.token_env_name(), token);
    }

    let mut child = command.spawn()?;
    let pid = child.id();
    let process_start_time = process_start_time(pid);
    let stdout = child.stdout.take().ok_or_else(|| {
        DashboardCommandError::Io(io::Error::other("failed to capture dashboard stdout"))
    })?;
    let mut stdout = io::BufReader::new(stdout);
    let mut line = String::new();
    stdout.read_line(&mut line)?;
    let url = match line.trim().strip_prefix("dashboard: ") {
        Some(url) => url.to_string(),
        None => return Err(DashboardCommandError::Io(dashboard_start_error())),
    };
    let state = DashboardServiceState {
        pid,
        url,
        process_start_time,
    };
    write_dashboard_state(&state)?;
    println!("dashboard started: {} pid {}", state.url, state.pid);

    Ok(())
}

pub(crate) fn dashboard_start_error() -> io::Error {
    let message = dashboard_log_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let message = message.trim();

    if message.is_empty() {
        return io::Error::other("dashboard did not start");
    }

    io::Error::other(format!(
        "dashboard did not start: {}",
        message.chars().take(500).collect::<String>()
    ))
}

pub(crate) fn print_dashboard_service_status() -> Result<(), DashboardCommandError> {
    let Some(state) = read_dashboard_state()? else {
        println!("dashboard stopped");
        return Ok(());
    };

    if dashboard_process_is_running(&state) {
        println!("dashboard running: {} pid {}", state.url, state.pid);
    } else if process_is_running(state.pid) {
        println!(
            "dashboard stale: pid {} no longer matches dashboard",
            state.pid
        );
    } else {
        println!("dashboard stale: {} pid {}", state.url, state.pid);
    }

    Ok(())
}

pub(crate) fn stop_dashboard_service() -> Result<(), DashboardCommandError> {
    let Some(state) = read_dashboard_state()? else {
        println!("dashboard stopped");
        return Ok(());
    };

    if dashboard_process_is_running(&state) {
        terminate_process(state.pid)?;
        println!("dashboard stopped: pid {}", state.pid);
    } else if process_is_running(state.pid) {
        println!(
            "dashboard state removed: pid {} no longer matches dashboard",
            state.pid
        );
    } else {
        println!("dashboard state removed: stale pid {}", state.pid);
    }
    remove_dashboard_state()?;

    Ok(())
}
