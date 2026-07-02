use super::*;

pub(crate) fn env_dashboard_host() -> Result<Option<Ipv4Addr>, DashboardCommandError> {
    parse_env_dashboard_host(std::env::var(DASHBOARD_HOST_ENV).ok())
}

pub(crate) fn env_dashboard_port() -> Result<Option<u16>, DashboardCommandError> {
    parse_env_dashboard_port(std::env::var(DASHBOARD_PORT_ENV).ok())
}

pub(crate) fn env_dashboard_auth_required() -> Result<Option<bool>, DashboardCommandError> {
    parse_env_dashboard_auth_required(std::env::var(DASHBOARD_AUTH_REQUIRED_ENV).ok())
}

pub(crate) fn env_dashboard_register_service() -> Result<Option<bool>, DashboardCommandError> {
    parse_env_dashboard_register_service(std::env::var(DASHBOARD_REGISTER_SERVICE_ENV).ok())
}

pub(crate) fn parse_env_dashboard_host(
    value: Option<String>,
) -> Result<Option<Ipv4Addr>, DashboardCommandError> {
    value
        .map(|value| {
            value.parse::<Ipv4Addr>().map_err(|_| {
                DashboardCommandError::InvalidArgument(format!(
                    "invalid {DASHBOARD_HOST_ENV} host `{value}`"
                ))
            })
        })
        .transpose()
}

pub(crate) fn parse_env_dashboard_port(
    value: Option<String>,
) -> Result<Option<u16>, DashboardCommandError> {
    value
        .map(|value| {
            value.parse::<u16>().map_err(|_| {
                DashboardCommandError::InvalidArgument(format!(
                    "invalid {DASHBOARD_PORT_ENV} port `{value}`"
                ))
            })
        })
        .transpose()
}

pub(crate) fn parse_env_dashboard_auth_required(
    value: Option<String>,
) -> Result<Option<bool>, DashboardCommandError> {
    value
        .map(|value| parse_dashboard_auth_mode(&value))
        .transpose()
}

pub(crate) fn parse_env_dashboard_register_service(
    value: Option<String>,
) -> Result<Option<bool>, DashboardCommandError> {
    value
        .map(|value| parse_dashboard_bool(&value, DASHBOARD_REGISTER_SERVICE_ENV))
        .transpose()
}
