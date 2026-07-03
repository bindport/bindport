use super::*;

#[derive(Debug, Serialize)]
pub struct StatusSnapshot {
    pub schema_version: &'static str,
    pub generated_at: String,
    pub outputs: Vec<StatusOutput>,
    pub services: Vec<StatusService>,
    pub runs: Vec<StatusRun>,
}

#[derive(Debug, Serialize)]
pub struct StatusOutput {
    pub name: String,
    pub pending: usize,
    pub rendered: usize,
    pub removed: usize,
    pub error: usize,
}

#[derive(Debug, Serialize)]
pub struct StatusService {
    pub project: String,
    pub service: String,
    pub state: String,
    pub port: u16,
    pub host: String,
    pub url: String,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_hash: Option<String>,
    pub git_common_dir: Option<String>,
    pub branch: Option<String>,
    pub branch_label: Option<String>,
    pub commit: Option<String>,
    pub identity_key: Option<String>,
    pub pid: Option<u32>,
    pub command: String,
    pub cwd: String,
    pub started_at: String,
    pub exited_at: Option<String>,
    pub exit_code: Option<i32>,
    pub health: String,
    pub outputs: Vec<StatusServiceOutput>,
    pub proxy: Option<StatusProxy>,
}

pub(crate) struct StatusServiceRow {
    pub(crate) service: StatusService,
    pub(crate) run_age_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusServiceOutput {
    pub name: String,
    pub status: String,
    pub reason: Option<String>,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct StatusProxy {
    pub adapter: String,
    pub rendered: bool,
    pub target: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusRun {
    pub id: i64,
    pub lease_id: i64,
    pub pid: u32,
    pub command: String,
    pub cwd: String,
    pub started_at: String,
    pub exited_at: Option<String>,
    pub exit_code: Option<i32>,
}

pub fn status_service_route_key(service: &StatusService) -> String {
    service.identity_key.clone().unwrap_or_else(|| {
        format!(
            "{}:{}:{}:{}:{}",
            service.project, service.service, service.host, service.port, service.started_at
        )
    })
}

pub(crate) fn status_proxy_for_outputs(outputs: &[StatusServiceOutput]) -> Option<StatusProxy> {
    outputs
        .iter()
        .find(|output| output.name == "traefik")
        .map(|output| StatusProxy {
            adapter: String::from("traefik"),
            rendered: output.status == OutputFileStatus::Rendered.as_str(),
            target: Some(output.path.clone()),
        })
}

impl Registry {
    pub fn status_snapshot(&mut self) -> Result<StatusSnapshot, RegistryError> {
        self.reconcile_stale_active_leases()?;

        let generated_at = utc_now(&self.connection)?;
        let mut services = self.status_services()?;
        self.attach_service_outputs(&mut services)?;
        let outputs = self.status_outputs()?;
        let runs = self.status_runs()?;

        Ok(StatusSnapshot {
            schema_version: STATUS_SCHEMA_VERSION,
            generated_at,
            outputs,
            services,
            runs,
        })
    }
    fn status_services(&self) -> Result<Vec<StatusService>, RegistryError> {
        let rows = {
            let mut statement = self.connection.prepare(
                "SELECT
                    leases.project,
                    leases.service,
                    leases.state,
                    leases.port,
                    leases.host,
                    leases.worktree_path,
                    leases.worktree_hash,
                    leases.git_common_dir,
                    leases.branch,
                    leases.branch_label,
                    leases.git_commit,
                    leases.identity_key,
                    leases.hostname,
                    leases.route_url,
                    leases.health_url,
                    runs.pid,
                    COALESCE(runs.command, 'reserved'),
                    COALESCE(runs.cwd, ''),
                    COALESCE(runs.started_at, leases.allocated_at),
                    runs.exited_at,
                    runs.exit_code,
                    CAST((julianday('now') - julianday(runs.started_at)) * 86400000 AS INTEGER)
                 FROM leases
                 LEFT JOIN runs ON runs.id = (
                    SELECT latest_runs.id
                    FROM runs latest_runs
                    WHERE latest_runs.lease_id = leases.id
                    ORDER BY latest_runs.started_at DESC, latest_runs.id DESC
                    LIMIT 1
                 )
                 JOIN (
                    SELECT MAX(leases.id) AS latest_lease_id
                    FROM leases
                    GROUP BY COALESCE(
                        leases.identity_key,
                        leases.project
                            || char(31)
                            || leases.service
                            || char(31)
                            || COALESCE(leases.worktree_path, '')
                            || char(31)
                            || COALESCE(leases.branch_label, '')
                            || char(31)
                            || leases.host
                    )
                 ) latest_services ON latest_services.latest_lease_id = leases.id
                 ORDER BY COALESCE(runs.started_at, leases.allocated_at) DESC, leases.id DESC",
            )?;
            let rows = statement.query_map([], |row| {
                let host = row.get::<_, String>(4)?;
                let port = row.get::<_, u16>(3)?;

                Ok(StatusServiceRow {
                    run_age_ms: row.get::<_, Option<i64>>(21)?.unwrap_or(i64::MAX),
                    service: StatusService {
                        project: row.get(0)?,
                        service: row.get(1)?,
                        state: row.get(2)?,
                        port,
                        url: format!("http://{host}:{port}"),
                        host,
                        worktree_path: row.get(5)?,
                        worktree_hash: row.get(6)?,
                        git_common_dir: row.get(7)?,
                        branch: row.get(8)?,
                        branch_label: row.get(9)?,
                        commit: row.get(10)?,
                        identity_key: row.get(11)?,
                        hostname: row.get(12)?,
                        route_url: row.get(13)?,
                        health_url: row.get(14)?,
                        pid: row.get(15)?,
                        command: row.get(16)?,
                        cwd: row.get(17)?,
                        started_at: row.get(18)?,
                        exited_at: row.get(19)?,
                        exit_code: row.get(20)?,
                        health: String::from("unknown"),
                        outputs: Vec::new(),
                        proxy: None,
                    },
                })
            })?;

            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut services = Vec::with_capacity(rows.len());
        for mut row in rows {
            row.service.health = health_status(
                &row.service.state,
                row.service.health_url.as_deref(),
                row.run_age_ms,
            );
            services.push(row.service);
        }

        Ok(services)
    }

    fn attach_service_outputs(&self, services: &mut [StatusService]) -> Result<(), RegistryError> {
        for service in services {
            let route_key = status_service_route_key(service);
            service.outputs = self.status_outputs_for_route(&route_key)?;
            service.proxy = status_proxy_for_outputs(&service.outputs);
        }

        Ok(())
    }

    fn status_outputs_for_route(
        &self,
        route_key: &str,
    ) -> Result<Vec<StatusServiceOutput>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT output_name, status, reason, rendered_path
             FROM output_files
             WHERE route_key = ?1
             ORDER BY output_name",
        )?;
        let rows = statement.query_map(params![route_key], |row| {
            Ok(StatusServiceOutput {
                name: row.get(0)?,
                status: row.get(1)?,
                reason: row.get(2)?,
                path: row.get(3)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn status_outputs(&self) -> Result<Vec<StatusOutput>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT
                output_name,
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'rendered' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'removed' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
             FROM output_files
             GROUP BY output_name
             ORDER BY output_name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StatusOutput {
                name: row.get(0)?,
                pending: row.get::<_, i64>(1)? as usize,
                rendered: row.get::<_, i64>(2)? as usize,
                removed: row.get::<_, i64>(3)? as usize,
                error: row.get::<_, i64>(4)? as usize,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn status_runs(&self) -> Result<Vec<StatusRun>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT id, lease_id, pid, command, cwd, started_at, exited_at, exit_code
             FROM runs
             ORDER BY started_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StatusRun {
                id: row.get(0)?,
                lease_id: row.get(1)?,
                pid: row.get(2)?,
                command: row.get(3)?,
                cwd: row.get(4)?,
                started_at: row.get(5)?,
                exited_at: row.get(6)?,
                exit_code: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
