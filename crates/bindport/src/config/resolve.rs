use super::*;

pub(crate) struct ResolvedConfig {
    pub(crate) loaded: Option<LoadedConfig>,
    pub(crate) fallback_path: Option<PathBuf>,
    pub(crate) port_range: PortRange,
    pub(crate) skip_ports: Vec<u16>,
}

pub(crate) fn resolve_config(cwd: &Path) -> Result<ResolvedConfig, ConfigError> {
    let fallback_path = fallback_config_path().ok();
    let loaded = discover_config(cwd, fallback_path.as_deref())?;
    let port_range = loaded
        .as_ref()
        .map(LoadedConfig::port_range)
        .transpose()?
        .unwrap_or(DEFAULT_PORT_RANGE);
    let skip_ports = loaded
        .as_ref()
        .map(LoadedConfig::skip_ports)
        .unwrap_or_else(|| DEFAULT_SKIP_PORTS.to_vec());

    Ok(ResolvedConfig {
        loaded,
        fallback_path,
        port_range,
        skip_ports,
    })
}
