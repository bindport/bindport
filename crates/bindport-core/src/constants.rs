use super::*;

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
pub(crate) const MAX_YAML_CONFIG_BYTES: usize = 256 * 1024;
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
