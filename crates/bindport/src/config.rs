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

pub(crate) fn run_config_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("explain") if args.len() == 1 => print_config_explain(),
        Some("validate") if args.len() == 1 => print_config_validate(),
        None | Some("--help" | "-h") => {
            print_config_help();
            ExitCode::SUCCESS
        }
        Some("explain") => {
            eprintln!("bindport: config explain does not take arguments");
            eprintln!("usage: bindport config explain");
            ExitCode::FAILURE
        }
        Some("validate") => {
            eprintln!("bindport: config validate does not take arguments");
            eprintln!("usage: bindport config validate");
            ExitCode::FAILURE
        }
        Some(command) => {
            eprintln!("bindport: unknown config command `{command}`");
            eprintln!("usage: bindport config explain|validate");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn print_config_validate() -> ExitCode {
    println!("BindPort config validate");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    println!("cwd: {}", cwd.display());

    let config = match resolve_config(&cwd) {
        Ok(config) => config,
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };

    print_config_source_explanation(&config);

    let issues = config
        .loaded
        .as_ref()
        .map(|loaded| loaded.config.validate())
        .unwrap_or_default();

    if issues.is_empty() {
        println!("validation: ok");
        ExitCode::SUCCESS
    } else {
        println!(
            "validation: {} {}",
            issues.len(),
            plural(issues.len(), "error")
        );
        for issue in issues {
            println!("  error: {issue}");
        }
        ExitCode::FAILURE
    }
}

pub(crate) fn print_config_explain() -> ExitCode {
    println!("BindPort config explain");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    println!("cwd: {}", cwd.display());

    let config = match resolve_config(&cwd) {
        Ok(config) => config,
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };

    print_config_source_explanation(&config);
    print_config_field_explanations(&config);

    let explained = explain_run_identity(&cwd, &[], &RunOptions::default(), &config);
    println!("identity:");
    println!(
        "  project: {} ({})",
        explained.identity.project, explained.project_source
    );
    println!(
        "  service: {} ({})",
        explained.identity.service, explained.service_source
    );
    println!("  key: {}", explained.identity.identity_key);

    ExitCode::SUCCESS
}

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

#[derive(Debug)]
pub(crate) struct IdentityExplanation {
    pub(crate) identity: ServiceIdentity,
    pub(crate) project_source: String,
    pub(crate) service_source: String,
}

pub(crate) fn explain_run_identity(
    cwd: &Path,
    command: &[String],
    options: &RunOptions,
    config: &ResolvedConfig,
) -> IdentityExplanation {
    let identity = resolve_run_identity(cwd, command, options, config);
    let env_project = env::var(BINDPORT_PROJECT_ENV).ok();
    let env_service = env::var(BINDPORT_SERVICE_ENV).ok();

    IdentityExplanation {
        project_source: identity_project_source(config, env_project.as_deref()),
        service_source: identity_service_source(cwd, config, options, env_service.as_deref()),
        identity,
    }
}

pub(crate) fn identity_project_source(
    config: &ResolvedConfig,
    env_project: Option<&str>,
) -> String {
    if non_empty_value(env_project).is_some() {
        return format!("environment {BINDPORT_PROJECT_ENV}");
    }

    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("inference");
    };

    if non_empty_value(local_config(loaded).and_then(|local| local.project.as_deref())).is_some() {
        String::from("local override config `project`")
    } else if non_empty_value(loaded.config.project.as_deref()).is_some() {
        format!("{} `project`", source_config_label(loaded.source))
    } else {
        String::from("inference")
    }
}

pub(crate) fn identity_service_source(
    cwd: &Path,
    config: &ResolvedConfig,
    options: &RunOptions,
    env_service: Option<&str>,
) -> String {
    if non_empty_value(options.service.as_deref()).is_some() {
        return String::from("CLI service argument");
    }

    if non_empty_value(env_service).is_some() {
        return format!("environment {BINDPORT_SERVICE_ENV}");
    }

    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("inference");
    };

    if let Some((_, source)) = config_service_source_for_cwd(loaded, cwd) {
        source
    } else {
        String::from("inference")
    }
}

pub(crate) fn config_service_source_for_cwd(
    loaded: &LoadedConfig,
    cwd: &Path,
) -> Option<(String, String)> {
    let service = loaded.configured_service_for_cwd(cwd)?;
    let name = non_empty_value(Some(service.name))?;

    Some((
        name.to_string(),
        configured_service_source_label(loaded, service.source),
    ))
}

pub(crate) fn configured_service_source_label(
    loaded: &LoadedConfig,
    source: ConfiguredServiceSource,
) -> String {
    match source {
        ConfiguredServiceSource::ServiceField => {
            if non_empty_value(local_config(loaded).and_then(|local| local.service.as_deref()))
                .is_some()
            {
                String::from("local override config `service`")
            } else {
                format!("{} `service`", source_config_label(loaded.source))
            }
        }
        ConfiguredServiceSource::PathMatch => {
            format!("{} `[[services]].path`", services_config_label(loaded))
        }
        ConfiguredServiceSource::SingleService => {
            format!("{} single `[[services]]`", services_config_label(loaded))
        }
    }
}

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
