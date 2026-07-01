// SPDX-License-Identifier: MIT

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

pub const SERVICE_NAME: &str = "bindport";
pub const DEFAULT_PORT_RANGE_START: u16 = 29_000;
pub const DEFAULT_PORT_RANGE_END: u16 = 29_999;
pub const DEFAULT_PORT_RANGE: PortRange = PortRange {
    start: DEFAULT_PORT_RANGE_START,
    end: DEFAULT_PORT_RANGE_END,
};
pub const DEFAULT_SKIP_PORTS: &[u16] = &[
    29_000, 29_070, 29_118, 29_167, 29_168, 29_169, 29_900, 29_901, 29_920, 29_999,
];
pub const DEFAULT_OUTPUT_TARGET_HOST: &str = "127.0.0.1";
pub const DEFAULT_OUTPUT_TARGET_SCHEME: &str = "http";
pub const DEFAULT_OUTPUT_AUTO_RENDER: bool = true;
pub const DEFAULT_OUTPUT_DEBOUNCE_MS: u64 = 250;
pub const DEFAULT_HOOK_TIMEOUT_MS: u64 = 5_000;
pub const CONFIG_FILENAMES: &[&str] = &[".bindport.toml", ".bindport.json", ".bindport.yaml"];
pub const LOCAL_CONFIG_FILENAMES: &[&str] = &[
    ".bindport.local.toml",
    ".bindport.local.json",
    ".bindport.local.yaml",
    ".bindport.local.yml",
    "bindport.local.toml",
    "bindport.local.json",
    "bindport.local.yaml",
    "bindport.local.yml",
];
pub const FALLBACK_CONFIG_FILE: &str = "config.toml";
pub const APPLIED_CONFIG_KEYS: &[&str] = &[
    "project",
    "service",
    "default_range",
    "skip_ports",
    "services",
    "dashboard",
    "output_defaults",
    "outputs",
    "hooks",
];
pub const BINDPORT_PROJECT_ENV: &str = "BINDPORT_PROJECT";
pub const BINDPORT_SERVICE_ENV: &str = "BINDPORT_SERVICE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub const fn contains(self, port: u16) -> bool {
        self.start <= port && port <= self.end
    }

    pub const fn len(self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.end as u32 - self.start as u32 + 1
        }
    }

    pub const fn is_empty(self) -> bool {
        self.start > self.end
    }
}

pub fn is_default_skip_port(port: u16) -> bool {
    DEFAULT_SKIP_PORTS.contains(&port)
}

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

    fn single_service_name(&self) -> Option<&str> {
        match self.services.as_deref() {
            Some([service]) => service.name.as_deref(),
            _ => None,
        }
    }

    fn service_name_for_cwd(&self, config_root: &Path, cwd: &Path) -> Option<&str> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValidationIssue {
    pub field: String,
    pub message: String,
}

impl ConfigValidationIssue {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

fn validate_services(services: Option<&[ServiceConfig]>, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(services) = services else {
        return;
    };
    let mut names = BTreeSet::new();

    for (index, service) in services.iter().enumerate() {
        let name_field = format!("services[{index}].name");
        match service.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => {
                if !names.insert(name.to_string()) {
                    issues.push(ConfigValidationIssue::new(
                        name_field,
                        format!("duplicate service name `{name}`; service names must be unique"),
                    ));
                }
            }
            _ => issues.push(ConfigValidationIssue::new(
                name_field,
                "service name is required",
            )),
        }

        if let Some(path) = service.path.as_deref() {
            validate_service_path(index, path, issues);
        }
        validate_service_command(index, service, issues);
    }
}

fn validate_service_path(index: usize, path: &str, issues: &mut Vec<ConfigValidationIssue>) {
    let field = format!("services[{index}].path");
    let path = path.trim();

    if path.is_empty() {
        issues.push(ConfigValidationIssue::new(
            field,
            "service path must not be empty",
        ));
        return;
    }

    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        issues.push(ConfigValidationIssue::new(
            field,
            "service path must be relative to the config file and must not contain `..`",
        ));
    }
}

fn validate_service_command(
    index: usize,
    service: &ServiceConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(command) = service.command.as_deref() else {
        if service.args.as_ref().is_some_and(|args| !args.is_empty()) {
            issues.push(ConfigValidationIssue::new(
                format!("services[{index}].args"),
                "service args require a service command",
            ));
        }
        return;
    };

    match command.first().map(String::as_str).map(str::trim) {
        Some(program) if !program.is_empty() => {}
        _ => issues.push(ConfigValidationIssue::new(
            format!("services[{index}].command"),
            "service command must start with a program",
        )),
    }
}

fn validate_outputs(outputs: Option<&[OutputConfig]>, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(outputs) = outputs else {
        return;
    };
    let mut names = BTreeSet::new();

    for (index, output) in outputs.iter().enumerate() {
        let name_field = format!("outputs[{index}].name");
        let Some(name) = output
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            issues.push(ConfigValidationIssue::new(
                name_field,
                "output name is required",
            ));
            continue;
        };

        if !names.insert(name.to_string()) {
            issues.push(ConfigValidationIssue::new(
                name_field,
                format!("duplicate output name `{name}`; output names must be unique"),
            ));
        }

        if !output.enabled.unwrap_or(true) {
            continue;
        }

        if output
            .template
            .as_deref()
            .map(str::trim)
            .filter(|template| !template.is_empty())
            .is_none()
        {
            issues.push(ConfigValidationIssue::new(
                format!("outputs[{index}].template"),
                format!("output `{name}` is missing required `template`"),
            ));
        }

        if output
            .target
            .as_deref()
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .is_none()
        {
            issues.push(ConfigValidationIssue::new(
                format!("outputs[{index}].target"),
                format!("output `{name}` is missing required `target`"),
            ));
        }
    }
}

fn validate_hooks(hooks: Option<&HooksConfig>, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(hooks) = hooks else {
        return;
    };

    if hooks.timeout_ms.is_some_and(|timeout| timeout == 0) {
        issues.push(ConfigValidationIssue::new(
            "hooks.timeout_ms",
            "hook timeout must be greater than 0",
        ));
    }

    let Some(commands) = hooks.commands.as_deref() else {
        return;
    };

    let mut names = BTreeSet::new();
    for (index, hook) in commands.iter().enumerate() {
        if !hook.enabled.unwrap_or(true) {
            continue;
        }

        let name = hook
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());

        if let Some(name) = name
            && !names.insert(name.to_string())
        {
            issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].name"),
                format!("duplicate hook name `{name}`; hook names must be unique"),
            ));
        }

        let Some(command) = hook.command.as_deref() else {
            issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].command"),
                "hook command is required",
            ));
            continue;
        };

        match command.first().map(String::as_str).map(str::trim) {
            Some(program) if !program.is_empty() => {}
            _ => issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].command"),
                "hook command must start with a program",
            )),
        }

        match hook.events.as_deref() {
            Some(events) if !events.is_empty() => {}
            Some(_) => issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].events"),
                "hook events must not be empty",
            )),
            None => issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].events"),
                "hook events are required",
            )),
        }

        if hook.timeout_ms.is_some_and(|timeout| timeout == 0) {
            issues.push(ConfigValidationIssue::new(
                format!("hooks.commands[{index}].timeout_ms"),
                "hook timeout must be greater than 0",
            ));
        }
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
    fn merge(&mut self, local: Self) {
        override_option(&mut self.host, local.host);
        override_option(&mut self.port, local.port);
        override_option(&mut self.register_service, local.register_service);
        override_option(&mut self.allowed_hosts, local.allowed_hosts);
        merge_option_with(&mut self.auth, local.auth, DashboardAuthConfig::merge);
    }
}

