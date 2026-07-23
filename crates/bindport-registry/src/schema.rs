use super::*;

impl Registry {
    pub(crate) fn ensure_schema(&mut self) -> Result<(), RegistryError> {
        self.connection.pragma_update(None, "foreign_keys", true)?;
        let path = self.path.clone();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let user_version =
            transaction.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
        if user_version > REGISTRY_USER_VERSION {
            return Err(RegistryError::UnsupportedRegistryVersion {
                path,
                found: user_version,
                supported: REGISTRY_USER_VERSION,
            });
        }
        transaction.execute_batch(
            "
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
                process_start_time INTEGER,
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
        Self::ensure_lease_identity_columns(&transaction)?;
        Self::ensure_run_process_columns(&transaction)?;
        transaction.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS leases_identity_key_idx
            ON leases(identity_key);

            CREATE TABLE IF NOT EXISTS output_files (
                id INTEGER PRIMARY KEY,
                output_name TEXT NOT NULL,
                output_scope TEXT NOT NULL DEFAULT 'unscoped',
                route_key TEXT NOT NULL,
                rendered_path TEXT NOT NULL,
                output_root TEXT,
                config_root TEXT,
                worktree_path TEXT,
                worktree_hash TEXT,
                status TEXT NOT NULL,
                reason TEXT,
                content_hash TEXT,
                template_hash TEXT,
                lease_id INTEGER,
                run_id INTEGER,
                rendered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(output_name, output_scope, route_key)
            );
            ",
        )?;
        Self::ensure_output_file_scope_columns(&transaction)?;
        transaction.execute_batch(
            "

            CREATE INDEX IF NOT EXISTS output_files_output_path_idx
            ON output_files(output_name, output_scope, rendered_path);

            CREATE INDEX IF NOT EXISTS output_files_route_key_idx
            ON output_files(route_key);

            CREATE TABLE IF NOT EXISTS output_render_state (
                output_name TEXT PRIMARY KEY,
                last_render_at_ms INTEGER NOT NULL
            );
            ",
        )?;
        transaction.pragma_update(None, "user_version", REGISTRY_USER_VERSION)?;
        transaction.commit()?;

        Ok(())
    }

    fn ensure_lease_identity_columns(connection: &Connection) -> Result<(), RegistryError> {
        let existing = Self::table_columns(connection, "leases")?;

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
                connection.execute(
                    &format!("ALTER TABLE leases ADD COLUMN {column} {definition}"),
                    [],
                )?;
            }
        }

        Ok(())
    }

    fn ensure_run_process_columns(connection: &Connection) -> Result<(), RegistryError> {
        let existing = Self::table_columns(connection, "runs")?;

        if !existing
            .iter()
            .any(|existing| existing == "process_start_time")
        {
            connection.execute("ALTER TABLE runs ADD COLUMN process_start_time INTEGER", [])?;
        }

        Ok(())
    }

    fn ensure_output_file_scope_columns(connection: &Connection) -> Result<(), RegistryError> {
        let existing = Self::table_columns(connection, "output_files")?;

        if existing.iter().any(|existing| existing == "output_scope") {
            return Ok(());
        }

        connection.execute_batch(
            "
            ALTER TABLE output_files RENAME TO output_files_scope_migration;

            CREATE TABLE output_files (
                id INTEGER PRIMARY KEY,
                output_name TEXT NOT NULL,
                output_scope TEXT NOT NULL DEFAULT 'unscoped',
                route_key TEXT NOT NULL,
                rendered_path TEXT NOT NULL,
                output_root TEXT,
                config_root TEXT,
                worktree_path TEXT,
                worktree_hash TEXT,
                status TEXT NOT NULL,
                reason TEXT,
                content_hash TEXT,
                template_hash TEXT,
                lease_id INTEGER,
                run_id INTEGER,
                rendered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(output_name, output_scope, route_key)
            );

            INSERT INTO output_files (
                id, output_name, output_scope, route_key, rendered_path,
                output_root, config_root, worktree_path, worktree_hash, status,
                reason, content_hash, template_hash, lease_id, run_id,
                rendered_at, updated_at
            )
            SELECT
                id, output_name, 'unscoped', route_key, rendered_path,
                NULL, NULL, NULL, NULL, status, reason, content_hash,
                template_hash, lease_id, run_id, rendered_at, updated_at
            FROM output_files_scope_migration;

            DROP TABLE output_files_scope_migration;
            ",
        )?;
        Ok(())
    }

    fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, RegistryError> {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
