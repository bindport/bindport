use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DashboardCommand {
    Serve,
    Start,
    Status,
    Stop,
    Help,
}

#[derive(Debug, Default)]
pub(crate) struct DashboardCliOptions {
    pub(crate) host: Option<Ipv4Addr>,
    pub(crate) port: Option<u16>,
    pub(crate) auth_required: Option<bool>,
    pub(crate) register_service: Option<bool>,
    pub(crate) token: Option<String>,
    pub(crate) token_env: Option<String>,
    pub(crate) allowed_hosts: Vec<String>,
    pub(crate) static_dir: Option<PathBuf>,
    pub(crate) serve_args: Vec<String>,
}

impl DashboardCliOptions {
    pub(crate) fn token_env_name(&self) -> &str {
        self.token_env.as_deref().unwrap_or(DASHBOARD_TOKEN_ENV)
    }
}
