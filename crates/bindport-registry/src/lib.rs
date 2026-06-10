// SPDX-License-Identifier: MIT

use bindport_core::SERVICE_NAME;

pub const DEFAULT_REGISTRY_FILE: &str = "registry.sqlite";

pub fn default_registry_directory_name() -> &'static str {
    SERVICE_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_defaults_are_named_for_bindport() {
        assert_eq!(default_registry_directory_name(), "bindport");
        assert_eq!(DEFAULT_REGISTRY_FILE, "registry.sqlite");
    }
}
