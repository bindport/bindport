use super::*;

pub const EXPORT_SCHEMA_VERSION: &str = "0.1";

#[derive(Debug, Serialize)]
pub struct RegistryExportSnapshot {
    pub schema_version: &'static str,
    pub generated_at: String,
    pub registry_path: String,
    pub user_version: i64,
    pub leases: Vec<RegistryExportLease>,
    pub runs: Vec<RegistryExportRun>,
    pub output_files: Vec<RegistryExportOutputFile>,
    pub output_render_state: Vec<RegistryExportOutputRenderState>,
}

#[derive(Debug, Serialize)]
pub struct RegistryExportLease {
    pub id: i64,
    pub project: String,
    pub service: String,
    pub worktree_path: Option<String>,
    pub worktree_hash: Option<String>,
    pub git_common_dir: Option<String>,
    pub branch: Option<String>,
    pub branch_label: Option<String>,
    pub git_commit: Option<String>,
    pub identity_key: Option<String>,
    pub port: u16,
    pub host: String,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub health_url: Option<String>,
    pub state: String,
    pub allocated_at: String,
    pub last_seen_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegistryExportRun {
    pub id: i64,
    pub lease_id: i64,
    pub pid: u32,
    pub process_start_time: Option<i64>,
    pub command: String,
    pub cwd: String,
    pub started_at: String,
    pub exited_at: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct RegistryExportOutputFile {
    pub id: i64,
    pub output_name: String,
    pub output_scope: String,
    pub route_key: String,
    pub rendered_path: String,
    pub output_root: Option<String>,
    pub config_root: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_hash: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub content_hash: Option<String>,
    pub template_hash: Option<String>,
    pub lease_id: Option<i64>,
    pub run_id: Option<i64>,
    pub rendered_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct RegistryExportOutputRenderState {
    pub output_name: String,
    pub last_render_at_ms: i64,
}

impl Registry {
    pub fn export_snapshot(&self) -> Result<RegistryExportSnapshot, RegistryError> {
        Ok(RegistryExportSnapshot {
            schema_version: EXPORT_SCHEMA_VERSION,
            generated_at: utc_now(&self.connection)?,
            registry_path: self.path.display().to_string(),
            user_version: self.user_version()?,
            leases: self.export_leases()?,
            runs: self.export_runs()?,
            output_files: self.export_output_files()?,
            output_render_state: self.export_output_render_state()?,
        })
    }

    fn user_version(&self) -> Result<i64, RegistryError> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(Into::into)
    }

    fn export_leases(&self) -> Result<Vec<RegistryExportLease>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, project, service, worktree_path, worktree_hash,
                git_common_dir, branch, branch_label, git_commit, identity_key,
                port, host, hostname, route_url, health_url, state,
                allocated_at, last_seen_at, released_at
             FROM leases
             ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RegistryExportLease {
                id: row.get(0)?,
                project: row.get(1)?,
                service: row.get(2)?,
                worktree_path: row.get(3)?,
                worktree_hash: row.get(4)?,
                git_common_dir: row.get(5)?,
                branch: row.get(6)?,
                branch_label: row.get(7)?,
                git_commit: row.get(8)?,
                identity_key: row.get(9)?,
                port: row.get(10)?,
                host: row.get(11)?,
                hostname: row.get(12)?,
                route_url: row.get(13)?,
                health_url: row.get(14)?,
                state: row.get(15)?,
                allocated_at: row.get(16)?,
                last_seen_at: row.get(17)?,
                released_at: row.get(18)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn export_runs(&self) -> Result<Vec<RegistryExportRun>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, lease_id, pid, process_start_time, command, cwd,
                started_at, exited_at, exit_code
             FROM runs
             ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RegistryExportRun {
                id: row.get(0)?,
                lease_id: row.get(1)?,
                pid: row.get(2)?,
                process_start_time: row.get(3)?,
                command: row.get(4)?,
                cwd: row.get(5)?,
                started_at: row.get(6)?,
                exited_at: row.get(7)?,
                exit_code: row.get(8)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn export_output_files(&self) -> Result<Vec<RegistryExportOutputFile>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, output_name, output_scope, route_key, rendered_path,
                output_root, config_root, worktree_path, worktree_hash, status,
                reason, content_hash, template_hash, lease_id, run_id,
                rendered_at, updated_at
             FROM output_files
             ORDER BY output_name, output_scope, route_key, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RegistryExportOutputFile {
                id: row.get(0)?,
                output_name: row.get(1)?,
                output_scope: row.get(2)?,
                route_key: row.get(3)?,
                rendered_path: row.get(4)?,
                output_root: row.get(5)?,
                config_root: row.get(6)?,
                worktree_path: row.get(7)?,
                worktree_hash: row.get(8)?,
                status: row.get(9)?,
                reason: row.get(10)?,
                content_hash: row.get(11)?,
                template_hash: row.get(12)?,
                lease_id: row.get(13)?,
                run_id: row.get(14)?,
                rendered_at: row.get(15)?,
                updated_at: row.get(16)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn export_output_render_state(
        &self,
    ) -> Result<Vec<RegistryExportOutputRenderState>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT output_name, last_render_at_ms
             FROM output_render_state
             ORDER BY output_name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RegistryExportOutputRenderState {
                output_name: row.get(0)?,
                last_render_at_ms: row.get(1)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
