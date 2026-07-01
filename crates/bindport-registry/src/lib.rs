// SPDX-License-Identifier: MIT

use std::{
    env, fmt, fs,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

use bindport_core::{SERVICE_NAME, ServiceIdentity};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub const DEFAULT_REGISTRY_FILE: &str = "registry.sqlite";
pub const REGISTRY_PATH_ENV: &str = "BINDPORT_REGISTRY_PATH";
pub const STATUS_SCHEMA_VERSION: &str = "0.4";
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_millis(300);
const HEALTH_PENDING_GRACE_MS: i64 = 2_000;

pub fn default_registry_directory_name() -> &'static str {
    SERVICE_NAME
}

pub fn default_registry_path() -> Result<PathBuf, RegistryError> {
    if let Some(path) = env::var_os(REGISTRY_PATH_ENV).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(state_home)
            .join(default_registry_directory_name())
            .join(DEFAULT_REGISTRY_FILE));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join(default_registry_directory_name())
            .join(DEFAULT_REGISTRY_FILE));
    }

    if let Some(appdata) = env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(appdata)
            .join(default_registry_directory_name())
            .join(DEFAULT_REGISTRY_FILE));
    }

    Err(RegistryError::MissingStateDirectory)
}

#[derive(Debug)]
pub enum RegistryError {
    MissingStateDirectory,
    CreateDirectory {
        path: PathBuf,
        source: io::Error,
    },
    Open {
        path: PathBuf,
        source: rusqlite::Error,
    },
    Sqlite(rusqlite::Error),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingStateDirectory => {
                write!(
                    f,
                    "could not determine registry directory; set {REGISTRY_PATH_ENV}"
                )
            }
            Self::CreateDirectory { path, source } => {
                write!(
                    f,
                    "failed to create registry directory `{}`: {source}",
                    path.display()
                )
            }
            Self::Open { path, source } => {
                write!(f, "failed to open registry `{}`: {source}", path.display())
            }
            Self::Sqlite(source) => write!(f, "registry database error: {source}"),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDirectory { source, .. } => Some(source),
            Self::Open { source, .. } | Self::Sqlite(source) => Some(source),
            Self::MissingStateDirectory => None,
        }
    }
}

impl From<rusqlite::Error> for RegistryError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}

pub struct Registry {
    connection: Connection,
    path: PathBuf,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanState {
    Stopped,
    Stale,
}

impl CleanState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanSummary {
    pub stopped_leases: usize,
    pub stale_leases: usize,
    pub runs: usize,
}

impl CleanSummary {
    pub fn total_leases(self) -> usize {
        self.stopped_leases + self.stale_leases
    }

