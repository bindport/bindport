use super::*;

pub(crate) fn env_dashboard_host() -> Result<Option<Ipv4Addr>, DashboardCommandError> {
    std::env::var(DASHBOARD_HOST_ENV)
        .ok()
        .map(|value| {
            value.parse::<Ipv4Addr>().map_err(|_| {
                DashboardCommandError::InvalidArgument(format!(
                    "invalid {DASHBOARD_HOST_ENV} host `{value}`"
                ))
            })
        })
        .transpose()
}

pub(crate) fn env_dashboard_port() -> Result<Option<u16>, DashboardCommandError> {
    std::env::var(DASHBOARD_PORT_ENV)
        .ok()
        .map(|value| {
            value.parse::<u16>().map_err(|_| {
                DashboardCommandError::InvalidArgument(format!(
                    "invalid {DASHBOARD_PORT_ENV} port `{value}`"
                ))
            })
        })
        .transpose()
}

pub(crate) fn env_dashboard_auth_required() -> Result<Option<bool>, DashboardCommandError> {
    std::env::var(DASHBOARD_AUTH_REQUIRED_ENV)
        .ok()
        .map(|value| parse_dashboard_auth_mode(&value))
        .transpose()
}

pub(crate) fn env_dashboard_register_service() -> Result<Option<bool>, DashboardCommandError> {
    std::env::var(DASHBOARD_REGISTER_SERVICE_ENV)
        .ok()
        .map(|value| parse_dashboard_bool(&value, DASHBOARD_REGISTER_SERVICE_ENV))
        .transpose()
}
