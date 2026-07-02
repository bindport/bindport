use super::*;

pub(crate) fn utc_now(connection: &Connection) -> Result<String, RegistryError> {
    connection
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |row| {
            row.get(0)
        })
        .map_err(Into::into)
}
