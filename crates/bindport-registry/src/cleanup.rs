use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanState {
    Stopped,
    Stale,
}

impl CleanState {
    pub(crate) fn as_str(self) -> &'static str {
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

    pub(crate) fn add_leases(&mut self, state: CleanState, count: usize) {
        match state {
            CleanState::Stopped => self.stopped_leases += count,
            CleanState::Stale => self.stale_leases += count,
        }
    }
}

impl Registry {
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

    pub fn prune_oldest_stale_leases(
        &mut self,
        start_port: u16,
        end_port: u16,
        max_total_leases: usize,
        dry_run: bool,
    ) -> Result<CleanSummary, RegistryError> {
        if start_port > end_port {
            return Ok(CleanSummary::default());
        }

        self.reconcile_stale_active_leases()?;

        let total_leases = self.connection.query_row(
            "SELECT COUNT(*)
             FROM leases
             WHERE port BETWEEN ?1 AND ?2",
            params![start_port, end_port],
            |row| row.get::<_, i64>(0),
        )? as usize;

        if total_leases <= max_total_leases {
            return Ok(CleanSummary::default());
        }

        let prune_count = total_leases - max_total_leases;
        let transaction = self.connection.transaction()?;
        let stale_lease_ids = {
            let mut statement = transaction.prepare(
                "SELECT id
                 FROM leases
                 WHERE state = 'stale'
                 AND port BETWEEN ?1 AND ?2
                 ORDER BY COALESCE(released_at, last_seen_at, allocated_at), id
                 LIMIT ?3",
            )?;
            let rows = statement
                .query_map(params![start_port, end_port, prune_count as i64], |row| {
                    row.get(0)
                })?;

            rows.collect::<Result<Vec<i64>, _>>()?
        };

        if stale_lease_ids.is_empty() {
            transaction.commit()?;
            return Ok(CleanSummary::default());
        }

        let mut summary = CleanSummary {
            stale_leases: stale_lease_ids.len(),
            ..CleanSummary::default()
        };

        for lease_id in &stale_lease_ids {
            summary.runs += transaction.query_row(
                "SELECT COUNT(*)
                 FROM runs
                 WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get::<_, i64>(0),
            )? as usize;
        }

        if dry_run {
            transaction.commit()?;
            return Ok(summary);
        }

        for lease_id in &stale_lease_ids {
            transaction.execute(
                "DELETE FROM runs
                 WHERE lease_id = ?1",
                params![lease_id],
            )?;
            transaction.execute(
                "DELETE FROM leases
                 WHERE id = ?1",
                params![lease_id],
            )?;
        }

        transaction.commit()?;

        Ok(summary)
    }
}
