use super::*;

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BindPortConfig {
    pub project: Option<String>,
    pub service: Option<String>,
    pub default_range: Option<String>,
    pub skip_ports: Option<Vec<u16>>,
    pub services: Option<Vec<ServiceConfig>>,
    pub dashboard: Option<DashboardConfig>,
    pub output_defaults: Option<OutputDefaultsConfig>,
    pub outputs: Option<Vec<OutputConfig>>,
    pub hooks: Option<HooksConfig>,
}

impl BindPortConfig {
    pub fn configured_service_name(&self) -> Option<&str> {
        self.service
            .as_deref()
            .or_else(|| self.single_service_name())
    }

    pub fn configured_service_name_for_cwd(&self, config_root: &Path, cwd: &Path) -> Option<&str> {
        self.configured_service_for_cwd(config_root, cwd)
            .map(|service| service.name)
    }

    pub fn configured_service_for_cwd(
        &self,
        config_root: &Path,
        cwd: &Path,
    ) -> Option<ConfiguredService<'_>> {
        if let Some(name) = self.service.as_deref() {
            return Some(ConfiguredService {
                name,
                source: ConfiguredServiceSource::ServiceField,
            });
        }

        if let Some(name) = self.service_name_for_cwd(config_root, cwd) {
            return Some(ConfiguredService {
                name,
                source: ConfiguredServiceSource::PathMatch,
            });
        }

