// SPDX-License-Identifier: MIT

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Traefik,
}

impl AdapterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traefik => "traefik",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traefik_is_first_adapter_name() {
        assert_eq!(AdapterKind::Traefik.as_str(), "traefik");
    }
}
