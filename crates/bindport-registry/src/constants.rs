use super::*;

pub const DEFAULT_REGISTRY_FILE: &str = "registry.sqlite";
pub const REGISTRY_PATH_ENV: &str = "BINDPORT_REGISTRY_PATH";
pub const STATUS_SCHEMA_VERSION: &str = "0.4";
pub(crate) const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_millis(300);
pub(crate) const HEALTH_PENDING_GRACE_MS: i64 = 2_000;
pub(crate) const REGISTRY_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
