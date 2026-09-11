use super::*;

mod lock;
mod start;

use lock::lock_dashboard_service;
pub(crate) use start::*;

pub(crate) fn print_dashboard_service_status() -> Result<(), DashboardCommandError> {
    let _lock = lock_dashboard_service()?;
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
    let _lock = lock_dashboard_service()?;
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
