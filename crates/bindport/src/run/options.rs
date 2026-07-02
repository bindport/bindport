#[derive(Debug, Default)]
pub(crate) struct RunOptions {
    pub(crate) service: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}
