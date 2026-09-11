use super::*;

pub(super) fn lock_dashboard_service() -> io::Result<fs::File> {
    let path = create_dashboard_state_dir()?.join("dashboard.lock");
    // Keep the inode stable so existing waiters and new callers share one lock.
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}
