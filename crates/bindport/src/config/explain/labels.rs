use super::*;

pub(crate) fn services_config_label(loaded: &LoadedConfig) -> &'static str {
    if local_config(loaded)
        .and_then(|local| local.services.as_ref())
        .is_some()
    {
        "local override config"
    } else {
        source_config_label(loaded.source)
    }
}

pub(crate) fn source_config_label(source: ConfigSource) -> &'static str {
    match source {
        ConfigSource::Project => "project config",
        ConfigSource::Fallback => "fallback config",
    }
}

pub(crate) fn non_empty_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
