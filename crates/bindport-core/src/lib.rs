// SPDX-License-Identifier: MIT

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
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
pub const APPLIED_CONFIG_KEYS: &[&str] = &["project", "default_range", "skip_ports"];

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
    pub default_range: Option<String>,
    pub skip_ports: Option<Vec<u16>>,
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
}
