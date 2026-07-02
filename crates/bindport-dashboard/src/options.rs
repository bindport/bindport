use super::*;

pub type DashboardCleanCallback =
    Arc<dyn Fn(&mut Registry, CleanSummary) -> Result<(), String> + Send + Sync + 'static>;
pub type DashboardStatusCallback = Arc<dyn Fn() -> serde_json::Value + Send + Sync + 'static>;

#[derive(Clone)]
pub struct DashboardOptions {
    pub host: Ipv4Addr,
    pub preferred_port: u16,
    pub fallback_range: PortRange,
    pub skip_ports: Vec<u16>,
    pub allowed_hosts: Vec<String>,
    pub auth: DashboardAuth,
    pub static_dir: Option<PathBuf>,
    pub clean_callback: Option<DashboardCleanCallback>,
    pub status_callback: Option<DashboardStatusCallback>,
}

impl fmt::Debug for DashboardOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DashboardOptions")
            .field("host", &self.host)
            .field("preferred_port", &self.preferred_port)
            .field("fallback_range", &self.fallback_range)
            .field("skip_ports", &self.skip_ports)
            .field("allowed_hosts", &self.allowed_hosts)
            .field("auth", &self.auth)
            .field("static_dir", &self.static_dir)
            .field("clean_callback", &self.clean_callback.is_some())
            .field("status_callback", &self.status_callback.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DashboardAuth {
    pub required: bool,
    pub token: Option<String>,
}

impl Default for DashboardOptions {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::LOCALHOST,
            preferred_port: DEFAULT_DASHBOARD_PORT,
            fallback_range: DEFAULT_PORT_RANGE,
            skip_ports: DEFAULT_SKIP_PORTS.to_vec(),
            allowed_hosts: default_allowed_hosts(),
            auth: DashboardAuth::default(),
            static_dir: None,
            clean_callback: None,
            status_callback: None,
        }
    }
}

pub(crate) fn default_allowed_hosts() -> Vec<String> {
    vec![String::from("localhost"), Ipv4Addr::LOCALHOST.to_string()]
}
