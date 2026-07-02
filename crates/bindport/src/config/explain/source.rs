use super::*;

pub(crate) fn print_config_source_explanation(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            print_config_local_override(loaded);
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }

    if let Some(loaded) = config.loaded.as_ref()
        && !loaded.unknown_keys.is_empty()
    {
        println!(
            "config warning: ignored unknown top-level keys: {}",
            loaded.unknown_keys.join(", ")
        );
        println!("config applied keys: {}", APPLIED_CONFIG_KEYS.join(", "));
    }
}

pub(crate) fn print_config_local_override(loaded: &LoadedConfig) {
    let Some(local) = loaded.local_override.as_ref() else {
        return;
    };

    println!(
        "config local override: {} ({} {})",
        local.path.display(),
        loaded.source.as_str(),
        local.format.as_str()
    );
    if local.git_tracked {
        println!(
            "config warning: local override is tracked by git; keep `.bindport.local.*` and `bindport.local.*` untracked for machine-local values"
        );
    }
}