    fn add_leases(&mut self, state: CleanState, count: usize) {
        match state {
            CleanState::Stopped => self.stopped_leases += count,
            CleanState::Stale => self.stale_leases += count,
        }
    }
}

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

struct StatusServiceRow {
    service: StatusService,
    run_age_ms: i64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileStatus {
    Pending,
    Rendered,
    Removed,
    Error,
}

impl OutputFileStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Rendered => "rendered",
            Self::Removed => "removed",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFileRecord {
    pub output_name: String,
    pub route_key: String,
    pub rendered_path: PathBuf,
    pub status: OutputFileStatus,
    pub reason: Option<String>,
    pub content_hash: Option<String>,
    pub template_hash: Option<String>,
    pub lease_id: Option<i64>,
    pub run_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFileOwnership {
    pub route_key: String,
    pub path: PathBuf,
    pub content_hash: String,
}

impl Registry {
    pub fn open_default() -> Result<Self, RegistryError> {
        Self::open(default_registry_path()?)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, RegistryError> {
        let path = path.into();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| RegistryError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let connection = Connection::open(&path).map_err(|source| RegistryError::Open {
            path: path.clone(),
            source,
        })?;
        let registry = Self { connection, path };
        registry.ensure_schema()?;

        Ok(registry)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

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
            .filter(|run| !process_is_running(run.pid))
            .collect::<Vec<_>>();

        if stale_runs.is_empty() {
            return Ok(0);
        }

        let now = utc_now(&self.connection)?;
        let transaction = self.connection.transaction()?;

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
        let transaction = self.connection.transaction()?;

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
                lease_id, pid, command, cwd, started_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![lease_id, run.pid, run.command, cwd, now],
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

    pub fn clean_leases(
        &mut self,
        states: &[CleanState],
        dry_run: bool,
    ) -> Result<CleanSummary, RegistryError> {
        self.reconcile_stale_active_leases()?;

        let transaction = self.connection.transaction()?;
        let mut summary = CleanSummary::default();

        for state in [CleanState::Stopped, CleanState::Stale] {
            if !states.contains(&state) {
                continue;
            }

            let lease_count = transaction.query_row(
                "SELECT COUNT(*)
                 FROM leases
                 WHERE state = ?1",
                params![state.as_str()],
                |row| row.get::<_, i64>(0),
            )? as usize;
            let run_count = transaction.query_row(
                "SELECT COUNT(*)
                 FROM runs
                 WHERE lease_id IN (
                    SELECT id
                    FROM leases
                    WHERE state = ?1
                 )",
                params![state.as_str()],
                |row| row.get::<_, i64>(0),
            )? as usize;

            summary.add_leases(state, lease_count);
            summary.runs += run_count;

            if dry_run {
                continue;
            }

            transaction.execute(
                "DELETE FROM runs
                 WHERE lease_id IN (
                    SELECT id
                    FROM leases
                    WHERE state = ?1
                 )",
                params![state.as_str()],
            )?;
            transaction.execute(
                "DELETE FROM leases
                 WHERE state = ?1",
                params![state.as_str()],
            )?;
        }

        transaction.commit()?;

        Ok(summary)
    }

    pub fn output_file_ownership(
        &self,
        output_name: &str,
    ) -> Result<Vec<OutputFileOwnership>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT route_key, rendered_path, content_hash
             FROM output_files
             WHERE output_name = ?1
             AND content_hash IS NOT NULL
             AND (
                status = 'rendered'
                OR (status = 'error' AND reason = 'external_modified')
             )
             ORDER BY rendered_path, route_key",
        )?;
        let rows = statement.query_map(params![output_name], |row| {
            Ok(OutputFileOwnership {
                route_key: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                content_hash: row.get(2)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_output_file(&mut self, record: &OutputFileRecord) -> Result<(), RegistryError> {
        let now = utc_now(&self.connection)?;
        let rendered_path = record.rendered_path.display().to_string();

        self.connection.execute(
            "INSERT INTO output_files (
                output_name, route_key, rendered_path, status, reason,
                content_hash, template_hash, lease_id, run_id, rendered_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10
             )
             ON CONFLICT(output_name, route_key) DO UPDATE SET
                rendered_path = excluded.rendered_path,
                status = excluded.status,
                reason = excluded.reason,
                content_hash = excluded.content_hash,
                template_hash = excluded.template_hash,
                lease_id = excluded.lease_id,
                run_id = excluded.run_id,
                updated_at = excluded.updated_at",
            params![
                &record.output_name,
                &record.route_key,
                rendered_path,
                record.status.as_str(),
                &record.reason,
                &record.content_hash,
                &record.template_hash,
                record.lease_id,
                record.run_id,
                now
            ],
        )?;

        Ok(())
    }

    pub fn reserve_auto_render(
        &mut self,
        output_name: &str,
        debounce_ms: u64,
    ) -> Result<Duration, RegistryError> {
        let now_ms = self.connection.query_row(
            "SELECT CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)",
            [],
            |row| row.get::<_, i64>(0),
        )?;

        self.reserve_auto_render_at(output_name, debounce_ms, now_ms)
    }

    fn reserve_auto_render_at(
        &mut self,
        output_name: &str,
        debounce_ms: u64,
        now_ms: i64,
    ) -> Result<Duration, RegistryError> {
        let transaction = self.connection.transaction()?;
        let previous_ms = transaction
            .query_row(
                "SELECT last_render_at_ms
                 FROM output_render_state
                 WHERE output_name = ?1",
                params![output_name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let debounce_ms = i64::try_from(debounce_ms).unwrap_or(i64::MAX);
        let delay_ms = if debounce_ms == 0 {
            0
        } else {
            previous_ms
                .map(|previous_ms| {
                    previous_ms
                        .saturating_add(debounce_ms)
                        .saturating_sub(now_ms)
                })
                .unwrap_or_default()
                .max(0)
        };
        let scheduled_ms = now_ms.saturating_add(delay_ms);

        transaction.execute(
            "INSERT INTO output_render_state (output_name, last_render_at_ms)
             VALUES (?1, ?2)
             ON CONFLICT(output_name) DO UPDATE SET
                last_render_at_ms = excluded.last_render_at_ms",
            params![output_name, scheduled_ms],
        )?;
        transaction.commit()?;

        Ok(Duration::from_millis(
            u64::try_from(delay_ms).unwrap_or(u64::MAX),
        ))
    }

    fn ensure_schema(&self) -> Result<(), RegistryError> {
        self.connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS leases (
                id INTEGER PRIMARY KEY,
                project TEXT NOT NULL,
                service TEXT NOT NULL,
                worktree_path TEXT,
                worktree_hash TEXT,
                git_common_dir TEXT,
                branch TEXT,
                branch_label TEXT,
                git_commit TEXT,
                identity_key TEXT,
                port INTEGER NOT NULL,
                host TEXT NOT NULL,
                hostname TEXT,
                route_url TEXT,
                health_url TEXT,
                state TEXT NOT NULL,
                allocated_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                released_at TEXT
            );

            CREATE INDEX IF NOT EXISTS leases_state_port_idx
            ON leases(state, port);

            CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY,
                lease_id INTEGER NOT NULL REFERENCES leases(id),
                pid INTEGER NOT NULL,
                command TEXT NOT NULL,
                cwd TEXT NOT NULL,
                started_at TEXT NOT NULL,
                exited_at TEXT,
                exit_code INTEGER
            );

            CREATE INDEX IF NOT EXISTS runs_lease_id_idx
            ON runs(lease_id);

            ",
        )?;
        self.ensure_lease_identity_columns()?;
        self.connection.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS leases_identity_key_idx
            ON leases(identity_key);

            CREATE TABLE IF NOT EXISTS output_files (
                id INTEGER PRIMARY KEY,
                output_name TEXT NOT NULL,
                route_key TEXT NOT NULL,
                rendered_path TEXT NOT NULL,
                status TEXT NOT NULL,
                reason TEXT,
                content_hash TEXT,
                template_hash TEXT,
                lease_id INTEGER,
                run_id INTEGER,
                rendered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(output_name, route_key)
            );

            CREATE INDEX IF NOT EXISTS output_files_output_path_idx
            ON output_files(output_name, rendered_path);

            CREATE INDEX IF NOT EXISTS output_files_route_key_idx
            ON output_files(route_key);

            CREATE TABLE IF NOT EXISTS output_render_state (
                output_name TEXT PRIMARY KEY,
                last_render_at_ms INTEGER NOT NULL
            );

            PRAGMA user_version = 7;
            ",
        )?;

        Ok(())
    }

    fn ensure_lease_identity_columns(&self) -> Result<(), RegistryError> {
        let existing = self.lease_columns()?;

        for (column, definition) in [
            ("worktree_path", "TEXT"),
            ("worktree_hash", "TEXT"),
            ("git_common_dir", "TEXT"),
            ("branch", "TEXT"),
            ("branch_label", "TEXT"),
            ("git_commit", "TEXT"),
            ("identity_key", "TEXT"),
            ("hostname", "TEXT"),
            ("route_url", "TEXT"),
            ("health_url", "TEXT"),
        ] {
            if !existing.iter().any(|existing| existing == column) {
                self.connection.execute(
                    &format!("ALTER TABLE leases ADD COLUMN {column} {definition}"),
                    [],
                )?;
            }
        }

        Ok(())
    }

    fn lease_columns(&self) -> Result<Vec<String>, RegistryError> {
        let mut statement = self.connection.prepare("PRAGMA table_info(leases)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn active_runs(&self) -> Result<Vec<ActiveRun>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT leases.id, runs.id, runs.pid
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
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
                    runs.command,
                    runs.cwd,
                    runs.started_at,
                    runs.exited_at,
                    runs.exit_code,
                    CAST((julianday('now') - julianday(runs.started_at)) * 86400000 AS INTEGER)
                 FROM leases
                 JOIN runs ON runs.lease_id = leases.id
                 JOIN (
                    SELECT MAX(runs.id) AS latest_run_id
                    FROM leases
                    JOIN runs ON runs.lease_id = leases.id
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
                 ) latest_services ON latest_services.latest_run_id = runs.id
                 ORDER BY runs.started_at DESC, runs.id DESC",
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

#[derive(Debug)]
struct ActiveRun {
    lease_id: i64,
    run_id: i64,
    pid: u32,
}

pub fn status_service_route_key(service: &StatusService) -> String {
    service.identity_key.clone().unwrap_or_else(|| {
        format!(
            "{}:{}:{}:{}:{}",
            service.project, service.service, service.host, service.port, service.started_at
        )
    })
}

fn status_proxy_for_outputs(outputs: &[StatusServiceOutput]) -> Option<StatusProxy> {
    outputs
        .iter()
        .find(|output| output.name == "traefik")
        .map(|output| StatusProxy {
            adapter: String::from("traefik"),
            rendered: output.status == OutputFileStatus::Rendered.as_str(),
            target: Some(output.path.clone()),
        })
}

fn health_status(state: &str, health_url: Option<&str>, run_age_ms: i64) -> String {
    if state != "active" {
        return String::from("unknown");
    }

    let Some(health_url) = health_url.filter(|url| !url.trim().is_empty()) else {
        return String::from("unknown");
    };

    if run_age_ms < HEALTH_PENDING_GRACE_MS {
        return String::from("pending");
    }

    match check_http_health(health_url) {
        HealthProbe::Healthy => String::from("healthy"),
        HealthProbe::Failing => String::from("failing"),
        HealthProbe::Unknown => String::from("unknown"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthProbe {
    Healthy,
    Failing,
    Unknown,
}

fn check_http_health(url: &str) -> HealthProbe {
    let target = match http_health_target(url) {
        Ok(Some(target)) => target,
        Ok(None) => return HealthProbe::Unknown,
        Err(()) => return HealthProbe::Failing,
    };

    match probe_http_target(&target) {
        Ok(status) if (200..400).contains(&status) => HealthProbe::Healthy,
        Ok(_) | Err(_) => HealthProbe::Failing,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpHealthTarget {
    address: SocketAddr,
    path: String,
    authority: String,
}

fn http_health_target(url: &str) -> Result<Option<HttpHealthTarget>, ()> {
    let url = url.trim();
    let Some(rest) = url.strip_prefix("http://") else {
        return if url.starts_with("https://") {
            Ok(None)
        } else {
            Err(())
        };
    };
    let (authority, path) = rest
        .split_once('/')
        .map(|(authority, path)| (authority, format!("/{path}")))
        .unwrap_or((rest, String::from("/")));
    let (host, port) = parse_http_authority(authority).ok_or(())?;
    let Some(address) = loopback_socket_addr(&host, port) else {
        return Ok(None);
    };

    Ok(Some(HttpHealthTarget {
        address,
        path,
        authority: authority.to_string(),
    }))
}

fn parse_http_authority(authority: &str) -> Option<(String, u16)> {
    if authority.is_empty() || authority.contains('@') {
        return None;
    }

    if let Some(rest) = authority.strip_prefix('[') {
        let (host, remainder) = rest.split_once(']')?;
        if host.is_empty() {
            return None;
        }
        let port = match remainder.strip_prefix(':') {
            Some(port) if !port.is_empty() => port.parse().ok()?,
            Some(_) => return None,
            None if remainder.is_empty() => 80,
            None => return None,
        };

        return Some((host.to_string(), port));
    }

    match authority.matches(':').count() {
        0 => Some((authority.to_string(), 80)),
        1 => {
            let (host, port) = authority.rsplit_once(':')?;
            if host.is_empty() || port.is_empty() {
                return None;
            }

            Some((host.to_string(), port.parse().ok()?))
        }
        _ => None,
    }
}

fn loopback_socket_addr(host: &str, port: u16) -> Option<SocketAddr> {
    if let Ok(address) = host.parse::<IpAddr>() {
        return address
            .is_loopback()
            .then_some(SocketAddr::new(address, port));
    }

    let normalized = host.trim_end_matches('.');
    let lower = normalized.to_ascii_lowercase();
    (lower == "localhost" || lower.ends_with(".localhost"))
        .then_some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

fn probe_http_target(target: &HttpHealthTarget) -> io::Result<u16> {
    let mut stream = TcpStream::connect_timeout(&target.address, HEALTH_CHECK_TIMEOUT)?;
    stream.set_read_timeout(Some(HEALTH_CHECK_TIMEOUT))?;
    stream.set_write_timeout(Some(HEALTH_CHECK_TIMEOUT))?;
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.path, target.authority
    )?;

    let mut response = Vec::new();
    let mut buffer = [0_u8; 128];
    while response.len() < 1024 {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => {
                response.extend_from_slice(&buffer[..bytes]);
                if response.contains(&b'\n') {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) && !response.is_empty() =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }

    if response.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty health response",
        ));
    }

    let response = std::str::from_utf8(&response)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "missing HTTP status in `{}`",
                    response.lines().next().unwrap_or_default()
                ),
            )
        })?;

    Ok(status)
}

fn utc_now(connection: &Connection) -> Result<String, RegistryError> {
    connection
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |row| {
            row.get(0)
        })
        .map_err(Into::into)
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };

    if result == 0 {
        return true;
    }

    matches!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
}

#[cfg(not(unix))]
fn process_is_running(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::TcpListener,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_run_start(project: &str, service: &str, port: u16, pid: u32) -> RunStart {
        RunStart {
            project: String::from(project),
            service: String::from(service),
            identity: None,
            host: String::from("127.0.0.1"),
            port,
            hostname: None,
            route_url: None,
            health_url: None,
            pid,
            command: String::from("next dev"),
            cwd: PathBuf::from("/tmp/bindport"),
        }
    }

    fn mark_latest_run_started_before_grace(registry: &Registry) {
        registry
            .connection
            .execute(
                "UPDATE runs
                 SET started_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-5 seconds')",
                [],
            )
            .expect("backdate run start");
    }

    fn free_loopback_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
        listener.local_addr().expect("local addr").port()
    }

    fn start_health_server(status: &'static str) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind health server");
        let port = listener.local_addr().expect("health server addr").port();

        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        });

        port
    }

    #[test]
    fn registry_defaults_are_named_for_bindport() {
        assert_eq!(default_registry_directory_name(), "bindport");
        assert_eq!(DEFAULT_REGISTRY_FILE, "registry.sqlite");
    }

    #[test]
    fn registry_records_finished_runs_for_status() {
        let mut registry = Registry::open(temp_registry_path("finished")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.schema_version, STATUS_SCHEMA_VERSION);
        assert!(snapshot.outputs.is_empty());
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.services[0].state, "stopped");
        assert_eq!(snapshot.services[0].port, 29_123);
        assert_eq!(snapshot.services[0].url, "http://127.0.0.1:29123");
        assert_eq!(snapshot.services[0].hostname.as_deref(), None);
        assert_eq!(snapshot.services[0].route_url.as_deref(), None);
        assert!(snapshot.services[0].outputs.is_empty());
        assert!(snapshot.services[0].proxy.is_none());
        assert_eq!(snapshot.services[0].exit_code, Some(0));
        assert_eq!(snapshot.runs.len(), 1);
    }

    #[test]
    fn health_targets_are_restricted_to_loopback_without_dns() {
        let target = http_health_target("http://127.0.0.1:29100/health")
            .expect("loopback URL parses")
            .expect("loopback URL is supported");
        assert_eq!(
            target.address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_100)
        );
        assert_eq!(target.authority, "127.0.0.1:29100");
        assert_eq!(target.path, "/health");

        let target = http_health_target("http://feature.branch.localhost:29101/ready")
            .expect("localhost URL parses")
            .expect("localhost URL is supported");
        assert_eq!(
            target.address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_101)
        );
        assert_eq!(target.authority, "feature.branch.localhost:29101");