impl DashboardAuthConfig {
    fn merge(&mut self, local: Self) {
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
    fn merge(&mut self, local: Self) {
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
    fn merge(&mut self, local: Self) {
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

impl OutputConfig {
    fn merge(&mut self, local: Self) {
        override_option(&mut self.enabled, local.enabled);
        override_option(&mut self.template, local.template);
        override_option(&mut self.root, local.root);
        override_option(&mut self.target, local.target);
        override_option(&mut self.target_host, local.target_host);
        override_option(&mut self.target_scheme, local.target_scheme);
        override_option(&mut self.auto_render, local.auto_render);
        override_option(&mut self.delete_on, local.delete_on);
        override_option(&mut self.on_failure, local.on_failure);
        override_option(&mut self.debounce_ms, local.debounce_ms);
        merge_map_option(&mut self.vars, local.vars);
    }
}

fn override_option<T>(base: &mut Option<T>, local: Option<T>) {
    if local.is_some() {
        *base = local;
    }
}

fn merge_option_with<T>(base: &mut Option<T>, local: Option<T>, merge: impl FnOnce(&mut T, T)) {
    match (base.as_mut(), local) {
        (Some(base), Some(local)) => merge(base, local),
        (None, Some(local)) => *base = Some(local),
        (_, None) => {}
    }
}

fn merge_map_option<T>(base: &mut Option<BTreeMap<String, T>>, local: Option<BTreeMap<String, T>>) {
    let Some(local) = local else {
        return;
    };

    if let Some(base) = base {
        base.extend(local);
    } else {
        *base = Some(local);
    }
}

fn merge_outputs(base: &mut Option<Vec<OutputConfig>>, local: Option<Vec<OutputConfig>>) {
    let Some(local_outputs) = local else {
        return;
    };

    let Some(base_outputs) = base else {
        *base = Some(local_outputs);
        return;
    };

    for local_output in local_outputs {
        let Some(local_name) = local_output.name.as_deref() else {
            base_outputs.push(local_output);
            continue;
        };

        if let Some(base_output) = base_outputs
            .iter_mut()
            .find(|output| output.name.as_deref() == Some(local_name))
        {
            base_output.merge(local_output);
        } else {
            base_outputs.push(local_output);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIdentity {
    pub worktree_path: PathBuf,
    pub worktree_hash: String,
    pub git_common_dir: PathBuf,
    pub branch: String,
    pub branch_label: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub project: String,
    pub service: String,
    pub git: Option<GitIdentity>,
    pub identity_key: String,
}

impl ServiceIdentity {
    pub fn port_scan_start(&self, range: PortRange) -> Option<u16> {
        stable_port_scan_start(&self.identity_key, range)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IdentitySources<'a> {
    pub cwd: &'a Path,
    pub command: &'a [String],
    pub cli_project: Option<&'a str>,
    pub cli_service: Option<&'a str>,
    pub env_project: Option<&'a str>,
    pub env_service: Option<&'a str>,
    pub config_project: Option<&'a str>,
    pub config_service: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Toml,
    Json,
    Yaml,
}

impl ConfigFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("toml") => Some(Self::Toml),
            Some("json") => Some(Self::Json),
            Some("yaml") => Some(Self::Yaml),
            Some("yml") => Some(Self::Yaml),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Yaml => "yaml",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Project,
    Fallback,
}

impl ConfigSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub format: ConfigFormat,
    pub source: ConfigSource,
    pub local_override: Option<LoadedLocalConfig>,
    pub config: BindPortConfig,
    pub unknown_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedLocalConfig {
    pub path: PathBuf,
    pub format: ConfigFormat,
    pub config: BindPortConfig,
    pub unknown_keys: Vec<String>,
}

impl LoadedConfig {
    pub fn configured_service_name_for_cwd(&self, cwd: &Path) -> Option<&str> {
        self.configured_service_for_cwd(cwd)
            .map(|service| service.name)
    }

    pub fn configured_service_for_cwd(&self, cwd: &Path) -> Option<ConfiguredService<'_>> {
        let config_root = self.path.parent().unwrap_or_else(|| Path::new("."));

        self.config.configured_service_for_cwd(config_root, cwd)
    }

    pub fn port_range(&self) -> Result<PortRange, ConfigError> {
        self.config
            .default_range
            .as_deref()
            .map(parse_port_range)
            .transpose()
            .map_err(|source| ConfigError::InvalidPortRange {
                path: self.path.clone(),
                source,
            })
            .map(|range| range.unwrap_or(DEFAULT_PORT_RANGE))
    }

    pub fn skip_ports(&self) -> Vec<u16> {
        self.config
            .skip_ports
            .clone()
            .unwrap_or_else(|| DEFAULT_SKIP_PORTS.to_vec())
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    UnknownFormat {
        path: PathBuf,
    },
    Parse {
        path: PathBuf,
        format: ConfigFormat,
        source: String,
    },
    InvalidPortRange {
        path: PathBuf,
        source: PortRangeParseError,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read config `{}`: {source}", path.display())
            }
            Self::UnknownFormat { path } => {
                write!(f, "unsupported config format `{}`", path.display())
            }
            Self::Parse {
                path,
                format,
                source,
            } => {
                write!(
                    f,
                    "failed to parse {} config `{}`: {source}",
                    format.as_str(),
                    path.display()
                )
            }
            Self::InvalidPortRange { path, source } => {
                write!(
                    f,
                    "invalid default_range in config `{}`: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::InvalidPortRange { source, .. } => Some(source),
            Self::UnknownFormat { .. } | Self::Parse { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortRangeParseError {
    MissingSeparator,
    InvalidStart(String),
    InvalidEnd(String),
    Empty(PortRange),
}

impl fmt::Display for PortRangeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator => write!(f, "expected START-END"),
            Self::InvalidStart(value) => write!(f, "invalid range start `{value}`"),
            Self::InvalidEnd(value) => write!(f, "invalid range end `{value}`"),
            Self::Empty(range) => write!(
                f,
                "range start {} must be less than or equal to end {}",
                range.start, range.end
            ),
        }
    }
}

impl std::error::Error for PortRangeParseError {}

pub fn discover_config(
    start: &Path,
    fallback_path: Option<&Path>,
) -> Result<Option<LoadedConfig>, ConfigError> {
    for directory in start.ancestors() {
        for filename in CONFIG_FILENAMES {
            let path = directory.join(filename);

            if path.is_file() {
                return load_config(path, ConfigSource::Project)
                    .and_then(load_project_local_override)
                    .map(Some);
            }
        }
    }

    if let Some(path) = fallback_path.filter(|path| path.is_file()) {
        return load_config(path, ConfigSource::Fallback).map(Some);
    }

    Ok(None)
}

fn load_project_local_override(mut loaded: LoadedConfig) -> Result<LoadedConfig, ConfigError> {
    if loaded.source != ConfigSource::Project {
        return Ok(loaded);
    }

    let Some(directory) = loaded.path.parent() else {
        return Ok(loaded);
    };

    for filename in LOCAL_CONFIG_FILENAMES {
        let path = directory.join(filename);

        if path.is_file() {
            let local = load_config(path, ConfigSource::Project)?;
            let LoadedConfig {
                path,
                format,
                config,
                unknown_keys,
                ..
            } = local;
            loaded.config.merge_local_override(config.clone());
            loaded.unknown_keys.extend(unknown_keys.clone());
            loaded.unknown_keys.sort();
            loaded.unknown_keys.dedup();
            loaded.local_override = Some(LoadedLocalConfig {
                path,
                format,
                config,
                unknown_keys,
            });
            return Ok(loaded);
        }
    }

    Ok(loaded)
}

pub fn load_config(
    path: impl Into<PathBuf>,
    source: ConfigSource,
) -> Result<LoadedConfig, ConfigError> {
    let path = path.into();
    let format = ConfigFormat::from_path(&path)
        .ok_or_else(|| ConfigError::UnknownFormat { path: path.clone() })?;
    let contents = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    let config = parse_config(format, &contents).map_err(|source| ConfigError::Parse {
        path: path.clone(),
        format,
        source,
    })?;
    let unknown_keys =
        unknown_top_level_config_keys(format, &contents).map_err(|source| ConfigError::Parse {
            path: path.clone(),
            format,
            source,
        })?;

    Ok(LoadedConfig {
        path,
        format,
        source,
        local_override: None,
        config,
        unknown_keys,
    })
}

pub fn parse_config(format: ConfigFormat, contents: &str) -> Result<BindPortConfig, String> {
    match format {
        ConfigFormat::Toml => toml::from_str(contents).map_err(|error| error.to_string()),
        ConfigFormat::Json => serde_json::from_str(contents).map_err(|error| error.to_string()),
        ConfigFormat::Yaml => serde_yaml_ng::from_str(contents).map_err(|error| error.to_string()),
    }
}

pub fn resolve_identity(sources: IdentitySources<'_>) -> ServiceIdentity {
    let git = detect_git_identity(sources.cwd);
    let package = package_inference(sources.cwd, git.as_ref());
    let project = first_non_empty([
        sources.cli_project,
        sources.env_project,
        sources.config_project,
    ])
    .map(str::to_owned)
    .or_else(|| package.project_name())
    .unwrap_or_else(|| infer_project_name(sources.cwd, git.as_ref()));
    let service = first_non_empty([
        sources.cli_service,
        sources.env_service,
        sources.config_service,
    ])
    .map(str::to_owned)
    .or_else(|| package.service_name())
    .unwrap_or_else(|| infer_service_name(sources.command));
    let identity_key = identity_key(&project, &service, sources.cwd, git.as_ref());

    ServiceIdentity {
        project,
        service,
        git,
        identity_key,
    }
}

pub fn detect_git_identity(cwd: &Path) -> Option<GitIdentity> {
    let worktree_path = git_output(cwd, ["rev-parse", "--show-toplevel"])?;
    let worktree_path = absolute_path(cwd, PathBuf::from(worktree_path));
    let git_common_dir = git_output(cwd, ["rev-parse", "--git-common-dir"])?;
    let git_common_dir = absolute_path(cwd, PathBuf::from(git_common_dir));
    let commit = git_output(cwd, ["rev-parse", "--short", "HEAD"])?;
    let branch = git_output(cwd, ["branch", "--show-current"])
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| format!("detached-{commit}"));
    let branch_label = normalize_branch_label(&branch);
    let worktree_hash = stable_path_hash(&worktree_path);

    Some(GitIdentity {
        worktree_path,
        worktree_hash,
        git_common_dir,
        branch,
        branch_label,
        commit,
    })
}

pub fn normalize_branch_label(branch: &str) -> String {
    let mut label = String::new();
    let mut previous_was_separator = false;

    for character in branch.chars() {
        if character.is_ascii_alphanumeric() {
            label.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator && !label.is_empty() {
            label.push('-');
            previous_was_separator = true;
        }
    }

    while label.ends_with('-') {
        label.pop();
    }

    if label.is_empty() {
        String::from("branch")
    } else {
        label
    }
}

fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> Option<String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=")
        .arg("-c")
        .arg("core.pager=cat")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn absolute_path(cwd: &Path, path: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };

    path.canonicalize().unwrap_or(path)
}

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<&str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn infer_project_name(cwd: &Path, git: Option<&GitIdentity>) -> String {
    git.map(|git| git.worktree_path.as_path())
        .unwrap_or(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn infer_service_name(command: &[String]) -> String {
    command
        .first()
        .and_then(|program| Path::new(program).file_stem())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("command")
        .to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageInference {
    root: Option<PackageMetadata>,
    nearest: Option<PackageMetadata>,
}

impl PackageInference {
    fn project_name(&self) -> Option<String> {
        self.root
            .as_ref()
            .or(self.nearest.as_ref())
            .map(|package| package.identity_name.clone())
    }

    fn service_name(&self) -> Option<String> {
        self.nearest
            .as_ref()
            .map(|package| package.identity_name.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageMetadata {
    identity_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceRoot {
    path: PathBuf,
    metadata: PackageMetadata,
}

#[derive(Debug, Deserialize)]
struct PnpmWorkspaceConfig {
    packages: Option<Vec<String>>,
}

fn package_inference(cwd: &Path, git: Option<&GitIdentity>) -> PackageInference {
    let git_boundary = git.map(|git| git.worktree_path.as_path());
    let workspace_root = nearest_workspace_root(cwd, git_boundary);
    let package_boundary = workspace_root
        .as_ref()
        .map(|workspace| workspace.path.as_path())
        .or(git_boundary);
    let root = workspace_root
        .as_ref()
        .map(|workspace| workspace.metadata.clone())
        .or_else(|| git.and_then(|git| read_package_metadata(&git.worktree_path)));
    let nearest = nearest_package_metadata(cwd, package_boundary);

    PackageInference { root, nearest }
}

fn nearest_workspace_root(cwd: &Path, boundary: Option<&Path>) -> Option<WorkspaceRoot> {
    let cwd = absolute_path(cwd, cwd.to_path_buf());

    for directory in cwd.ancestors() {
        if let Some(boundary) = boundary
            && !directory.starts_with(boundary)
        {
            break;
        }

        if is_workspace_root(directory) {
            return Some(WorkspaceRoot {
                path: directory.to_path_buf(),
                metadata: workspace_root_metadata(directory),
            });
        }

        if Some(directory) == boundary {
            break;
        }
    }

    None
}

fn is_workspace_root(directory: &Path) -> bool {
    package_json_has_workspaces(directory) || pnpm_workspace_has_packages(directory)
}

fn package_json_has_workspaces(directory: &Path) -> bool {
    let contents = fs::read_to_string(directory.join("package.json")).ok();
    let Some(value) = contents
        .as_deref()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(contents).ok())
    else {
        return false;
    };

    workspace_packages_present(value.get("workspaces"))
}

fn workspace_packages_present(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Array(packages)) => packages.iter().any(non_empty_json_string),
        Some(serde_json::Value::Object(workspace)) => workspace
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|packages| packages.iter().any(non_empty_json_string)),
        _ => false,
    }
}

fn non_empty_json_string(value: &serde_json::Value) -> bool {
    value
        .as_str()
        .is_some_and(|package| !package.trim().is_empty())
}

fn pnpm_workspace_has_packages(directory: &Path) -> bool {
    let contents = fs::read_to_string(directory.join("pnpm-workspace.yaml")).ok();
    let Some(config) = contents
        .as_deref()
        .and_then(|contents| serde_yaml_ng::from_str::<PnpmWorkspaceConfig>(contents).ok())
    else {
        return false;
    };

    config
        .packages
        .is_some_and(|packages| packages.iter().any(|package| !package.trim().is_empty()))
}

fn workspace_root_metadata(directory: &Path) -> PackageMetadata {
    read_package_metadata(directory).unwrap_or_else(|| PackageMetadata {
        identity_name: directory_identity_name(directory),
    })
}

fn nearest_package_metadata(cwd: &Path, boundary: Option<&Path>) -> Option<PackageMetadata> {
    let cwd = absolute_path(cwd, cwd.to_path_buf());

    for directory in cwd.ancestors() {
        if let Some(boundary) = boundary
            && !directory.starts_with(boundary)
        {
            break;
        }

        if let Some(package) = read_package_metadata(directory) {
            return Some(package);
        }

        if Some(directory) == boundary {
            break;
        }
    }

    None
}

fn read_package_metadata(directory: &Path) -> Option<PackageMetadata> {
    let contents = fs::read_to_string(directory.join("package.json")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
    let name = value.get("name")?.as_str()?;
    let identity_name = package_identity_name(name)?;

    Some(PackageMetadata { identity_name })
}

fn directory_identity_name(directory: &Path) -> String {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(package_identity_name)
        .unwrap_or(None)
        .unwrap_or_else(|| String::from("workspace"))
}

fn package_identity_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let name = if let Some(scoped) = name.strip_prefix('@') {
        scoped.split_once('/').map(|(_, package)| package)?
    } else {
        name
    };
    let name = name.trim();

    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

fn identity_key(project: &str, service: &str, cwd: &Path, git: Option<&GitIdentity>) -> String {
    let (path_hash, branch_label) = git
        .map(|git| (git.worktree_hash.as_str(), git.branch_label.as_str()))
        .unwrap_or_else(|| ("no-git", "no-branch"));
    let path_hash = if path_hash == "no-git" {
        stable_path_hash(&absolute_path(cwd, cwd.to_path_buf()))
    } else {
        path_hash.to_owned()
    };

    format!(
        "v1:p{}:{project}:s{}:{service}:w{path_hash}:b{}:{branch_label}",
        project.len(),
        service.len(),
        branch_label.len()
    )
}

pub fn stable_port_scan_start(seed: &str, range: PortRange) -> Option<u16> {
    if range.is_empty() {
        return None;
    }

    let offset = stable_hash(seed.as_bytes()) % u64::from(range.len());
    let port = range.start as u32 + u32::try_from(offset).expect("range length fits in u32");

    Some(u16::try_from(port).expect("port remains within configured range"))
}

fn stable_path_hash(path: &Path) -> String {
    let path = path.to_string_lossy();

    format!("{:016x}", stable_hash(path.as_bytes()))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;

    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

fn unknown_top_level_config_keys(
    format: ConfigFormat,
    contents: &str,
) -> Result<Vec<String>, String> {
    match format {
        ConfigFormat::Toml => {
            let table = contents
                .parse::<toml::Table>()
                .map_err(|error| error.to_string())?;
            Ok(unknown_config_keys(table.keys().map(String::as_str)))
        }
        ConfigFormat::Json => {
            let value = serde_json::from_str::<serde_json::Value>(contents)
                .map_err(|error| error.to_string())?;
            let Some(object) = value.as_object() else {
                return Ok(Vec::new());
            };
            Ok(unknown_config_keys(object.keys().map(String::as_str)))
        }
        ConfigFormat::Yaml => {
            let value = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(contents)
                .map_err(|error| error.to_string())?;
            let Some(mapping) = value.as_mapping() else {
                return Ok(Vec::new());
            };
            Ok(unknown_config_keys(
                mapping.keys().filter_map(serde_yaml_ng::Value::as_str),
            ))
        }
    }
}

fn unknown_config_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut keys = keys
        .into_iter()
        .filter(|key| !APPLIED_CONFIG_KEYS.contains(key))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

pub fn parse_port_range(value: &str) -> Result<PortRange, PortRangeParseError> {
    let (start, end) = value
        .split_once('-')
        .ok_or(PortRangeParseError::MissingSeparator)?;
    let start = start
        .trim()
        .parse::<u16>()
        .map_err(|_| PortRangeParseError::InvalidStart(start.trim().to_owned()))?;
    let end = end
        .trim()
        .parse::<u16>()
        .map_err(|_| PortRangeParseError::InvalidEnd(end.trim().to_owned()))?;
    let range = PortRange { start, end };

    if range.is_empty() {
        return Err(PortRangeParseError::Empty(range));
    }

    Ok(range)
}

pub fn default_fallback_config() -> String {
    let skip_ports = DEFAULT_SKIP_PORTS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "# BindPort fallback config. Project .bindport.* files discovered upward override this file.\n\
         # This file is optional; BindPort uses built-in defaults when no config exists.\n\
         default_range = \"{}-{}\"\n\
         skip_ports = [{}]\n\
         \n\
         [dashboard]\n\
         host = \"127.0.0.1\"\n\
         port = 27080\n\
         register_service = false\n\
         allowed_hosts = [\"localhost\", \"127.0.0.1\"]\n\
         \n\
         [dashboard.auth]\n\
         required = false\n\
         token_env = \"BINDPORT_DASHBOARD_TOKEN\"\n",
        DEFAULT_PORT_RANGE.start, DEFAULT_PORT_RANGE.end, skip_ports
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn default_range_matches_roadmap() {
        assert_eq!(DEFAULT_PORT_RANGE.start, 29_000);
        assert_eq!(DEFAULT_PORT_RANGE.end, 29_999);
        assert_eq!(DEFAULT_PORT_RANGE.len(), 1_000);
    }

    #[test]
    fn inverted_range_is_empty() {
        let range = PortRange { start: 100, end: 0 };

        assert!(range.is_empty());
        assert_eq!(range.len(), 0);
    }

    #[test]
    fn default_skiplist_marks_reserved_ports() {
        assert!(is_default_skip_port(29_000));
        assert!(is_default_skip_port(29_999));
        assert!(!is_default_skip_port(29_500));
    }

    #[test]
    fn config_filenames_preserve_format_precedence() {
        assert_eq!(
            CONFIG_FILENAMES,
            [".bindport.toml", ".bindport.json", ".bindport.yaml"]
        );
    }

    #[test]
    fn parses_config_formats() {
        let toml = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\ndefault_range = \"29100-29199\"\nskip_ports = [29100]\n[dashboard]\nhost = \"127.0.0.1\"\nport = 27080\nregister_service = true\nallowed_hosts = [\"localhost\"]\n[dashboard.auth]\nrequired = true\ntoken_env = \"BINDPORT_DASHBOARD_TOKEN\"\n[[services]]\nname = \"web\"\npath = \"apps/web\"\ncommand = [\"storybook\", \"dev\"]\nargs = [\"--port\", \"{port}\"]\nhostname = \"{branch}.{project}.localhost\"\nenv.PORT = \"{port}\"\nenv.NEXT_PUBLIC_BINDPORT_URL = \"{route_url}\"\n",
        )
        .expect("toml config");
        let json = parse_config(
            ConfigFormat::Json,
            r#"{"project":"demo","default_range":"29100-29199","skip_ports":[29100],"dashboard":{"host":"127.0.0.1","port":27080,"register_service":true,"allowed_hosts":["localhost"],"auth":{"required":true,"token_env":"BINDPORT_DASHBOARD_TOKEN"}},"services":[{"name":"web","path":"apps/web","command":["storybook","dev"],"args":["--port","{port}"],"hostname":"{branch}.{project}.localhost","env":{"PORT":"{port}","NEXT_PUBLIC_BINDPORT_URL":"{route_url}"}}]}"#,
        )
        .expect("json config");
        let yaml = parse_config(
            ConfigFormat::Yaml,
            "project: demo\ndefault_range: 29100-29199\nskip_ports:\n  - 29100\ndashboard:\n  host: 127.0.0.1\n  port: 27080\n  register_service: true\n  allowed_hosts:\n    - localhost\n  auth:\n    required: true\n    token_env: BINDPORT_DASHBOARD_TOKEN\nservices:\n  - name: web\n    path: apps/web\n    command:\n      - storybook\n      - dev\n    args:\n      - --port\n      - \"{port}\"\n    hostname: \"{branch}.{project}.localhost\"\n    env:\n      PORT: \"{port}\"\n      NEXT_PUBLIC_BINDPORT_URL: \"{route_url}\"\n",
        )
        .expect("yaml config");

        assert_eq!(toml, json);
        assert_eq!(json, yaml);
        let dashboard = toml.dashboard.as_ref().expect("dashboard config");
        assert_eq!(dashboard.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(dashboard.port, Some(27_080));
        assert_eq!(dashboard.register_service, Some(true));
        assert_eq!(
            dashboard.allowed_hosts,
            Some(vec![String::from("localhost")])
        );
        let auth = dashboard.auth.as_ref().expect("dashboard auth");
        assert_eq!(auth.required, Some(true));
        assert_eq!(auth.token_env.as_deref(), Some("BINDPORT_DASHBOARD_TOKEN"));
        let service = toml.service_config("web").expect("service config by name");
        assert_eq!(service.path.as_deref(), Some("apps/web"));
        assert_eq!(
            service.command_argv(),
            Some(vec![
                String::from("storybook"),
                String::from("dev"),
                String::from("--port"),
                String::from("{port}"),
            ])
        );
        assert_eq!(
            service.hostname.as_deref(),
            Some("{branch}.{project}.localhost")
        );
        assert_eq!(
            service
                .env
                .as_ref()
                .and_then(|env| env.get("NEXT_PUBLIC_BINDPORT_URL"))
                .map(String::as_str),
            Some("{route_url}")
        );
        assert_eq!(toml.configured_service_name(), Some("web"));
    }

    #[test]
    fn parses_hook_config_formats() {
        let toml = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[hooks]\ntimeout_ms = 2500\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\", \"output_rendered\"]\ncommand = [\"bindport\", \"render\"]\ntimeout_ms = 1000\n",
        )
        .expect("toml hooks config");
        let json = parse_config(
            ConfigFormat::Json,
            r#"{"project":"demo","hooks":{"timeout_ms":2500,"commands":[{"name":"reload","events":["route_started","output_rendered"],"command":["bindport","render"],"timeout_ms":1000}]}}"#,
        )
        .expect("json hooks config");
        let yaml = parse_config(
            ConfigFormat::Yaml,
            "project: demo\nhooks:\n  timeout_ms: 2500\n  commands:\n    - name: reload\n      events:\n        - route_started\n        - output_rendered\n      command:\n        - bindport\n        - render\n      timeout_ms: 1000\n",
        )
        .expect("yaml hooks config");

        assert_eq!(toml, json);
        assert_eq!(json, yaml);
        let hooks = toml.hooks.as_ref().expect("hooks");
        assert_eq!(hooks.timeout_ms, Some(2_500));
        let command = &hooks.commands.as_ref().expect("hook commands")[0];
        assert_eq!(command.name.as_deref(), Some("reload"));
        assert_eq!(
            command.events,
            Some(vec![HookEvent::RouteStarted, HookEvent::OutputRendered])
        );
        assert_eq!(
            command.command,
            Some(vec![String::from("bindport"), String::from("render")])
        );
        assert_eq!(command.timeout_ms, Some(1_000));
    }

    #[test]
    fn service_paths_infer_service_from_cwd() {
        let root = temp_test_dir("service-paths");
        let web_src = root.join("apps").join("web").join("src");
        let api = root.join("apps").join("api");
        fs::create_dir_all(&web_src).expect("web src");
        fs::create_dir_all(&api).expect("api dir");
        let config = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
        )
        .expect("config");

        assert_eq!(
            config.configured_service_name_for_cwd(&root, &web_src),
            Some("web")
        );
        let matched = config
            .configured_service_for_cwd(&root, &web_src)
            .expect("matched web service");
        assert_eq!(matched.name, "web");
        assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);
        assert_eq!(
            config.configured_service_name_for_cwd(&root, &api),
            Some("api")
        );
        assert_eq!(config.configured_service_name_for_cwd(&root, &root), None);
    }

    #[test]
    fn deepest_service_path_match_wins() {
        let root = temp_test_dir("service-path-depth");
        let api_src = root.join("apps").join("api").join("src");
        fs::create_dir_all(&api_src).expect("api src");
        let config = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[[services]]\nname = \"apps\"\npath = \"apps\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
        )
        .expect("config");

        assert_eq!(
            config.configured_service_name_for_cwd(&root, &api_src),
            Some("api")
        );
        let matched = config
            .configured_service_for_cwd(&root, &api_src)
            .expect("matched api service");
        assert_eq!(matched.name, "api");
        assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);
    }

    #[test]
    fn configured_service_precedence_covers_path_ties_and_single_service() {
        let root = temp_test_dir("service-precedence");
        let web_src = root.join("apps").join("web").join("src");
        fs::create_dir_all(&web_src).expect("web src");
        let config = BindPortConfig {
            services: Some(vec![
                ServiceConfig {
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("empty-path")),
                    path: Some(String::from(" ")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("first-web")),
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("second-web")),
                    path: Some(String::from("apps/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("apps")),
                    path: Some(String::from("apps")),
                    ..ServiceConfig::default()
                },
            ]),
            ..BindPortConfig::default()
        };

        let matched = config
            .configured_service_for_cwd(&root, &web_src)
            .expect("matched first web service");
        assert_eq!(matched.name, "first-web");
        assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);

        let explicit = BindPortConfig {
            service: Some(String::from("explicit")),
            services: config.services.clone(),
            ..BindPortConfig::default()
        };
        assert_eq!(
            explicit.configured_service_for_cwd(&root, &web_src),
            Some(ConfiguredService {
                name: "explicit",
                source: ConfiguredServiceSource::ServiceField
            })
        );

        let single = BindPortConfig {
            services: Some(vec![ServiceConfig {
                name: Some(String::from("solo")),
                ..ServiceConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        assert_eq!(
            single.configured_service_for_cwd(&root, &root),
            Some(ConfiguredService {
                name: "solo",
                source: ConfiguredServiceSource::SingleService
            })
        );
    }

    #[test]
    fn parses_output_config_formats() {
        let toml = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[output_defaults]\nroot = \".bindport/generated\"\ntarget_host = \"127.0.0.1\"\ntarget_scheme = \"http\"\nauto_render = true\ndelete_on = [\"removed\"]\non_failure = \"warn\"\ndebounce_ms = 250\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"web\"]\ntls = false\n",
        )
        .expect("toml config");
        let json = parse_config(
            ConfigFormat::Json,
            r#"{"project":"demo","output_defaults":{"root":".bindport/generated","target_host":"127.0.0.1","target_scheme":"http","auto_render":true,"delete_on":["removed"],"on_failure":"warn","debounce_ms":250},"outputs":[{"name":"traefik","template":"bindport-traefik","target":"traefik/{{ route.slug }}.yml","vars":{"entrypoints":["web"],"tls":false}}]}"#,
        )
        .expect("json config");
        let yaml = parse_config(
            ConfigFormat::Yaml,
            "project: demo\noutput_defaults:\n  root: .bindport/generated\n  target_host: 127.0.0.1\n  target_scheme: http\n  auto_render: true\n  delete_on:\n    - removed\n  on_failure: warn\n  debounce_ms: 250\noutputs:\n  - name: traefik\n    template: bindport-traefik\n    target: traefik/{{ route.slug }}.yml\n    vars:\n      entrypoints:\n        - web\n      tls: false\n",
        )
        .expect("yaml config");

        assert_eq!(toml, json);
        assert_eq!(json, yaml);
        let defaults = toml.output_defaults.as_ref().expect("output defaults");
        assert_eq!(defaults.root.as_deref(), Some(".bindport/generated"));
        assert_eq!(defaults.delete_on, Some(vec![OutputDeleteState::Removed]));
        assert_eq!(defaults.on_failure, Some(OutputFailurePolicy::Warn));
        assert_eq!(defaults.debounce_ms, Some(250));

        let output = toml.output_config("traefik").expect("output by name");
        assert_eq!(output.template.as_deref(), Some("bindport-traefik"));
        assert_eq!(
            output
                .vars
                .as_ref()
                .and_then(|vars| vars.get("entrypoints")),
            Some(&serde_json::json!(["web"]))
        );
        assert_eq!(
            output.vars.as_ref().and_then(|vars| vars.get("tls")),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn local_override_merges_output_config_by_name() {
        let root = temp_test_dir("local-output-override");
        fs::write(
            root.join(".bindport.toml"),
            "project = \"base-project\"\n[output_defaults]\nroot = \".bindport/generated\"\ndebounce_ms = 250\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"web\"]\ntls = false\n[[outputs]]\nname = \"debug\"\ntemplate = \"debug-route\"\ntarget = \"debug/{{ route.slug }}.txt\"\n",
        )
        .expect("write base config");
        fs::write(
            root.join(".bindport.local.toml"),
            "project = \"local-project\"\n[output_defaults]\nroot = \"/tmp/bindport-traefik\"\n[[outputs]]\nname = \"traefik\"\ntarget = \"{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"websecure\"]\n[[outputs]]\nname = \"extra\"\ntemplate = \"extra-template\"\ntarget = \"extra/{{ route.slug }}.txt\"\n",
        )
        .expect("write local override");

        let loaded = discover_config(&root, None)
            .expect("discover config")
            .expect("loaded config");

        assert_eq!(loaded.config.project.as_deref(), Some("local-project"));
        assert_eq!(
            loaded
                .local_override
                .as_ref()
                .map(|local| local.path.as_path()),
            Some(root.join(".bindport.local.toml").as_path())
        );
        let defaults = loaded
            .config
            .output_defaults
            .as_ref()
            .expect("output defaults");
        assert_eq!(defaults.root.as_deref(), Some("/tmp/bindport-traefik"));
        assert_eq!(defaults.debounce_ms, Some(250));

        let traefik = loaded
            .config
            .output_config("traefik")
            .expect("merged traefik output");
        assert_eq!(traefik.template.as_deref(), Some("bindport-traefik"));
        assert_eq!(traefik.target.as_deref(), Some("{{ route.slug }}.yml"));
        assert_eq!(
            traefik
                .vars
                .as_ref()
                .and_then(|vars| vars.get("entrypoints")),
            Some(&serde_json::json!(["websecure"]))
        );
        assert_eq!(
            traefik.vars.as_ref().and_then(|vars| vars.get("tls")),
            Some(&serde_json::json!(false))
        );
        assert!(loaded.config.output_config("debug").is_some());
        assert!(loaded.config.output_config("extra").is_some());
    }

    #[test]
    fn local_override_merges_dashboard_defaults_and_output_edges() {
        let mut config = BindPortConfig {
            dashboard: Some(DashboardConfig {
                host: Some(String::from("127.0.0.1")),
                port: Some(27_080),
                register_service: Some(false),
                auth: Some(DashboardAuthConfig {
                    required: Some(false),
                    token_env: Some(String::from("OLD_TOKEN")),
                    ..DashboardAuthConfig::default()
                }),
                ..DashboardConfig::default()
            }),
            output_defaults: Some(OutputDefaultsConfig {
                root: Some(String::from(".bindport/generated")),
                target_host: Some(String::from("127.0.0.1")),
                ..OutputDefaultsConfig::default()
            }),
            ..BindPortConfig::default()
        };

        config.merge_local_override(BindPortConfig {
            dashboard: Some(DashboardConfig {
                port: Some(27_081),
                allowed_hosts: Some(vec![String::from("localhost")]),
                auth: Some(DashboardAuthConfig {
                    required: Some(true),
                    token: Some(String::from("test-token")),
                    ..DashboardAuthConfig::default()
                }),
                ..DashboardConfig::default()
            }),
            output_defaults: Some(OutputDefaultsConfig {
                target_scheme: Some(String::from("https")),
                ..OutputDefaultsConfig::default()
            }),
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("traefik")),
                template: Some(String::from("bindport-traefik")),
                target: Some(String::from("{{ route.slug }}.yml")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        });

        let dashboard = config.dashboard.as_ref().expect("dashboard");
        assert_eq!(dashboard.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(dashboard.port, Some(27_081));
        assert_eq!(dashboard.register_service, Some(false));
        assert_eq!(
            dashboard.allowed_hosts,
            Some(vec![String::from("localhost")])
        );
        let auth = dashboard.auth.as_ref().expect("auth");
        assert_eq!(auth.required, Some(true));
        assert_eq!(auth.token.as_deref(), Some("test-token"));
        assert_eq!(auth.token_env.as_deref(), Some("OLD_TOKEN"));

        let defaults = config.output_defaults.as_ref().expect("output defaults");
        assert_eq!(defaults.root.as_deref(), Some(".bindport/generated"));
        assert_eq!(defaults.target_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(defaults.target_scheme.as_deref(), Some("https"));
        assert!(config.output_config("traefik").is_some());

        config.merge_local_override(BindPortConfig {
            outputs: Some(vec![OutputConfig {
                template: Some(String::from("nameless")),
                target: Some(String::from("nameless.txt")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        });
        assert_eq!(config.outputs.as_ref().expect("outputs").len(), 2);
    }

    #[test]
    fn effective_outputs_apply_defaults_and_skip_disabled_entries() {
        let config = parse_config(
            ConfigFormat::Toml,
            "project = \"demo\"\n[output_defaults]\nroot = \".bindport/generated\"\ntarget_host = \"host.docker.internal\"\ntarget_scheme = \"https\"\nauto_render = false\ndelete_on = [\"stopped\", \"removed\"]\non_failure = \"block\"\ndebounce_ms = 500\n[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"traefik/{{ route.slug }}.yml\"\n[outputs.vars]\nentrypoints = [\"websecure\"]\n[[outputs]]\nname = \"disabled\"\nenabled = false\n",
        )
        .expect("config");

        let outputs = config.effective_outputs().expect("effective outputs");

        assert_eq!(outputs.len(), 1);
        let output = &outputs[0];
        assert_eq!(output.name, "traefik");
        assert_eq!(output.template, "bindport-traefik");
        assert_eq!(output.root.as_deref(), Some(".bindport/generated"));
        assert_eq!(output.target, "traefik/{{ route.slug }}.yml");
        assert_eq!(output.target_host, "host.docker.internal");
        assert_eq!(output.target_scheme, "https");
        assert!(!output.auto_render);
        assert_eq!(
            output.delete_on,
            vec![OutputDeleteState::Stopped, OutputDeleteState::Removed]
        );
        assert_eq!(output.on_failure, OutputFailurePolicy::Block);
        assert_eq!(output.debounce_ms, 500);
        assert_eq!(
            output.vars.get("entrypoints"),
            Some(&serde_json::json!(["websecure"]))
        );
    }

    #[test]
    fn effective_outputs_use_builtin_defaults() {
        let config = parse_config(
            ConfigFormat::Toml,
            "[[outputs]]\nname = \"traefik\"\ntemplate = \"bindport-traefik\"\ntarget = \"{{ route.slug }}.yml\"\n",
        )
        .expect("config");

        let output = config
            .effective_outputs()
            .expect("effective outputs")
            .pop()
            .expect("output");

        assert_eq!(output.root, None);
        assert_eq!(output.target_host, DEFAULT_OUTPUT_TARGET_HOST);
        assert_eq!(output.target_scheme, DEFAULT_OUTPUT_TARGET_SCHEME);
        assert_eq!(output.auto_render, DEFAULT_OUTPUT_AUTO_RENDER);
        assert_eq!(output.delete_on, vec![OutputDeleteState::Removed]);
        assert_eq!(output.on_failure, OutputFailurePolicy::Warn);
        assert_eq!(output.debounce_ms, DEFAULT_OUTPUT_DEBOUNCE_MS);
    }

    #[test]
    fn effective_outputs_report_required_field_errors() {
        let missing_name = BindPortConfig {
            outputs: Some(vec![OutputConfig {
                template: Some(String::from("bindport-traefik")),
                target: Some(String::from("{{ route.slug }}.yml")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        assert!(matches!(
            missing_name.effective_outputs(),
            Err(OutputConfigError::MissingName { index: 0 })
        ));

        let missing_template = BindPortConfig {
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("traefik")),
                target: Some(String::from("{{ route.slug }}.yml")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        assert!(matches!(
            missing_template.effective_outputs(),
            Err(OutputConfigError::MissingTemplate { name }) if name == "traefik"
        ));

        let missing_target = BindPortConfig {
            outputs: Some(vec![OutputConfig {
                name: Some(String::from("traefik")),
                template: Some(String::from("bindport-traefik")),
                ..OutputConfig::default()
            }]),
            ..BindPortConfig::default()
        };
        let error = missing_target
            .effective_outputs()
            .expect_err("missing target error");
        assert_eq!(
            error.to_string(),
            "output `traefik` is missing required `target`"
        );

        let duplicate = BindPortConfig {
            outputs: Some(vec![
                OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("{{ route.slug }}.yml")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    target: Some(String::from("{{ route.slug }}.yml")),
                    ..OutputConfig::default()
                },
            ]),
            ..BindPortConfig::default()
        };
        let error = duplicate.effective_outputs().expect_err("duplicate error");
        assert_eq!(
            error.to_string(),
            "output `traefik` is defined more than once"
        );
    }

    #[test]
    fn validate_reports_service_and_output_errors() {
        let config = BindPortConfig {
            services: Some(vec![
                ServiceConfig {
                    path: Some(String::from("../api")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("worker")),
                    path: Some(String::from(" ")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("web")),
                    path: Some(String::from("/tmp/web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("web")),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("args-only")),
                    args: Some(vec![String::from("--port")]),
                    ..ServiceConfig::default()
                },
                ServiceConfig {
                    name: Some(String::from("empty-command")),
                    command: Some(Vec::new()),
                    ..ServiceConfig::default()
                },
            ]),
            outputs: Some(vec![
                OutputConfig {
                    name: Some(String::from("traefik")),
                    template: Some(String::from("bindport-traefik")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    name: Some(String::from("debug")),
                    target: Some(String::from("debug/{{ route.slug }}.txt")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    name: Some(String::from("debug")),
                    template: Some(String::from("debug-route")),
                    target: Some(String::from("debug/{{ route.slug }}.txt")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    enabled: Some(false),
                    name: Some(String::from("disabled")),
                    ..OutputConfig::default()
                },
                OutputConfig {
                    template: Some(String::from("nameless")),
                    target: Some(String::from("nameless.txt")),
                    ..OutputConfig::default()
                },
            ]),
            hooks: Some(HooksConfig {
                timeout_ms: Some(0),
                commands: Some(vec![
                    HookCommandConfig {
                        name: Some(String::from("reload")),
                        events: Some(Vec::new()),
                        command: Some(vec![String::from(" ")]),
                        timeout_ms: Some(0),
                        ..HookCommandConfig::default()
                    },
                    HookCommandConfig {
                        name: Some(String::from("reload")),
                        command: Some(vec![String::from("true")]),
                        ..HookCommandConfig::default()
                    },
                    HookCommandConfig {
                        name: Some(String::from("missing-command")),
                        events: Some(vec![HookEvent::RouteStarted]),
                        ..HookCommandConfig::default()
                    },
                    HookCommandConfig {
                        enabled: Some(false),
                        name: Some(String::from("disabled-placeholder")),
                        ..HookCommandConfig::default()
                    },
                ]),
            }),
            ..BindPortConfig::default()
        };

        let issues = config.validate();

        assert_eq!(issues.len(), 18);
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].name" && issue.message == "service name is required"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[0].path"
                && issue
                    .message
                    .contains("must be relative to the config file")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[1].path" && issue.message == "service path must not be empty"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[2].path"
                && issue
                    .message
                    .contains("must be relative to the config file")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[3].name"
                && issue.message.contains("duplicate service name `web`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[4].args"
                && issue.message == "service args require a service command"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "services[5].command"
                && issue.message == "service command must start with a program"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[0].target"
                && issue
                    .message
                    .contains("output `traefik` is missing required `target`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[1].template"
                && issue
                    .message
                    .contains("output `debug` is missing required `template`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[2].name"
                && issue.message.contains("duplicate output name `debug`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "outputs[4].name" && issue.message == "output name is required"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.timeout_ms"
                && issue.message == "hook timeout must be greater than 0"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[0].command"
                && issue.message == "hook command must start with a program"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[0].events"
                && issue.message == "hook events must not be empty"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[0].timeout_ms"
                && issue.message == "hook timeout must be greater than 0"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[1].name"
                && issue.message.contains("duplicate hook name `reload`")
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[1].events" && issue.message == "hook events are required"
        }));
        assert!(issues.iter().any(|issue| {
            issue.field == "hooks.commands[2].command"
                && issue.message == "hook command is required"
        }));
        assert!(BindPortConfig::default().validate().is_empty());
        assert_eq!(
            ConfigValidationIssue::new("field", "message").to_string(),
            "field: message"
        );
    }

    #[test]
    fn local_override_filenames_preserve_format_precedence() {
        assert_eq!(
            LOCAL_CONFIG_FILENAMES,
            [
                ".bindport.local.toml",
                ".bindport.local.json",
                ".bindport.local.yaml",
                ".bindport.local.yml",
                "bindport.local.toml",
                "bindport.local.json",
                "bindport.local.yaml",
                "bindport.local.yml"
            ]
        );
    }

    #[test]
    fn reports_unknown_top_level_config_keys() {
        let keys = unknown_top_level_config_keys(
            ConfigFormat::Toml,
            "project = \"demo\"\ndefaultrange = \"29100-29199\"\n[proxy.traefik]\nenabled = true\n",
        )
        .expect("unknown keys");

        assert_eq!(keys, ["defaultrange", "proxy"]);
        assert_eq!(
            unknown_top_level_config_keys(ConfigFormat::Json, "[]").expect("json array"),
            Vec::<String>::new()
        );
        assert_eq!(
            unknown_top_level_config_keys(ConfigFormat::Yaml, "[]").expect("yaml array"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn normalizes_branch_labels_for_hostnames() {
        assert_eq!(normalize_branch_label("feature/tree"), "feature-tree");
        assert_eq!(
            normalize_branch_label("BUGFIX/JIRA-123_widget"),
            "bugfix-jira-123-widget"
        );
        assert_eq!(normalize_branch_label("!!!"), "branch");
    }

    #[test]
    fn identity_sources_follow_precedence() {
        let cwd = Path::new("/tmp/bindport");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: None,
            cli_service: Some("cli-service"),
            env_project: Some("env-project"),
            env_service: Some("env-service"),
            config_project: Some("config-project"),
            config_service: Some("config-service"),
        });

        assert_eq!(identity.project, "env-project");
        assert_eq!(identity.service, "cli-service");
    }

    #[test]
    fn config_identity_beats_inference() {
        let cwd = Path::new("/tmp/bindport");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: Some("config-project"),
            config_service: Some("config-service"),
        });

        assert_eq!(identity.project, "config-project");
        assert_eq!(identity.service, "config-service");
    }

    #[test]
    fn package_metadata_infers_standalone_identity() {
        let root = temp_test_dir("package-standalone");
        fs::write(root.join("package.json"), r#"{"name":"@stutz/hoststamp"}"#)
            .expect("write package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "hoststamp");
        assert_eq!(identity.service, "hoststamp");
    }

    #[test]
    fn package_workspaces_infer_root_project_without_git() {
        let root = temp_test_dir("package-workspaces-root");
        fs::write(
            root.join("package.json"),
            r#"{"name":"orderful","workspaces":["apps/*"]}"#,
        )
        .expect("write root package json");
        let api = root.join("apps").join("api");
        let api_src = api.join("src");
        fs::create_dir_all(&api_src).expect("api src");
        fs::write(api.join("package.json"), r#"{"name":"@orderful/api"}"#)
            .expect("write api package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &api_src,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "orderful");
        assert_eq!(identity.service, "api");
    }

    #[test]
    fn package_workspace_object_infers_root_project() {
        let root = temp_test_dir("package-workspace-object");
        fs::write(
            root.join("package.json"),
            r#"{"name":"hoststamp","workspaces":{"packages":["packages/*"]}}"#,
        )
        .expect("write root package json");
        let web = root.join("packages").join("web");
        fs::create_dir_all(&web).expect("web dir");
        fs::write(web.join("package.json"), r#"{"name":"@hoststamp/web"}"#)
            .expect("write web package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &web,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "hoststamp");
        assert_eq!(identity.service, "web");
    }

    #[test]
    fn pnpm_workspace_yaml_infers_root_project_without_git() {
        let root = temp_test_dir("pnpm-workspace-root");
        fs::write(root.join("package.json"), r#"{"name":"orderful"}"#)
            .expect("write root package json");
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n")
            .expect("write pnpm workspace");
        let web = root.join("apps").join("web");
        fs::create_dir_all(&web).expect("web dir");
        fs::write(web.join("package.json"), r#"{"name":"@orderful/web"}"#)
            .expect("write web package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &web,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "orderful");
        assert_eq!(identity.service, "web");
    }

    #[test]
    fn package_workspace_root_beats_outer_git_root_package() {
        let root = temp_test_dir("workspace-below-git-root");
        git(&root, ["init"]);
        git(&root, ["config", "user.email", "bindport@example.invalid"]);
        git(&root, ["config", "user.name", "BindPort Test"]);
        git(&root, ["config", "commit.gpgsign", "false"]);
        fs::write(root.join("package.json"), r#"{"name":"outer"}"#)
            .expect("write outer package json");
        let workspace = root.join("frontend");
        fs::create_dir_all(&workspace).expect("workspace dir");
        fs::write(
            workspace.join("package.json"),
            r#"{"name":"orderful","workspaces":["apps/*"]}"#,
        )
        .expect("write workspace package json");
        let web = workspace.join("apps").join("web");
        fs::create_dir_all(&web).expect("web dir");
        fs::write(web.join("package.json"), r#"{"name":"@orderful/web"}"#)
            .expect("write web package json");
        fs::write(root.join("README.md"), "test\n").expect("write fixture");
        git(
            &root,
            [
                "add",
                "README.md",
                "package.json",
                "frontend/package.json",
                "frontend/apps/web/package.json",
            ],
        );
        git(&root, ["commit", "-m", "initial"]);
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &web,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "orderful");
        assert_eq!(identity.service, "web");
        assert!(identity.git.is_some());
    }

    #[test]
    fn package_metadata_uses_git_root_project_and_nearest_service() {
        let root = temp_test_dir("package-monorepo");
        git(&root, ["init"]);
        git(&root, ["config", "user.email", "bindport@example.invalid"]);
        git(&root, ["config", "user.name", "BindPort Test"]);
        git(&root, ["config", "commit.gpgsign", "false"]);
        fs::write(root.join("package.json"), r#"{"name":"hoststamp"}"#)
            .expect("write root package json");
        let service = root.join("apps").join("web");
        fs::create_dir_all(&service).expect("service dir");
        fs::write(service.join("package.json"), r#"{"name":"@hoststamp/web"}"#)
            .expect("write service package json");
        fs::write(root.join("README.md"), "test\n").expect("write fixture");
        git(
            &root,
            ["add", "README.md", "package.json", "apps/web/package.json"],
        );
        git(&root, ["commit", "-m", "initial"]);
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &service,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(identity.project, "hoststamp");
        assert_eq!(identity.service, "web");
        assert!(identity.git.is_some());
    }

    #[test]
    fn explicit_identity_beats_package_metadata() {
        let root = temp_test_dir("package-explicit");
        fs::write(root.join("package.json"), r#"{"name":"package-project"}"#)
            .expect("write package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &command,
            cli_project: None,
            cli_service: Some("cli-service"),
            env_project: Some("env-project"),
            env_service: Some("env-service"),
            config_project: Some("config-project"),
            config_service: Some("config-service"),
        });

        assert_eq!(identity.project, "env-project");
        assert_eq!(identity.service, "cli-service");
    }

    #[test]
    fn invalid_package_metadata_falls_back_to_directory_and_command() {
        let root = temp_test_dir("package-invalid");
        fs::write(root.join("package.json"), r#"{"name":""}"#).expect("write package json");
        let command = [String::from("next")];

        let identity = resolve_identity(IdentitySources {
            cwd: &root,
            command: &command,
            cli_project: None,
            cli_service: None,
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_eq!(
            identity.project,
            root.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(identity.service, "next");
    }

    #[test]
    fn package_identity_handles_scoped_names_and_workspace_fallbacks() {
        assert_eq!(
            package_identity_name("@scope/web"),
            Some(String::from("web"))
        );
        assert_eq!(package_identity_name("@scope/"), None);
        assert_eq!(package_identity_name(" "), None);
        assert_eq!(
            directory_identity_name(Path::new("/")),
            String::from("workspace")
        );

        let root = temp_test_dir("workspace-name-fallback");
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n")
            .expect("write pnpm workspace");
        let metadata = workspace_root_metadata(&root);
        assert_eq!(
            metadata.identity_name,
            root.file_name().unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn identity_key_delimits_project_and_service_values() {
        let cwd = Path::new("/tmp/bindport");
        let command = [String::from("next")];
        let first = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: Some("a:b"),
            cli_service: Some("c"),
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });
        let second = resolve_identity(IdentitySources {
            cwd,
            command: &command,
            cli_project: Some("a"),
            cli_service: Some("b:c"),
            env_project: None,
            env_service: None,
            config_project: None,
            config_service: None,
        });

        assert_ne!(first.identity_key, second.identity_key);
        assert!(first.identity_key.starts_with("v1:"));
    }

    #[test]
    fn identity_port_scan_start_is_stable_and_in_range() {
        let identity = ServiceIdentity {
            project: String::from("bindport"),
            service: String::from("web"),
            git: None,
            identity_key: String::from("v1:test"),
        };
        let range = PortRange {
            start: 29_100,
            end: 29_199,
        };
        let scan_start = identity.port_scan_start(range).expect("scan start");

        assert!(range.contains(scan_start));
        assert_eq!(identity.port_scan_start(range), Some(scan_start));
        assert_eq!(
            identity.port_scan_start(PortRange { start: 100, end: 0 }),
            None
        );
    }

    #[test]
    fn detects_git_worktree_branch_and_commit() {
        let root = temp_test_dir("git-identity");
        git(&root, ["init"]);
        git(&root, ["config", "user.email", "bindport@example.invalid"]);
        git(&root, ["config", "user.name", "BindPort Test"]);
        git(&root, ["config", "commit.gpgsign", "false"]);
        fs::write(root.join("README.md"), "test\n").expect("write fixture");
        git(&root, ["add", "README.md"]);
        git(&root, ["commit", "-m", "initial"]);
        git(&root, ["checkout", "-B", "feature/tree"]);
        let nested = root.join("apps").join("web");
        fs::create_dir_all(&nested).expect("nested dir");

        let identity = detect_git_identity(&nested).expect("git identity");

        assert_eq!(identity.worktree_path, root.canonicalize().expect("root"));
        assert_eq!(identity.branch, "feature/tree");
        assert_eq!(identity.branch_label, "feature-tree");
        assert!(!identity.commit.is_empty());
        assert!(!identity.worktree_hash.is_empty());
    }

    #[test]
    fn parses_port_range() {
        assert_eq!(
            parse_port_range("29100-29199").expect("range"),
            PortRange {
                start: 29_100,
                end: 29_199
            }
        );
        assert_eq!(
            parse_port_range("29100")
                .expect_err("missing separator")
                .to_string(),
            "expected START-END"
        );
        assert_eq!(
            parse_port_range("start-29199")
                .expect_err("invalid start")
                .to_string(),
            "invalid range start `start`"
        );
        assert_eq!(
            parse_port_range("29100-end")
                .expect_err("invalid end")
                .to_string(),
            "invalid range end `end`"
        );
        assert!(matches!(
            parse_port_range("29199-29100"),
            Err(PortRangeParseError::Empty(_))
        ));
    }

    #[test]
    fn config_errors_preserve_display_and_sources() {
        let path = PathBuf::from("/tmp/bindport.toml");
        let read = ConfigError::Read {
            path: path.clone(),
            source: io::Error::new(io::ErrorKind::NotFound, "missing"),
        };
        assert!(read.to_string().contains("failed to read config"));
        assert!(std::error::Error::source(&read).is_some());

        let unknown = ConfigError::UnknownFormat {
            path: PathBuf::from("/tmp/bindport.txt"),
        };
        assert_eq!(
            unknown.to_string(),
            "unsupported config format `/tmp/bindport.txt`"
        );
        assert!(std::error::Error::source(&unknown).is_none());

        let parse = ConfigError::Parse {
            path: path.clone(),
            format: ConfigFormat::Json,
            source: String::from("bad json"),
        };
        assert!(
            parse
                .to_string()
                .contains("failed to parse json config `/tmp/bindport.toml`")
        );
        assert!(std::error::Error::source(&parse).is_none());

        let range = ConfigError::InvalidPortRange {
            path,
            source: PortRangeParseError::MissingSeparator,
        };
        assert!(
            range
                .to_string()
                .contains("invalid default_range in config `/tmp/bindport.toml`")
        );
        assert!(std::error::Error::source(&range).is_some());
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bindport-core-{name}-{}-{now}", std::process::id()));

        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .expect("run git");

        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
