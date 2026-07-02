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

#[derive(Debug)]
pub(crate) struct ActiveRun {
    pub(crate) lease_id: i64,
    pub(crate) run_id: i64,
    pub(crate) pid: u32,
    pub(crate) process_start_time: Option<i64>,
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

    pub fn reconcile_stale_active_leases(&mut self) -> Result<usize, RegistryError> {
        let active_runs = self.active_runs()?;
        let stale_runs = active_runs
            .into_iter()
            .filter(|run| !active_run_process_matches(run))
            .collect::<Vec<_>>();

        if stale_runs.is_empty() {
            return Ok(0);
        }

        let now = utc_now(&self.connection)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        for stale_run in &stale_runs {
            transaction.execute(
                "UPDATE runs
                 SET exited_at = COALESCE(exited_at, ?1)
                 WHERE id = ?2",
                params![now, stale_run.run_id],
            )?;
            transaction.execute(
                "UPDATE leases
                 SET state = 'stale', last_seen_at = ?1, released_at = ?1
                 WHERE id = ?2",
                params![now, stale_run.lease_id],
            )?;
        }

        transaction.commit()?;

        Ok(stale_runs.len())
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

    fn active_runs(&self) -> Result<Vec<ActiveRun>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT leases.id, runs.id, runs.pid, runs.process_start_time
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
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
