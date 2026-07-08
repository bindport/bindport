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

pub const UNSCOPED_OUTPUT_SCOPE: &str = "unscoped";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFileScope {
    pub key: String,
    pub output_root: Option<PathBuf>,
    pub config_root: Option<PathBuf>,
    pub worktree_path: Option<PathBuf>,
    pub worktree_hash: Option<String>,
}

impl OutputFileScope {
    pub fn new(
        output_root: PathBuf,
        config_root: PathBuf,
        worktree_path: Option<PathBuf>,
        worktree_hash: Option<String>,
    ) -> Self {
        let key = output_scope_key(&output_root, &config_root);

        Self {
            key,
            output_root: Some(output_root),
            config_root: Some(config_root),
            worktree_path,
            worktree_hash,
        }
    }

    pub fn unscoped() -> Self {
        Self {
            key: String::from(UNSCOPED_OUTPUT_SCOPE),
            output_root: None,
            config_root: None,
            worktree_path: None,
            worktree_hash: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFileRecord {
    pub output_name: String,
    pub scope: OutputFileScope,
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
        scope: &OutputFileScope,
    ) -> Result<Vec<OutputFileOwnership>, RegistryError> {
        let mut statement = self.connection.prepare(
            "SELECT output_scope, route_key, rendered_path, content_hash
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
            let output_scope = row.get::<_, String>(0)?;
            let path = PathBuf::from(row.get::<_, String>(2)?);

            Ok(OutputFileOwnershipRow {
                route_key: row.get(1)?,
                path,
                content_hash: row.get(3)?,
                output_scope,
            })
        })?;

        let mut by_path = std::collections::BTreeMap::<PathBuf, OutputFileOwnershipRow>::new();
        for owned in rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|owned| output_file_scope_matches(owned, scope))
        {
            let should_replace = by_path.get(&owned.path).is_some_and(|existing| {
                owned.output_scope == scope.key && existing.output_scope != scope.key
            });
            if should_replace || !by_path.contains_key(&owned.path) {
                by_path.insert(owned.path.clone(), owned);
            }
        }
        let ownership = by_path
            .into_values()
            .map(|owned| OutputFileOwnership {
                route_key: owned.route_key,
                path: owned.path,
                content_hash: owned.content_hash,
            })
            .collect();

        Ok(ownership)
    }

    pub fn record_output_file(&mut self, record: &OutputFileRecord) -> Result<(), RegistryError> {
        let now = utc_now(&self.connection)?;
        let rendered_path = record.rendered_path.display().to_string();
        let output_root = record
            .scope
            .output_root
            .as_ref()
            .map(|path| path.display().to_string());
        let config_root = record
            .scope
            .config_root
            .as_ref()
            .map(|path| path.display().to_string());
        let worktree_path = record
            .scope
            .worktree_path
            .as_ref()
            .map(|path| path.display().to_string());

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if record.scope.key != UNSCOPED_OUTPUT_SCOPE {
            transaction.execute(
                "DELETE FROM output_files
                 WHERE output_name = ?1
                 AND output_scope = ?2
                 AND route_key = ?3",
                params![
                    &record.output_name,
                    UNSCOPED_OUTPUT_SCOPE,
                    &record.route_key
                ],
            )?;
        }

        transaction.execute(
            "INSERT INTO output_files (
                output_name, output_scope, route_key, rendered_path, output_root,
                config_root, worktree_path, worktree_hash, status, reason,
                content_hash, template_hash, lease_id, run_id, rendered_at,
                updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?15
             )
             ON CONFLICT(output_name, output_scope, route_key) DO UPDATE SET
                rendered_path = excluded.rendered_path,
                output_root = excluded.output_root,
                config_root = excluded.config_root,
                worktree_path = excluded.worktree_path,
                worktree_hash = excluded.worktree_hash,
                status = excluded.status,
                reason = excluded.reason,
                content_hash = excluded.content_hash,
                template_hash = excluded.template_hash,
                lease_id = excluded.lease_id,
                run_id = excluded.run_id,
                updated_at = excluded.updated_at",
            params![
                &record.output_name,
                &record.scope.key,
                &record.route_key,
                rendered_path,
                output_root,
                config_root,
                worktree_path,
                &record.scope.worktree_hash,
                record.status.as_str(),
                &record.reason,
                &record.content_hash,
                &record.template_hash,
                record.lease_id,
                record.run_id,
                now
            ],
        )?;
        transaction.commit()?;

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

#[derive(Debug)]
struct OutputFileOwnershipRow {
    route_key: String,
    path: PathBuf,
    content_hash: String,
    output_scope: String,
}

fn output_file_scope_matches(owned: &OutputFileOwnershipRow, scope: &OutputFileScope) -> bool {
    if owned.output_scope == scope.key {
        return true;
    }

    owned.output_scope == UNSCOPED_OUTPUT_SCOPE
        && scope
            .output_root
            .as_ref()
            .is_some_and(|root| owned.path.starts_with(root))
}

fn output_scope_key(output_root: &Path, config_root: &Path) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    let values = [
        String::from("v1"),
        output_root.display().to_string(),
        config_root.display().to_string(),
    ];

    for value in values {
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{hash:016x}")
}
