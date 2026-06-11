// SPDX-License-Identifier: MIT

use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use bindport_core::{SERVICE_NAME, ServiceIdentity};
use rusqlite::{Connection, params};
use serde::Serialize;

pub const DEFAULT_REGISTRY_FILE: &str = "registry.sqlite";
pub const REGISTRY_PATH_ENV: &str = "BINDPORT_REGISTRY_PATH";
pub const STATUS_SCHEMA_VERSION: &str = "0.1";

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
    pub pid: u32,
    pub command: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartedRun {
    pub lease_id: i64,
    pub run_id: i64,
}

#[derive(Debug, Serialize)]
pub struct StatusSnapshot {
    pub schema_version: &'static str,
    pub generated_at: String,
    pub services: Vec<StatusService>,
    pub runs: Vec<StatusRun>,
}

#[derive(Debug, Serialize)]
pub struct StatusService {
    pub project: String,
    pub service: String,
    pub state: String,
    pub port: u16,
    pub host: String,
    pub url: String,
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
                branch, branch_label, git_commit, identity_key, port, host, state,
                allocated_at, last_seen_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'active', ?12, ?12
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
        let services = self.status_services()?;
        let runs = self.status_runs()?;

        Ok(StatusSnapshot {
            schema_version: STATUS_SCHEMA_VERSION,
            generated_at,
            services,
            runs,
        })
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

            PRAGMA user_version = 2;
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
                runs.pid,
                runs.command,
                runs.cwd,
                runs.started_at,
                runs.exited_at,
                runs.exit_code
             FROM leases
             JOIN runs ON runs.lease_id = leases.id
             ORDER BY runs.started_at DESC, runs.id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let host = row.get::<_, String>(4)?;
            let port = row.get::<_, u16>(3)?;

            Ok(StatusService {
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
                pid: row.get(12)?,
                command: row.get(13)?,
                cwd: row.get(14)?,
                started_at: row.get(15)?,
                exited_at: row.get(16)?,
                exit_code: row.get(17)?,
                health: String::from("unknown"),
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn registry_defaults_are_named_for_bindport() {
        assert_eq!(default_registry_directory_name(), "bindport");
        assert_eq!(DEFAULT_REGISTRY_FILE, "registry.sqlite");
    }

    #[test]
    fn registry_records_finished_runs_for_status() {
        let mut registry = Registry::open(temp_registry_path("finished")).expect("registry");
        let started = registry
            .record_run_started(&RunStart {
                project: String::from("bindport"),
                service: String::from("next"),
                identity: None,
                host: String::from("127.0.0.1"),
                port: 29_123,
                pid: 12_345,
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.schema_version, STATUS_SCHEMA_VERSION);
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.services[0].state, "stopped");
        assert_eq!(snapshot.services[0].port, 29_123);
        assert_eq!(snapshot.services[0].url, "http://127.0.0.1:29123");
        assert_eq!(snapshot.services[0].exit_code, Some(0));
        assert_eq!(snapshot.runs.len(), 1);
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
            identity_key: String::from("bindport:web:abc123:feature-tree"),
        };
        let started = registry
            .record_run_started(&RunStart {
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity: Some(identity),
                host: String::from("127.0.0.1"),
                port: 29_124,
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
        assert_eq!(
            service.identity_key.as_deref(),
            Some("bindport:web:abc123:feature-tree")
        );
    }

    #[test]
    fn active_ports_reports_active_and_reserved_leases() {
        let mut registry = Registry::open(temp_registry_path("active")).expect("registry");
        registry
            .record_run_started(&RunStart {
                project: String::from("bindport"),
                service: String::from("web"),
                identity: None,
                host: String::from("127.0.0.1"),
                port: 29_500,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record start");

        assert_eq!(registry.active_ports().expect("ports"), vec![29_500]);
    }

    #[cfg(unix)]
    #[test]
    fn active_ports_marks_dead_pid_stale() {
        let mut registry = Registry::open(temp_registry_path("stale")).expect("registry");
        registry
            .record_run_started(&RunStart {
                project: String::from("bindport"),
                service: String::from("web"),
                identity: None,
                host: String::from("127.0.0.1"),
                port: 29_500,
                pid: 2_000_000_000,
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
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
}
