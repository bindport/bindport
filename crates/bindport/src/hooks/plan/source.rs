use super::*;

pub(crate) fn hook_command_source(config: &ResolvedConfig) -> String {
    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("unknown config");
    };

    if let Some(local) = loaded.local_override.as_ref()
        && local
            .config
            .hooks
            .as_ref()
            .and_then(|hooks| hooks.commands.as_ref())
            .is_some()
    {
        return format!("local override config `{}`", local.path.display());
    }

    format!(
        "{} config `{}`",
        loaded.source.as_str(),
        loaded.path.display()
    )
}
