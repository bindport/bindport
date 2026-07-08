use super::*;

const BINDPORT_LOG_ENV: &str = "BINDPORT_LOG";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiagnosticLog {
    enabled: bool,
}

impl DiagnosticLog {
    pub(crate) const fn disabled() -> Self {
        Self { enabled: false }
    }

    pub(crate) const fn enabled() -> Self {
        Self { enabled: true }
    }

    pub(crate) fn from_env() -> Self {
        match env::var(BINDPORT_LOG_ENV) {
            Ok(value) if diagnostic_log_env_value_enabled(&value) => Self::enabled(),
            _ => Self::disabled(),
        }
    }

    pub(crate) fn debug(self, message: fmt::Arguments<'_>) {
        if self.enabled {
            eprintln!("bindport: debug: {message}");
        }
    }
}

pub(crate) fn diagnostic_log_env_value_enabled(value: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .any(|value| matches!(value, "1" | "true" | "yes" | "debug" | "trace" | "verbose"))
}