        assert!(
            http_health_target("http://169.254.169.254/latest")
                .expect("metadata URL parses")
                .is_none()
        );
        assert!(
            http_health_target("http://example.invalid/health")
                .expect("external URL parses")
                .is_none()
        );
        assert!(
            http_health_target("https://127.0.0.1:29100/health")
                .expect("https URL parses as unsupported")
                .is_none()
        );
    }

    #[test]
    fn status_health_is_pending_during_startup_grace() {
        let mut registry = Registry::open(temp_registry_path("health-pending")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(format!("http://127.0.0.1:{}/health", free_loopback_port()));

        registry.record_run_started(&run).expect("record start");

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "pending");
    }

    #[test]
    fn status_health_reports_healthy_http_response() {
        let direct_port = start_health_server("204 No Content");
        let direct_url = format!("http://127.0.0.1:{direct_port}/health");
        let direct_target = http_health_target(&direct_url)
            .expect("direct health URL parses")
            .expect("direct health URL is supported");
        let direct_status = probe_http_target(&direct_target).expect("direct health probe");
        assert!((200..400).contains(&direct_status));

        let health_port = start_health_server("204 No Content");
        let mut registry = Registry::open(temp_registry_path("health-healthy")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(format!("http://127.0.0.1:{health_port}/health"));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "healthy");
    }

