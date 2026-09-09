use super::*;

impl Registry {
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
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
