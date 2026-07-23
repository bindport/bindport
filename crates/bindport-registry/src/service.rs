use super::*;

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

    pub fn reserve_services<E>(
        &mut self,
        identities: &[ServiceIdentity],
        mut plan: impl FnMut(&ServiceIdentity, &[u16], Option<u16>) -> Result<ReservationCandidate, E>,
    ) -> Result<Vec<RegistryService>, BatchReservationError<E>> {
        self.reconcile_stale_active_leases()?;
        let now = utc_now(&self.connection)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(RegistryError::from)?;
        let mut occupied_ports = active_and_reserved_ports(&transaction)?;
        let mut services = Vec::with_capacity(identities.len());

        for identity in identities {
            if let Some(service) = select_scoped_service(&transaction, identity)? {
                services.push(service);
                continue;
            }

            let previous_port = previous_identity_port(&transaction, &identity.identity_key)?;
            let candidate = plan(identity, &occupied_ports, previous_port)
                .map_err(BatchReservationError::Plan)?;
            if occupied_ports.contains(&candidate.port) {
                return Err(RegistryError::PortConflict {
                    port: candidate.port,
                }
                .into());
            }

            let git = identity.git.as_ref();
            transaction
                .execute(
                    "INSERT INTO leases (
                        project, service, worktree_path, worktree_hash, git_common_dir,
                        branch, branch_label, git_commit, identity_key, port, host,
                        hostname, route_url, health_url, state, allocated_at, last_seen_at
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                        ?14, 'reserved', ?15, ?15
                     )",
                    params![
                        identity.project,
                        identity.service,
                        git.map(|git| git.worktree_path.display().to_string()),
                        git.map(|git| git.worktree_hash.as_str()),
                        git.map(|git| git.git_common_dir.display().to_string()),
                        git.map(|git| git.branch.as_str()),
                        git.map(|git| git.branch_label.as_str()),
                        git.map(|git| git.commit.as_str()),
                        identity.identity_key,
                        candidate.port,
                        candidate.host,
                        candidate.hostname,
                        candidate.route_url,
                        candidate.health_url,
                        now,
                    ],
                )
                .map_err(RegistryError::from)?;
            let lease_id = transaction.last_insert_rowid();
            occupied_ports.push(candidate.port);
            services.push(RegistryService {
                lease_id,
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity_key: identity.identity_key.clone(),
                state: String::from("reserved"),
                host: candidate.host,
                port: candidate.port,
                hostname: candidate.hostname,
                route_url: candidate.route_url,
                health_url: candidate.health_url,
            });
        }

        transaction.commit().map_err(RegistryError::from)?;

        Ok(services)
    }

    pub fn promote_reserved_lease(
        &mut self,
        run: &ReservedRunStart,
    ) -> Result<StartedRun, RegistryError> {
        let now = utc_now(&self.connection)?;
        let cwd = run.cwd.display().to_string();
        let process_start_time = process_start_time(run.pid);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE leases
             SET state = 'active', last_seen_at = ?1
             WHERE id = ?2 AND state = 'reserved'",
            params![now, run.lease_id],
        )?;
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
