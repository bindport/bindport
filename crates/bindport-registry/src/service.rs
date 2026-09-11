use super::*;

mod reservation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryService {
    pub lease_id: i64,
    pub project: String,
    pub service: String,
    pub identity_key: String,
    pub state: String,
    pub host: String,
    pub port: u16,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservationCandidate {
    pub host: String,
    pub port: u16,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
}

#[derive(Debug)]
pub enum BatchReservationError<E> {
    Registry(RegistryError),
    Plan(E),
}

impl<E> From<RegistryError> for BatchReservationError<E> {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

#[derive(Debug, Clone)]
pub struct ReservedRunStart {
    pub lease_id: i64,
    pub pid: u32,
    pub command: String,
    pub cwd: PathBuf,
}

impl Registry {
    pub fn select_service(
        &mut self,
        identity: &ServiceIdentity,
    ) -> Result<RegistryService, RegistryError> {
        self.select_services(std::slice::from_ref(identity))
            .map(|mut services| services.remove(0))
    }

    pub fn select_services(
        &mut self,
        identities: &[ServiceIdentity],
    ) -> Result<Vec<RegistryService>, RegistryError> {
        self.reconcile_stale_active_leases()?;
        let transaction = self.connection.transaction()?;
        let services = identities
            .iter()
            .map(|identity| {
                select_scoped_service(&transaction, identity)?.ok_or_else(|| {
                    RegistryError::ServiceNotFound {
                        project: identity.project.clone(),
                        service: identity.service.clone(),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;

        Ok(services)
    }

    pub fn promote_reserved_lease(
        &mut self,
        run: &ReservedRunStart,
    ) -> Result<StartedRun, RegistryError> {
        self.promote_reserved_lease_inner(run, None)
    }

    pub fn promote_reserved_lease_with_metadata(
        &mut self,
        run: &ReservedRunStart,
        hostname: Option<&str>,
        route_url: Option<&str>,
        health_url: Option<&str>,
    ) -> Result<StartedRun, RegistryError> {
        self.promote_reserved_lease_inner(run, Some((hostname, route_url, health_url)))
    }

    fn promote_reserved_lease_inner(
        &mut self,
        run: &ReservedRunStart,
        metadata: Option<(Option<&str>, Option<&str>, Option<&str>)>,
    ) -> Result<StartedRun, RegistryError> {
        let now = utc_now(&self.connection)?;
        let cwd = run.cwd.display().to_string();
        let process_start_time = process_start_time(run.pid);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = match metadata {
            Some((hostname, route_url, health_url)) => transaction.execute(
                "UPDATE leases
                 SET state = 'active', last_seen_at = ?1, hostname = ?3,
                     route_url = ?4, health_url = ?5
                 WHERE id = ?2 AND state = 'reserved'",
                params![now, run.lease_id, hostname, route_url, health_url],
            )?,
            None => transaction.execute(
                "UPDATE leases
                 SET state = 'active', last_seen_at = ?1
                 WHERE id = ?2 AND state = 'reserved'",
                params![now, run.lease_id],
            )?,
        };
        if updated == 0 {
            return Err(RegistryError::ReservationNotFound {
                lease_id: run.lease_id,
            });
        }

        transaction.execute(
            "INSERT INTO runs (
                lease_id, pid, process_start_time, command, cwd, started_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run.lease_id,
                run.pid,
                process_start_time,
                run.command,
                cwd,
                now
            ],
        )?;
        let started = StartedRun {
            lease_id: run.lease_id,
            run_id: transaction.last_insert_rowid(),
        };

        transaction.commit()?;

        Ok(started)
    }
}

fn select_scoped_service(
    connection: &Connection,
    identity: &ServiceIdentity,
) -> Result<Option<RegistryService>, RegistryError> {
    let mut statement = connection.prepare(
        "SELECT
            id, project, service, identity_key, state, host, port, hostname,
            route_url, health_url
         FROM leases
         WHERE project = ?1
         AND service = ?2
         AND identity_key = ?3
         AND state IN ('active', 'reserved')
         ORDER BY id",
    )?;
    let matches = statement
        .query_map(
            params![identity.project, identity.service, identity.identity_key],
            |row| {
                Ok(RegistryService {
                    lease_id: row.get(0)?,
                    project: row.get(1)?,
                    service: row.get(2)?,
                    identity_key: row.get(3)?,
                    state: row.get(4)?,
                    host: row.get(5)?,
                    port: row.get(6)?,
                    hostname: row.get(7)?,
                    route_url: row.get(8)?,
                    health_url: row.get(9)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    match matches.as_slice() {
        [] => Ok(None),
        [service] => Ok(Some(service.clone())),
        _ => Err(RegistryError::AmbiguousService {
            project: identity.project.clone(),
            service: identity.service.clone(),
        }),
    }
}

fn active_and_reserved_ports(connection: &Connection) -> Result<Vec<u16>, RegistryError> {
    let mut statement = connection.prepare(
        "SELECT port
         FROM leases
         WHERE state IN ('active', 'reserved')",
    )?;
    let rows = statement.query_map([], |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn previous_identity_port(
    connection: &Connection,
    identity_key: &str,
) -> Result<Option<u16>, RegistryError> {
    connection
        .query_row(
            "SELECT port
             FROM leases
             WHERE identity_key = ?1
             ORDER BY last_seen_at DESC, id DESC
             LIMIT 1",
            params![identity_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}
