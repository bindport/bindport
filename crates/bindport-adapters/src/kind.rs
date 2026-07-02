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
