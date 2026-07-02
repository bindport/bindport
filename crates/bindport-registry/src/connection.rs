use super::*;

pub fn default_registry_directory_name() -> &'static str {
    SERVICE_NAME
}

pub fn default_registry_path() -> Result<PathBuf, RegistryError> {
    if let Some(path) = env::var_os(REGISTRY_PATH_ENV).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(state_home)
            .join(default_registry_directory_name())
            .join(DEFAULT_REGISTRY_FILE));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join(default_registry_directory_name())
            .join(DEFAULT_REGISTRY_FILE));
    }

    if let Some(appdata) = env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(appdata)
            .join(default_registry_directory_name())
            .join(DEFAULT_REGISTRY_FILE));
    }

    Err(RegistryError::MissingStateDirectory)
}

pub(crate) fn reject_registry_symlink(path: &Path) -> Result<(), RegistryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(RegistryError::UnsafePath {
            path: path.to_path_buf(),
            message: "registry database must not be a symlink",
        }),
        Ok(_) | Err(_) => Ok(()),
    }
}

#[cfg(unix)]
pub(crate) fn harden_registry_directory(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub(crate) fn harden_registry_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn harden_registry_file(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub(crate) fn harden_registry_file(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub struct Registry {
    pub(crate) connection: Connection,
    pub(crate) path: PathBuf,
}

impl Registry {
    pub fn open_default() -> Result<Self, RegistryError> {
        Self::open(default_registry_path()?)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, RegistryError> {
        let path = path.into();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let should_harden_parent = !parent.exists()
                || parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == default_registry_directory_name());
            fs::create_dir_all(parent).map_err(|source| RegistryError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
            if should_harden_parent {
                harden_registry_directory(parent).map_err(|source| {
                    RegistryError::CreateDirectory {
                        path: parent.to_path_buf(),
                        source,
                    }
                })?;
            }
        }

        reject_registry_symlink(&path)?;
        let connection = Connection::open(&path).map_err(|source| RegistryError::Open {
            path: path.clone(),
            source,
        })?;
        connection.busy_timeout(REGISTRY_BUSY_TIMEOUT)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        harden_registry_file(&path).map_err(|source| RegistryError::CreateDirectory {
            path: path.clone(),
            source,
        })?;
        let registry = Self { connection, path };
        registry.ensure_schema()?;

        Ok(registry)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
