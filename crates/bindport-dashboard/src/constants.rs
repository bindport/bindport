pub const DEFAULT_DASHBOARD_PORT: u16 = 27_080;
pub(crate) const DASHBOARD_APP_NAME: &str = "BindPort";
pub(crate) const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
pub(crate) const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
pub(crate) const MAX_HEADER_BYTES: usize = 16 * 1024;
pub(crate) const DASHBOARD_ACTION_HEADER: &str = "X-BindPort-Dashboard-Action";
pub(crate) const INDEX_HTML: &str = include_str!("../static/index.html");
pub(crate) const APP_CSS: &str = include_str!("../static/app.css");
pub(crate) const APP_JS: &str = include_str!("../static/app.js");
#[cfg(debug_assertions)]
pub(crate) const DEV_RELOAD_JS: &str = include_str!("../static/dev-reload.js");
