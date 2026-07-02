use super::*;

pub fn default_fallback_config() -> String {
    let skip_ports = DEFAULT_SKIP_PORTS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "# BindPort fallback config. Project .bindport.* files discovered upward override this file.\n\
         # This file is optional; BindPort uses built-in defaults when no config exists.\n\
         default_range = \"{}-{}\"\n\
         skip_ports = [{}]\n\
         \n\
         [dashboard]\n\
         host = \"127.0.0.1\"\n\
         port = 27080\n\
         register_service = false\n\
         allowed_hosts = [\"localhost\", \"127.0.0.1\"]\n\
         \n\
         [dashboard.auth]\n\
         required = false\n\
         token_env = \"BINDPORT_DASHBOARD_TOKEN\"\n",
        DEFAULT_PORT_RANGE.start, DEFAULT_PORT_RANGE.end, skip_ports
    )
}
