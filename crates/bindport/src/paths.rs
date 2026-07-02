use super::*;

pub(crate) fn fallback_config_path() -> io::Result<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(config_home)
            .join(SERVICE_NAME)
            .join(FALLBACK_CONFIG_FILE));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join(SERVICE_NAME)
            .join(FALLBACK_CONFIG_FILE));
    }

    if let Some(appdata) = env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(appdata)
            .join(SERVICE_NAME)
            .join(FALLBACK_CONFIG_FILE));
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not determine config directory; set XDG_CONFIG_HOME, HOME, or APPDATA",
    ))
}
pub(crate) fn dashboard_state_path() -> io::Result<PathBuf> {
    Ok(dashboard_state_dir()?.join(DASHBOARD_STATE_FILE))
}

pub(crate) fn dashboard_log_path() -> io::Result<PathBuf> {
    Ok(dashboard_state_dir()?.join(DASHBOARD_LOG_FILE))
}

pub(crate) fn hook_trust_path() -> io::Result<PathBuf> {
    Ok(bindport_state_dir()?.join(HOOK_TRUST_FILE))
}

pub(crate) fn create_dashboard_state_dir() -> io::Result<PathBuf> {
    let path = dashboard_state_dir()?;
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub(crate) fn open_dashboard_log() -> io::Result<fs::File> {
    let path = create_dashboard_state_dir()?.join(DASHBOARD_LOG_FILE);
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

pub(crate) fn dashboard_state_dir() -> io::Result<PathBuf> {
    bindport_state_dir()
}

pub(crate) fn bindport_state_dir() -> io::Result<PathBuf> {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(state_home).join(SERVICE_NAME));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join(SERVICE_NAME));
    }

    if let Some(appdata) = env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(appdata).join(SERVICE_NAME));
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not determine state directory; set XDG_STATE_HOME, HOME, or APPDATA",
    ))
}
