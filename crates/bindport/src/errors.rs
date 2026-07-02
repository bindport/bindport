use super::*;

pub(crate) fn open_optional_registry() -> Option<Registry> {
    match Registry::open_default() {
        Ok(registry) => Some(registry),
        Err(error) => {
            print_registry_warning("registry unavailable", &error);
            registry_disabled_warning();
            None
        }
    }
}

pub(crate) fn print_runner_error(error: &RunnerError) {
    eprintln!("bindport: {error}");
}

pub(crate) fn print_config_error(error: &ConfigError) {
    eprintln!("bindport: {error}");
}

pub(crate) fn print_registry_error(error: &RegistryError) {
    eprintln!("bindport: {error}");
    eprintln!("bindport: set {REGISTRY_PATH_ENV} to override the registry path");
}

pub(crate) fn print_registry_warning(context: &str, error: &RegistryError) {
    eprintln!("bindport: warning: {context}: {error}");
}

pub(crate) fn print_auto_render_warning(context: &str, error: &RenderCommandError) {
    eprintln!("bindport: warning: output auto-render failed after {context}: {error}");
}

pub(crate) fn registry_disabled_warning() {
    eprintln!(
        "bindport: warning: running without registry recording; set {REGISTRY_PATH_ENV} to restore it"
    );
}
