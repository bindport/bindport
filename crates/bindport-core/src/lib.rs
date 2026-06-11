// SPDX-License-Identifier: MIT

use std::{
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
pub const CONFIG_FILENAMES: &[&str] = &[".bindport.toml", ".bindport.json", ".bindport.yaml"];
pub const FALLBACK_CONFIG_FILE: &str = "config.toml";
pub const APPLIED_CONFIG_KEYS: &[&str] = &["project", "service", "default_range", "skip_ports"];
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
    pub config: BindPortConfig,
    pub unknown_keys: Vec<String>,
}

impl LoadedConfig {
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
                return load_config(path, ConfigSource::Project).map(Some);
            }
        }
    }

    if let Some(path) = fallback_path.filter(|path| path.is_file()) {
        return load_config(path, ConfigSource::Fallback).map(Some);
    }

    Ok(None)
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

fn package_inference(cwd: &Path, git: Option<&GitIdentity>) -> PackageInference {
    let root = git.and_then(|git| read_package_metadata(&git.worktree_path));
    let nearest = nearest_package_metadata(cwd, git.map(|git| git.worktree_path.as_path()));

    PackageInference { root, nearest }
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
         skip_ports = [{}]\n",
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
            "project = \"demo\"\ndefault_range = \"29100-29199\"\nskip_ports = [29100]\n",
        )
        .expect("toml config");
        let json = parse_config(
            ConfigFormat::Json,
            r#"{"project":"demo","default_range":"29100-29199","skip_ports":[29100]}"#,
        )
        .expect("json config");
        let yaml = parse_config(
            ConfigFormat::Yaml,
            "project: demo\ndefault_range: 29100-29199\nskip_ports:\n  - 29100\n",
        )
        .expect("yaml config");

        assert_eq!(toml, json);
        assert_eq!(json, yaml);
    }

    #[test]
    fn reports_unknown_top_level_config_keys() {
        let keys = unknown_top_level_config_keys(
            ConfigFormat::Toml,
            "project = \"demo\"\ndefaultrange = \"29100-29199\"\n[proxy.traefik]\nenabled = true\n",
        )
        .expect("unknown keys");

        assert_eq!(keys, ["defaultrange", "proxy"]);
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
        assert!(matches!(
            parse_port_range("29199-29100"),
            Err(PortRangeParseError::Empty(_))
        ));
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
