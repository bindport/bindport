#[derive(Debug)]
pub(crate) struct EnvTemplate {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) configured: bool,
}

#[derive(Debug, Default)]
pub(crate) struct RunTemplates {
    pub(crate) command: Option<Vec<String>>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<EnvTemplate>,
}

#[derive(Debug)]
pub(crate) struct RunMetadata {
    pub(crate) command: Option<Vec<String>>,
    pub(crate) hostname: Option<String>,
    pub(crate) route_url: Option<String>,
    pub(crate) health_url: Option<String>,
    pub(crate) env: Vec<(String, String)>,
}