    #[test]
    fn status_health_reports_failing_http_response() {
        let mut registry = Registry::open(temp_registry_path("health-failing")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(format!("http://127.0.0.1:{}/health", free_loopback_port()));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "failing");
    }

    #[test]
    fn status_health_reports_unknown_for_non_loopback_http_targets() {
        let mut registry =
            Registry::open(temp_registry_path("health-non-loopback")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(String::from("http://192.0.2.1/health"));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "unknown");
    }

    #[test]
    fn status_health_reports_unknown_for_unsupported_schemes() {
        let mut registry =
            Registry::open(temp_registry_path("health-unsupported")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(String::from("https://web.localhost/health"));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "unknown");
    }

    #[test]
    fn clean_leases_dry_run_counts_without_deleting_stopped_runs() {
        let mut registry = Registry::open(temp_registry_path("clean-dry-run")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let summary = registry
            .clean_leases(&[CleanState::Stopped], true)
            .expect("clean dry-run");

        assert_eq!(summary.stopped_leases, 1);
        assert_eq!(summary.stale_leases, 0);
        assert_eq!(summary.runs, 1);
        assert_eq!(summary.total_leases(), 1);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.runs.len(), 1);
    }

    #[test]
    fn clean_leases_removes_stopped_runs() {
        let mut registry = Registry::open(temp_registry_path("clean-stopped")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let summary = registry
            .clean_leases(&[CleanState::Stopped, CleanState::Stale], false)
            .expect("clean");

        assert_eq!(summary.stopped_leases, 1);
        assert_eq!(summary.stale_leases, 0);
        assert_eq!(summary.runs, 1);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert!(snapshot.services.is_empty());
        assert!(snapshot.runs.is_empty());
    }

    #[test]
    fn output_file_ownership_returns_rendered_files_with_hashes() {
        let mut registry = Registry::open(temp_registry_path("output-files")).expect("registry");
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: String::from("route-1"),
                rendered_path: PathBuf::from("/tmp/bindport/route-1.yml"),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: None,
                run_id: None,
            })
            .expect("record rendered file");
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: String::from("route-2"),
                rendered_path: PathBuf::from("/tmp/bindport/route-2.yml"),
                status: OutputFileStatus::Error,
                reason: Some(String::from("template_error")),
                content_hash: None,
                template_hash: Some(String::from("template-1")),
                lease_id: None,
                run_id: None,
            })
            .expect("record error file");

        let ownership = registry
            .output_file_ownership("traefik")
            .expect("ownership records");

        assert_eq!(
            ownership,
            vec![OutputFileOwnership {
                route_key: String::from("route-1"),
                path: PathBuf::from("/tmp/bindport/route-1.yml"),
                content_hash: String::from("hash-1")
            }]
        );

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.outputs.len(), 1);
        assert_eq!(snapshot.outputs[0].name, "traefik");
        assert_eq!(snapshot.outputs[0].rendered, 1);
        assert_eq!(snapshot.outputs[0].error, 1);
    }

    #[test]
    fn output_file_ownership_keeps_external_modified_expected_hashes() {
        let mut registry =
            Registry::open(temp_registry_path("output-files-modified")).expect("registry");
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: String::from("route-1"),
                rendered_path: PathBuf::from("/tmp/bindport/route-1.yml"),
                status: OutputFileStatus::Error,
                reason: Some(String::from("external_modified")),
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: None,
                run_id: None,
            })
            .expect("record external modification");

        let ownership = registry
            .output_file_ownership("traefik")
            .expect("ownership records");

        assert_eq!(
            ownership,
            vec![OutputFileOwnership {
                route_key: String::from("route-1"),
                path: PathBuf::from("/tmp/bindport/route-1.yml"),
                content_hash: String::from("hash-1")
            }]
        );
    }

    #[test]
    fn record_output_file_upserts_by_output_and_route() {
        let mut registry =
            Registry::open(temp_registry_path("output-files-upsert")).expect("registry");
        let mut record = OutputFileRecord {
            output_name: String::from("traefik"),
            route_key: String::from("route-1"),
            rendered_path: PathBuf::from("/tmp/bindport/old.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("old-hash")),
            template_hash: Some(String::from("template-1")),
            lease_id: Some(1),
            run_id: Some(2),
        };

        registry
            .record_output_file(&record)
            .expect("record first file");
        record.rendered_path = PathBuf::from("/tmp/bindport/new.yml");
        record.content_hash = Some(String::from("new-hash"));
        registry
            .record_output_file(&record)
            .expect("record updated file");

        let ownership = registry
            .output_file_ownership("traefik")
            .expect("ownership records");

        assert_eq!(ownership.len(), 1);
        assert_eq!(ownership[0].path, PathBuf::from("/tmp/bindport/new.yml"));
        assert_eq!(ownership[0].content_hash, "new-hash");
    }

    #[test]
    fn auto_render_reservations_apply_debounce_windows() {
        let mut registry =
            Registry::open(temp_registry_path("auto-render-reservation")).expect("registry");

        let first = registry
            .reserve_auto_render_at("traefik", 250, 1_000)
            .expect("first reservation");
        let second = registry
            .reserve_auto_render_at("traefik", 250, 1_100)
            .expect("debounced reservation");
        let disabled = registry
            .reserve_auto_render_at("traefik", 0, 1_100)
            .expect("disabled debounce");

        assert_eq!(first, Duration::from_millis(0));
        assert_eq!(second, Duration::from_millis(150));
        assert_eq!(disabled, Duration::from_millis(0));
    }

    #[cfg(unix)]
    #[test]
    fn clean_leases_reconciles_and_removes_stale_runs() {
        let mut registry = Registry::open(temp_registry_path("clean-stale")).expect("registry");
        registry
            .record_run_started(&test_run_start("bindport", "web", 29_500, 2_000_000_000))
            .expect("record start");

        let summary = registry
            .clean_leases(&[CleanState::Stale], false)
            .expect("clean stale");

        assert_eq!(summary.stopped_leases, 0);
        assert_eq!(summary.stale_leases, 1);
        assert_eq!(summary.runs, 1);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert!(snapshot.services.is_empty());
        assert!(snapshot.runs.is_empty());
    }

    #[test]
    fn registry_records_identity_fields_for_status() {
        let mut registry = Registry::open(temp_registry_path("identity")).expect("registry");
        let identity = ServiceIdentity {
            project: String::from("bindport"),
            service: String::from("web"),
            git: Some(bindport_core::GitIdentity {
                worktree_path: PathBuf::from("/tmp/bindport-worktree"),
                worktree_hash: String::from("abc123"),
                git_common_dir: PathBuf::from("/tmp/bindport-worktree/.git"),
                branch: String::from("feature/tree"),
                branch_label: String::from("feature-tree"),
                commit: String::from("1234567"),
            }),
            identity_key: String::from("v1:identity"),
        };
        let started = registry
            .record_run_started(&RunStart {
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity: Some(identity),
                host: String::from("127.0.0.1"),
                port: 29_124,
                hostname: Some(String::from("feature-tree.bindport.localhost")),
                route_url: Some(String::from("http://feature-tree.bindport.localhost")),
                health_url: None,
                pid: 12_346,
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport-worktree"),
            })
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert_eq!(
            service.worktree_path.as_deref(),
            Some("/tmp/bindport-worktree")
        );
        assert_eq!(service.worktree_hash.as_deref(), Some("abc123"));
        assert_eq!(service.branch.as_deref(), Some("feature/tree"));
        assert_eq!(service.branch_label.as_deref(), Some("feature-tree"));
        assert_eq!(service.commit.as_deref(), Some("1234567"));
        assert_eq!(service.identity_key.as_deref(), Some("v1:identity"));
        assert_eq!(
            service.hostname.as_deref(),
            Some("feature-tree.bindport.localhost")
        );
        assert_eq!(
            service.route_url.as_deref(),
            Some("http://feature-tree.bindport.localhost")
        );
    }

    #[test]
    fn status_snapshot_reports_service_outputs_and_traefik_proxy_alias() {
        let mut registry =
            Registry::open(temp_registry_path("status-service-outputs")).expect("registry");
        let identity = test_identity("v1:status-output");
        let started = registry
            .record_run_started(&RunStart {
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity: Some(identity.clone()),
                host: String::from("127.0.0.1"),
                port: 29_124,
                hostname: Some(String::from("status.localhost")),
                route_url: Some(String::from("http://status.localhost")),
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record start");

        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: identity.identity_key,
                rendered_path: PathBuf::from("/tmp/bindport/traefik/web.yml"),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: Some(started.lease_id),
                run_id: Some(started.run_id),
            })
            .expect("record rendered file");

        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert_eq!(snapshot.outputs.len(), 1);
        assert_eq!(snapshot.outputs[0].name, "traefik");
        assert_eq!(snapshot.outputs[0].rendered, 1);
        assert_eq!(service.outputs.len(), 1);
        assert_eq!(service.outputs[0].name, "traefik");
        assert_eq!(service.outputs[0].status, "rendered");
        assert_eq!(service.outputs[0].reason, None);
        assert_eq!(service.outputs[0].path, "/tmp/bindport/traefik/web.yml");
        let proxy = service.proxy.as_ref().expect("traefik proxy alias");
        assert_eq!(proxy.adapter, "traefik");
        assert!(proxy.rendered);
        assert_eq!(
            proxy.target.as_deref(),
            Some("/tmp/bindport/traefik/web.yml")
        );
    }

    #[test]
    fn status_snapshot_links_outputs_for_services_without_identity_keys() {
        let mut registry =
            Registry::open(temp_registry_path("status-output-fallback-key")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start(
                "bindport",
                "next",
                29_123,
                std::process::id(),
            ))
            .expect("record start");
        let route_key = {
            let snapshot = registry.status_snapshot().expect("snapshot");
            let service = &snapshot.services[0];

            assert!(service.identity_key.is_none());
            format!(
                "{}:{}:{}:{}:{}",
                service.project, service.service, service.host, service.port, service.started_at
            )
        };

        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key,
                rendered_path: PathBuf::from("/tmp/bindport/traefik/next.yml"),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: Some(started.lease_id),
                run_id: Some(started.run_id),
            })
            .expect("record rendered file");

        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert_eq!(service.outputs.len(), 1);
        assert_eq!(service.outputs[0].name, "traefik");
        assert_eq!(service.outputs[0].path, "/tmp/bindport/traefik/next.yml");
        assert!(service.proxy.as_ref().expect("proxy").rendered);
    }

    #[test]
    fn previous_identity_port_returns_latest_matching_lease() {
        let mut registry = Registry::open(temp_registry_path("previous-port")).expect("registry");
        let first_identity = test_identity("v1:first");
        let second_identity = test_identity("v1:second");
        let first = registry
            .record_run_started(&RunStart {
                project: first_identity.project.clone(),
                service: first_identity.service.clone(),
                identity: Some(first_identity.clone()),
                host: String::from("127.0.0.1"),
                port: 29_123,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record first start");
        registry
            .record_run_finished(first, Some(0))
            .expect("record first finish");
        let second = registry
            .record_run_started(&RunStart {
                project: first_identity.project.clone(),
                service: first_identity.service.clone(),
                identity: Some(first_identity.clone()),
                host: String::from("127.0.0.1"),
                port: 29_124,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record second start");
        registry
            .record_run_finished(second, Some(0))
            .expect("record second finish");
        let other = registry
            .record_run_started(&RunStart {
                project: second_identity.project.clone(),
                service: second_identity.service.clone(),
                identity: Some(second_identity),
                host: String::from("127.0.0.1"),
                port: 29_125,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record other start");
        registry
            .record_run_finished(other, Some(0))
            .expect("record other finish");

        assert_eq!(
            registry
                .previous_identity_port(&first_identity.identity_key)
                .expect("previous port"),
            Some(29_124)
        );
        assert_eq!(
            registry
                .previous_identity_port("v1:missing")
                .expect("missing previous port"),
            None
        );
    }

    #[test]
    fn active_ports_reports_active_and_reserved_leases() {
        let mut registry = Registry::open(temp_registry_path("active")).expect("registry");
        registry
            .record_run_started(&test_run_start(
                "bindport",
                "web",
                29_500,
                std::process::id(),
            ))
            .expect("record start");

        assert_eq!(registry.active_ports().expect("ports"), vec![29_500]);
    }

    #[cfg(unix)]
    #[test]
    fn active_ports_marks_dead_pid_stale() {
        let mut registry = Registry::open(temp_registry_path("stale")).expect("registry");
        registry
            .record_run_started(&test_run_start("bindport", "web", 29_500, 2_000_000_000))
            .expect("record start");

        assert!(registry.active_ports().expect("ports").is_empty());

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services[0].state, "stale");
        assert!(snapshot.services[0].exited_at.is_some());
        assert_eq!(snapshot.services[0].exit_code, None);
    }

    fn temp_registry_path(name: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();

        env::temp_dir().join(format!(
            "bindport-registry-{name}-{}-{now}.sqlite",
            std::process::id()
        ))
    }

    fn test_identity(identity_key: &str) -> ServiceIdentity {
        ServiceIdentity {
            project: String::from("bindport"),
            service: String::from("web"),
            git: None,
            identity_key: identity_key.to_owned(),
        }
    }
}