        self.single_service_name().map(|name| ConfiguredService {
            name,
            source: ConfiguredServiceSource::SingleService,
        })
    }

    pub fn service_config(&self, service_name: &str) -> Option<&ServiceConfig> {
        self.services.as_deref()?.iter().find(|service| {
            service
                .name
                .as_deref()
                .is_some_and(|name| name == service_name)
        })
    }

    pub(crate) fn single_service_name(&self) -> Option<&str> {
        match self.services.as_deref() {
            Some([service]) => service.name.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn service_name_for_cwd(&self, config_root: &Path, cwd: &Path) -> Option<&str> {
        let services = self.services.as_deref()?;
        let cwd = absolute_path(config_root, cwd.to_path_buf());
        let mut best = None;

        for (index, service) in services.iter().enumerate() {
            let Some(name) = service
                .name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
            else {
                continue;
            };
            let Some(path) = service
                .path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
            else {
                continue;
            };

            let service_root = absolute_path(config_root, PathBuf::from(path));
            if !cwd.starts_with(&service_root) {
                continue;
            }

            let depth = service_root.components().count();
            match best {
                Some((best_depth, best_index, _))
                    if best_depth > depth || (best_depth == depth && best_index < index) => {}
                _ => best = Some((depth, index, name)),
            }
        }

        best.map(|(_, _, name)| name)
    }

    pub fn output_config(&self, output_name: &str) -> Option<&OutputConfig> {
        self.outputs.as_deref()?.iter().find(|output| {
            output
                .name
                .as_deref()
                .is_some_and(|name| name == output_name)
        })
    }

    pub fn effective_outputs(&self) -> Result<Vec<EffectiveOutputConfig>, OutputConfigError> {
        let Some(outputs) = self.outputs.as_deref() else {
            return Ok(Vec::new());
        };
        let defaults = self.output_defaults.as_ref();
        let mut seen_names = BTreeSet::new();
        let mut effective = Vec::new();

        for (index, output) in outputs.iter().enumerate() {
            let name = output
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .ok_or(OutputConfigError::MissingName { index })?;
            let name = name.to_string();

            if !seen_names.insert(name.clone()) {
                return Err(OutputConfigError::DuplicateName { name });
            }

            let enabled = output.enabled.unwrap_or(true);
            if !enabled {
                continue;
            }

            let template = output
                .template
                .as_ref()
                .filter(|template| !template.trim().is_empty())
                .cloned()
                .ok_or_else(|| OutputConfigError::MissingTemplate { name: name.clone() })?;
            let target = output
                .target
                .as_ref()
                .filter(|target| !target.trim().is_empty())
                .cloned()
                .ok_or_else(|| OutputConfigError::MissingTarget { name: name.clone() })?;

            effective.push(EffectiveOutputConfig {
                name,
                template,
                root: output
                    .root
                    .clone()
                    .or_else(|| defaults.and_then(|defaults| defaults.root.clone())),
                target,
                target_host: output
                    .target_host
                    .clone()
                    .or_else(|| defaults.and_then(|defaults| defaults.target_host.clone()))
                    .unwrap_or_else(|| DEFAULT_OUTPUT_TARGET_HOST.to_string()),
                target_scheme: output
                    .target_scheme
                    .clone()
                    .or_else(|| defaults.and_then(|defaults| defaults.target_scheme.clone()))
                    .unwrap_or_else(|| DEFAULT_OUTPUT_TARGET_SCHEME.to_string()),
                auto_render: output
                    .auto_render
                    .or_else(|| defaults.and_then(|defaults| defaults.auto_render))
                    .unwrap_or(DEFAULT_OUTPUT_AUTO_RENDER),
                delete_on: output
                    .delete_on
                    .clone()
                    .or_else(|| defaults.and_then(|defaults| defaults.delete_on.clone()))
                    .unwrap_or_else(|| vec![OutputDeleteState::Removed]),
                on_failure: output
                    .on_failure
                    .clone()
                    .or_else(|| defaults.and_then(|defaults| defaults.on_failure.clone()))
                    .unwrap_or(OutputFailurePolicy::Warn),
                debounce_ms: output
                    .debounce_ms
                    .or_else(|| defaults.and_then(|defaults| defaults.debounce_ms))
                    .unwrap_or(DEFAULT_OUTPUT_DEBOUNCE_MS),
                vars: output.vars.clone().unwrap_or_default(),
            });
        }

        Ok(effective)
    }

    pub fn validate(&self) -> Vec<ConfigValidationIssue> {
        let mut issues = Vec::new();

        validate_output_defaults(self.output_defaults.as_ref(), &mut issues);
        validate_services(self.services.as_deref(), &mut issues);
        validate_outputs(self.outputs.as_deref(), &mut issues);
        validate_hooks(self.hooks.as_ref(), &mut issues);

        issues
    }

    pub fn merge_local_override(&mut self, local: BindPortConfig) {
        override_option(&mut self.project, local.project);
        override_option(&mut self.service, local.service);
        override_option(&mut self.default_range, local.default_range);
        override_option(&mut self.skip_ports, local.skip_ports);
        override_option(&mut self.services, local.services);
        merge_option_with(&mut self.dashboard, local.dashboard, DashboardConfig::merge);
        merge_option_with(
            &mut self.output_defaults,
            local.output_defaults,
            OutputDefaultsConfig::merge,
        );
        merge_outputs(&mut self.outputs, local.outputs);
        merge_option_with(&mut self.hooks, local.hooks, HooksConfig::merge);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfiguredService<'a> {
    pub name: &'a str,
    pub source: ConfiguredServiceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfiguredServiceSource {
    ServiceField,
    PathMatch,
    SingleService,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServiceConfig {
    pub name: Option<String>,
    pub path: Option<String>,
    pub command: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
}

impl ServiceConfig {
    pub fn command_argv(&self) -> Option<Vec<String>> {
        let mut command = self.command.clone()?;
        if let Some(args) = self.args.as_ref() {
            command.extend(args.iter().cloned());
        }
        Some(command)
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DashboardConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub register_service: Option<bool>,
    pub allowed_hosts: Option<Vec<String>>,
    pub auth: Option<DashboardAuthConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DashboardAuthConfig {
    pub required: Option<bool>,
    pub token: Option<String>,
    pub token_env: Option<String>,
}

impl DashboardConfig {
    pub(crate) fn merge(&mut self, local: Self) {
        override_option(&mut self.host, local.host);
        override_option(&mut self.port, local.port);
        override_option(&mut self.register_service, local.register_service);
        override_option(&mut self.allowed_hosts, local.allowed_hosts);
        merge_option_with(&mut self.auth, local.auth, DashboardAuthConfig::merge);
    }
}

impl DashboardAuthConfig {
    pub(crate) fn merge(&mut self, local: Self) {
        override_option(&mut self.required, local.required);
        override_option(&mut self.token, local.token);
        override_option(&mut self.token_env, local.token_env);
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutputDefaultsConfig {
    pub root: Option<String>,
    pub target_host: Option<String>,
    pub target_scheme: Option<String>,
    pub auto_render: Option<bool>,
    pub delete_on: Option<Vec<OutputDeleteState>>,
    pub on_failure: Option<OutputFailurePolicy>,
    pub debounce_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveOutputConfig {
    pub name: String,
    pub template: String,
    pub root: Option<String>,
    pub target: String,
    pub target_host: String,
    pub target_scheme: String,
    pub auto_render: bool,
    pub delete_on: Vec<OutputDeleteState>,
    pub on_failure: OutputFailurePolicy,
    pub debounce_ms: u64,
    pub vars: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputConfigError {
    MissingName { index: usize },
    DuplicateName { name: String },
    MissingTemplate { name: String },
    MissingTarget { name: String },
}

impl fmt::Display for OutputConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingName { index } => {
                write!(f, "outputs[{index}] is missing required `name`")
            }
            Self::DuplicateName { name } => {
                write!(f, "output `{name}` is defined more than once")
            }
            Self::MissingTemplate { name } => {
                write!(f, "output `{name}` is missing required `template`")
            }
            Self::MissingTarget { name } => {
                write!(f, "output `{name}` is missing required `target`")
            }
        }
    }
}

impl std::error::Error for OutputConfigError {}

impl OutputDefaultsConfig {
    pub(crate) fn merge(&mut self, local: Self) {
        override_option(&mut self.root, local.root);
        override_option(&mut self.target_host, local.target_host);
        override_option(&mut self.target_scheme, local.target_scheme);
        override_option(&mut self.auto_render, local.auto_render);
        override_option(&mut self.delete_on, local.delete_on);
        override_option(&mut self.on_failure, local.on_failure);
        override_option(&mut self.debounce_ms, local.debounce_ms);
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputDeleteState {
    Stopped,
    Stale,
    Removed,
}

impl OutputDeleteState {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Stale => "stale",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFailurePolicy {
    Warn,
    Block,
}

impl OutputFailurePolicy {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutputConfig {
    pub enabled: Option<bool>,
    pub name: Option<String>,
    pub template: Option<String>,
    pub root: Option<String>,
    pub target: Option<String>,
    pub target_host: Option<String>,
    pub target_scheme: Option<String>,
    pub auto_render: Option<bool>,
    pub delete_on: Option<Vec<OutputDeleteState>>,
    pub on_failure: Option<OutputFailurePolicy>,
    pub debounce_ms: Option<u64>,
    pub vars: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HooksConfig {
    pub timeout_ms: Option<u64>,
    pub commands: Option<Vec<HookCommandConfig>>,
}

impl HooksConfig {
    pub(crate) fn merge(&mut self, local: Self) {
        override_option(&mut self.timeout_ms, local.timeout_ms);
        override_option(&mut self.commands, local.commands);
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HookCommandConfig {
    pub enabled: Option<bool>,
    pub name: Option<String>,
    pub events: Option<Vec<HookEvent>>,
    pub command: Option<Vec<String>>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    RouteStarted,
    RouteFinished,
    RoutesRemoved,
    RoutesMarkedStale,
    RenderRequested,
    OutputRendered,
}

impl HookEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RouteStarted => "route_started",
            Self::RouteFinished => "route_finished",
            Self::RoutesRemoved => "routes_removed",
            Self::RoutesMarkedStale => "routes_marked_stale",
            Self::RenderRequested => "render_requested",
            Self::OutputRendered => "output_rendered",
        }
    }
}
