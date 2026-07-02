use super::*;

pub(crate) fn print_config_field_explanations(config: &ResolvedConfig) {
    println!("fields:");

    match config.loaded.as_ref() {
        Some(loaded) => {
            print_config_field(
                "project",
                optional_config_value(loaded.config.project.as_deref()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.project.as_deref()),
                    loaded.config.project.as_deref(),
                ),
            );
            print_config_field(
                "service",
                optional_config_value(loaded.config.service.as_deref()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.service.as_deref()),
                    loaded.config.service.as_deref(),
                ),
            );
            print_config_field(
                "default_range",
                format!("{}-{}", config.port_range.start, config.port_range.end),
                defaulted_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.default_range.as_deref()),
                    loaded.config.default_range.as_deref(),
                ),
            );
            print_config_field(
                "skip_ports",
                format!("{} ports", config.skip_ports.len()),
                defaulted_vec_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.skip_ports.as_ref()),
                    loaded.config.skip_ports.as_ref(),
                ),
            );
            print_config_field(
                "services",
                list_config_value(loaded.config.services.as_ref().map(Vec::len), "entry"),
                optional_vec_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.services.as_ref()),
                    loaded.config.services.as_ref(),
                ),
            );
            print_config_field(
                "dashboard",
                configured_value(loaded.config.dashboard.is_some()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.dashboard.as_ref()),
                    loaded.config.dashboard.as_ref(),
                ),
            );
            print_config_field(
                "output_defaults",
                configured_value(loaded.config.output_defaults.is_some()),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.output_defaults.as_ref()),
                    loaded.config.output_defaults.as_ref(),
                ),
            );
            print_config_field(
                "outputs",
                list_config_value(loaded.config.outputs.as_ref().map(Vec::len), "entry"),
                optional_vec_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.outputs.as_ref()),
                    loaded.config.outputs.as_ref(),
                ),
            );
            print_config_field(
                "hooks",
                list_config_value(
                    loaded
                        .config
                        .hooks
                        .as_ref()
                        .and_then(|hooks| hooks.commands.as_ref())
                        .map(Vec::len),
                    "entry",
                ),
                optional_field_source(
                    loaded,
                    local_config(loaded).and_then(|local| local.hooks.as_ref()),
                    loaded.config.hooks.as_ref(),
                ),
            );
        }
        None => {
            print_config_field("project", "<unset>", "not configured");
            print_config_field("service", "<unset>", "not configured");
            print_config_field(
                "default_range",
                format!("{}-{}", config.port_range.start, config.port_range.end),
                "built-in default",
            );
            print_config_field(
                "skip_ports",
                format!("{} ports", config.skip_ports.len()),
                "built-in default",
            );
            print_config_field("services", "<unset>", "not configured");
            print_config_field("dashboard", "<unset>", "not configured");
            print_config_field("output_defaults", "<unset>", "not configured");
            print_config_field("outputs", "<unset>", "not configured");
            print_config_field("hooks", "<unset>", "not configured");
        }
    }
}

pub(crate) fn print_config_field(name: &str, value: impl AsRef<str>, source: impl AsRef<str>) {
    println!("  {name}: {} ({})", value.as_ref(), source.as_ref());
}

pub(crate) fn optional_config_value(value: Option<&str>) -> String {
    non_empty_value(value)
        .map(str::to_string)
        .unwrap_or_else(|| String::from("<unset>"))
}

pub(crate) fn configured_value(configured: bool) -> &'static str {
    if configured { "configured" } else { "<unset>" }
}

pub(crate) fn list_config_value(count: Option<usize>, unit: &str) -> String {
    match count {
        Some(1) => format!("1 {unit}"),
        Some(count) if unit == "entry" => format!("{count} entries"),
        Some(count) => format!("{count} {unit}s"),
        None => String::from("<unset>"),
    }
}

pub(crate) fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

pub(crate) fn local_config(loaded: &LoadedConfig) -> Option<&BindPortConfig> {
    loaded.local_override.as_ref().map(|local| &local.config)
}

pub(crate) fn optional_field_source<T: ?Sized>(
    loaded: &LoadedConfig,
    local_value: Option<&T>,
    effective_value: Option<&T>,
) -> String {
    if local_value.is_some() {
        String::from("local override config")
    } else if effective_value.is_some() {
        source_config_label(loaded.source).to_string()
    } else {
        String::from("not configured")
    }
}

pub(crate) fn optional_vec_field_source<T>(
    loaded: &LoadedConfig,
    local_value: Option<&Vec<T>>,
    effective_value: Option<&Vec<T>>,
) -> String {
    optional_field_source(loaded, local_value, effective_value)
}

pub(crate) fn defaulted_field_source<T: ?Sized>(
    loaded: &LoadedConfig,
    local_value: Option<&T>,
    effective_value: Option<&T>,
) -> String {
    if local_value.is_some() {
        String::from("local override config")
    } else if effective_value.is_some() {
        source_config_label(loaded.source).to_string()
    } else {
        String::from("built-in default")
    }
}

pub(crate) fn defaulted_vec_field_source<T>(
    loaded: &LoadedConfig,
    local_value: Option<&Vec<T>>,
    effective_value: Option<&Vec<T>>,
) -> String {
    defaulted_field_source(loaded, local_value, effective_value)
}
