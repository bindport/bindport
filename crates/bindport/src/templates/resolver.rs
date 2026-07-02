use super::*;

pub(crate) fn template_resolver(cwd: &Path) -> Result<TemplateResolver, ConfigError> {
    let config = resolve_config(cwd)?;

    Ok(TemplateResolver::new(
        Some(project_template_dir(cwd, &config)),
        global_template_dir(),
    ))
}

pub(crate) fn project_template_dir(cwd: &Path, config: &ResolvedConfig) -> PathBuf {
    config
        .loaded
        .as_ref()
        .filter(|loaded| loaded.source == ConfigSource::Project)
        .and_then(|loaded| loaded.path.parent())
        .unwrap_or(cwd)
        .join(".bindport")
        .join("templates")
}

pub(crate) fn global_template_dir() -> Option<PathBuf> {
    fallback_config_path()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("templates")))
}
