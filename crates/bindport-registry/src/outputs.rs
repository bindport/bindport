use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileStatus {
    Pending,
    Rendered,
    Removed,
    Error,
}

impl OutputFileStatus {
    pub(crate) fn as_str(self) -> &'static str {
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

    pub(crate) fn reserve_auto_render_at(
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
}
