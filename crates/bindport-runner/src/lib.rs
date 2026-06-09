// SPDX-License-Identifier: MIT

pub const PORT_ENV_VAR: &str = "PORT";

pub fn port_env(port: u16) -> (&'static str, String) {
    (PORT_ENV_VAR, port.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_env_uses_expected_variable_name() {
        assert_eq!(port_env(29_123), ("PORT", String::from("29123")));
    }
}
