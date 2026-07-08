use super::*;

impl Registry {
    pub(crate) fn ensure_schema(&mut self) -> Result<(), RegistryError> {
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
        self.ensure_lease_identity_columns()?;
        self.ensure_run_process_columns()?;
        self.connection.execute_batch(
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
        self.ensure_output_file_scope_columns()?;
        self.connection.execute_batch(
            "

            CREATE INDEX IF NOT EXISTS output_files_output_path_idx
            ON output_files(output_name, output_scope, rendered_path);

            CREATE INDEX IF NOT EXISTS output_files_route_key_idx
            ON output_files(route_key);

            CREATE TABLE IF NOT EXISTS output_render_state (
                output_name TEXT PRIMARY KEY,
                last_render_at_ms INTEGER NOT NULL
            );

            PRAGMA user_version = 9;
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

    fn ensure_run_process_columns(&self) -> Result<(), RegistryError> {
        let existing = self.run_columns()?;

        if !existing
            .iter()
            .any(|existing| existing == "process_start_time")
        {
            self.connection
                .execute("ALTER TABLE runs ADD COLUMN process_start_time INTEGER", [])?;
        }

        Ok(())
    }

    fn run_columns(&self) -> Result<Vec<String>, RegistryError> {
        let mut statement = self.connection.prepare("PRAGMA table_info(runs)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn ensure_output_file_scope_columns(&mut self) -> Result<(), RegistryError> {
        let existing = self.output_file_columns()?;

        if existing.iter().any(|existing| existing == "output_scope") {
            return Ok(());
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(
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
        transaction.commit()?;

        Ok(())
    }

    fn output_file_columns(&self) -> Result<Vec<String>, RegistryError> {
        let mut statement = self.connection.prepare("PRAGMA table_info(output_files)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
