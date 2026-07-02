use super::*;

pub(crate) fn init_fallback_config() -> ExitCode {
    match write_fallback_config() {
        Ok(InitConfigResult::Created(path)) => {
            println!("created config: {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(InitConfigResult::AlreadyExists(path)) => {
            println!("config already exists: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bindport: failed to initialize fallback config: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) enum InitConfigResult {
    Created(PathBuf),
    AlreadyExists(PathBuf),
}

pub(crate) fn write_fallback_config() -> io::Result<InitConfigResult> {
    let path = fallback_config_path()?;

    if path.is_file() {
        return Ok(InitConfigResult::AlreadyExists(path));
    }

    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("`{}` exists but is not a file", path.display()),
        ));
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, default_fallback_config())?;

    Ok(InitConfigResult::Created(path))
}
