use super::*;

#[derive(Debug, Clone)]
pub struct RunStart {
    pub project: String,
    pub service: String,
    pub identity: Option<ServiceIdentity>,
    pub host: String,
    pub port: u16,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
    pub pid: u32,
    pub command: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartedRun {
    pub lease_id: i64,
    pub run_id: i64,
}

#[derive(Debug, Clone)]
pub struct ReserveLease {
    pub project: String,
    pub service: String,
    pub identity: Option<ServiceIdentity>,
    pub host: String,
    pub port: u16,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedLease {
    pub lease_id: i64,
    pub project: String,
    pub service: String,
    pub port: u16,
    pub host: String,
    pub route_url: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ActiveRun {
    pub(crate) lease_id: i64,
    pub(crate) run_id: i64,
    pub(crate) pid: u32,
    pub(crate) process_start_time: Option<i64>,
    pub(crate) command: String,
}

impl Registry {
    pub fn active_ports(&mut self) -> Result<Vec<u16>, RegistryError> {
        self.reconcile_stale_active_leases()?;

        let mut statement = self.connection.prepare(
            "SELECT port
             FROM leases
             WHERE state IN ('active', 'reserved')",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, u16>(0))?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn previous_identity_port(
        &mut self,
        identity_key: &str,
    ) -> Result<Option<u16>, RegistryError> {
        if identity_key.is_empty() {
            return Ok(None);
        }

        self.reconcile_stale_active_leases()?;

        self.connection
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

    pub fn reserved_identity_lease(
        &mut self,
        identity_key: &str,
    ) -> Result<Option<ReservedLease>, RegistryError> {
        if identity_key.is_empty() {
            return Ok(None);
        }

        self.reconcile_stale_active_leases()?;

        self.connection
            .query_row(
                "SELECT id, project, service, port, host, route_url
                 FROM leases
                 WHERE identity_key = ?1
                 AND state = 'reserved'
                 ORDER BY last_seen_at DESC, id DESC
                 LIMIT 1",
                params![identity_key],
                |row| {
                    Ok(ReservedLease {
                        lease_id: row.get(0)?,
                        project: row.get(1)?,
                        service: row.get(2)?,
                        port: row.get(3)?,
                        host: row.get(4)?,
                        route_url: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_reserved_lease(
        &mut self,
        lease: &ReserveLease,
    ) -> Result<ReservedLease, RegistryError> {
        let now = utc_now(&self.connection)?;
        let identity = lease.identity.as_ref();
        let git = identity.and_then(|identity| identity.git.as_ref());
        let worktree_path = git.map(|git| git.worktree_path.display().to_string());
        let worktree_hash = git.map(|git| git.worktree_hash.as_str());
        let git_common_dir = git.map(|git| git.git_common_dir.display().to_string());
        let branch = git.map(|git| git.branch.as_str());
        let branch_label = git.map(|git| git.branch_label.as_str());
        let git_commit = git.map(|git| git.commit.as_str());
        let identity_key = identity.map(|identity| identity.identity_key.as_str());
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let port_in_use = transaction
            .query_row(
                "SELECT 1
                 FROM leases
                 WHERE port = ?1
                 AND state IN ('active', 'reserved')
                 LIMIT 1",
                params![lease.port],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if port_in_use {
            return Err(RegistryError::PortConflict { port: lease.port });
        }

        transaction.execute(
            "INSERT INTO leases (
                project, service, worktree_path, worktree_hash, git_common_dir,
                branch, branch_label, git_commit, identity_key, port, host,
                hostname, route_url, health_url, state,
                allocated_at, last_seen_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, 'reserved', ?15, ?15
             )",
            params![
                lease.project,
                lease.service,
                worktree_path,
                worktree_hash,
                git_common_dir,
                branch,
                branch_label,
                git_commit,
                identity_key,
                lease.port,
                lease.host,
                lease.hostname,
                lease.route_url,
                lease.health_url,
                now
            ],
        )?;
        let lease_id = transaction.last_insert_rowid();

        transaction.commit()?;

        Ok(ReservedLease {
            lease_id,
            project: lease.project.clone(),
            service: lease.service.clone(),
            port: lease.port,
            host: lease.host.clone(),
            route_url: lease.route_url.clone(),
        })
    }

    pub fn release_reserved_port(
        &mut self,
        port: u16,
    ) -> Result<Option<ReservedLease>, RegistryError> {
        self.release_reserved_lease(
            "SELECT id, project, service, port, host, route_url
             FROM leases
             WHERE port = ?1
             AND state = 'reserved'
             ORDER BY last_seen_at DESC, id DESC
             LIMIT 1",
            params![port],
        )
    }

    pub fn release_reserved_identity(
        &mut self,
        identity_key: &str,
    ) -> Result<Option<ReservedLease>, RegistryError> {
        self.release_reserved_lease(
            "SELECT id, project, service, port, host, route_url
             FROM leases
             WHERE identity_key = ?1
             AND state = 'reserved'
             ORDER BY last_seen_at DESC, id DESC
             LIMIT 1",
            params![identity_key],
        )
    }

    fn release_reserved_lease<P>(
        &mut self,
        query: &str,
        params: P,
    ) -> Result<Option<ReservedLease>, RegistryError>
    where
        P: rusqlite::Params,
    {
        self.reconcile_stale_active_leases()?;

        let Some(lease) = self
            .connection
            .query_row(query, params, |row| {
                Ok(ReservedLease {
                    lease_id: row.get(0)?,
                    project: row.get(1)?,
                    service: row.get(2)?,
                    port: row.get(3)?,
                    host: row.get(4)?,
                    route_url: row.get(5)?,
                })
            })
            .optional()?
        else {
            return Ok(None);
        };

        let now = utc_now(&self.connection)?;
        let updated = self.connection.execute(
            "UPDATE leases
             SET state = 'stopped', last_seen_at = ?1, released_at = ?1
             WHERE id = ?2 AND state = 'reserved'",
            params![now, lease.lease_id],
        )?;
        if updated == 0 {
            return Ok(None);
        }

        Ok(Some(lease))
    }

    pub fn reconcile_stale_active_leases(&mut self) -> Result<usize, RegistryError> {
        let active_runs = self.active_runs()?;
        let stale_runs = active_runs
            .into_iter()
            .filter(|run| !active_run_process_matches(run))
            .collect::<Vec<_>>();

        self.mark_observed_runs_stale(&stale_runs)
    }

    pub(crate) fn mark_observed_runs_stale(
        &mut self,
        stale_runs: &[ActiveRun],
    ) -> Result<usize, RegistryError> {
        if stale_runs.is_empty() {
            return Ok(0);
        }

        let now = utc_now(&self.connection)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut transitioned = 0;

        for stale_run in stale_runs {
            let updated = transaction.execute(
                "UPDATE leases
                 SET state = 'stale', last_seen_at = ?1, released_at = ?1
                 WHERE id = ?2 AND state = 'active'",
                params![now, stale_run.lease_id],
            )?;
            if updated == 0 {
                continue;
            }
            transaction.execute(
                "UPDATE runs
                 SET exited_at = COALESCE(exited_at, ?1)
                 WHERE id = ?2 AND exited_at IS NULL",
                params![now, stale_run.run_id],
            )?;
            transitioned += updated;
        }

        transaction.commit()?;

        Ok(transitioned)
    }

    pub fn record_run_started(&mut self, run: &RunStart) -> Result<StartedRun, RegistryError> {
        let now = utc_now(&self.connection)?;
        let cwd = run.cwd.display().to_string();
        let identity = run.identity.as_ref();
        let git = identity.and_then(|identity| identity.git.as_ref());
        let worktree_path = git.map(|git| git.worktree_path.display().to_string());
        let worktree_hash = git.map(|git| git.worktree_hash.as_str());
        let git_common_dir = git.map(|git| git.git_common_dir.display().to_string());
        let branch = git.map(|git| git.branch.as_str());
        let branch_label = git.map(|git| git.branch_label.as_str());
        let git_commit = git.map(|git| git.commit.as_str());
        let identity_key = identity.map(|identity| identity.identity_key.as_str());
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let port_in_use = transaction
            .query_row(
                "SELECT 1
                 FROM leases
                 WHERE port = ?1
                 AND state IN ('active', 'reserved')
                 LIMIT 1",
                params![run.port],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if port_in_use {
            return Err(RegistryError::PortConflict { port: run.port });
        }
        let process_start_time = process_start_time(run.pid);

        transaction.execute(
            "INSERT INTO leases (
                project, service, worktree_path, worktree_hash, git_common_dir,
                branch, branch_label, git_commit, identity_key, port, host,
                hostname, route_url, health_url, state,
                allocated_at, last_seen_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, 'active', ?15, ?15
             )",
            params![
                run.project,
                run.service,
                worktree_path,
                worktree_hash,
                git_common_dir,
                branch,
                branch_label,
                git_commit,
                identity_key,
                run.port,
                run.host,
                run.hostname,
                run.route_url,
                run.health_url,
                now
            ],
        )?;
        let lease_id = transaction.last_insert_rowid();

        transaction.execute(
            "INSERT INTO runs (
                lease_id, pid, process_start_time, command, cwd, started_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![lease_id, run.pid, process_start_time, run.command, cwd, now],
        )?;
        let run_id = transaction.last_insert_rowid();

        transaction.commit()?;

        Ok(StartedRun { lease_id, run_id })
    }

    pub fn record_run_finished(
        &mut self,
        run: StartedRun,
        exit_code: Option<i32>,
    ) -> Result<(), RegistryError> {
        let now = utc_now(&self.connection)?;
        let transaction = self.connection.transaction()?;

        transaction.execute(
            "UPDATE runs
             SET exited_at = ?1, exit_code = ?2
             WHERE id = ?3",
            params![now, exit_code, run.run_id],
        )?;
        transaction.execute(
            "UPDATE leases
             SET state = 'stopped', last_seen_at = ?1, released_at = ?1
             WHERE id = ?2",
            params![now, run.lease_id],
        )?;

        transaction.commit()?;

        Ok(())
    }

    pub fn record_reserved_run_failed(
        &mut self,
        run: StartedRun,
        exit_code: Option<i32>,
    ) -> Result<(), RegistryError> {
        let now = utc_now(&self.connection)?;
        let transaction = self.connection.transaction()?;

        let updated_run = transaction.execute(
            "UPDATE runs
             SET exited_at = ?1, exit_code = ?2
             WHERE id = ?3 AND lease_id = ?4",
            params![now, exit_code, run.run_id, run.lease_id],
        )?;
        let updated_lease = transaction.execute(
            "UPDATE leases
             SET state = 'reserved', last_seen_at = ?1, released_at = NULL
             WHERE id = ?2 AND state IN ('active', 'stale')",
            params![now, run.lease_id],
        )?;
        if updated_run == 0 || updated_lease == 0 {
            return Err(RegistryError::ReservationNotFound {
                lease_id: run.lease_id,
            });
        }

        transaction.commit()?;

        Ok(())
    }

    fn active_runs(&self) -> Result<Vec<ActiveRun>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT leases.id, runs.id, runs.pid, runs.process_start_time, runs.command
             FROM leases
             JOIN runs ON runs.lease_id = leases.id
             WHERE leases.state = 'active'
             AND runs.exited_at IS NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ActiveRun {
                lease_id: row.get(0)?,
                run_id: row.get(1)?,
                pid: row.get(2)?,
                process_start_time: row.get(3)?,
                command: row.get(4)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
