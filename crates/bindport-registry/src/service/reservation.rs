use super::*;

const RESERVATION_COMMIT_RETRIES: usize = 3;

impl Registry {
    pub fn reserve_services<E>(
        &mut self,
        identities: &[ServiceIdentity],
        plan: impl FnMut(&ServiceIdentity, &[u16], Option<u16>) -> Result<ReservationCandidate, E>,
    ) -> Result<Vec<RegistryService>, BatchReservationError<E>> {
        self.reserve_services_with_preflight(identities, plan, |_, _| Ok(()))
            .map(|services| services.into_iter().map(|(service, _)| service).collect())
    }

    pub fn reserve_service<E>(
        &mut self,
        identity: &ServiceIdentity,
        plan: impl FnMut(&ServiceIdentity, &[u16], Option<u16>) -> Result<ReservationCandidate, E>,
    ) -> Result<(RegistryService, bool), BatchReservationError<E>> {
        self.reserve_services_with_preflight(std::slice::from_ref(identity), plan, |_, _| Ok(()))
            .map(|mut services| services.remove(0))
    }

    /// Preflights all newly planned candidates before opening the write transaction.
    ///
    /// Preflight is skipped when nothing needs reserving and repeated on replanning.
    /// A preflight error aborts this batch's inserts; stale reconciliation and
    /// callback side effects are not rolled back. Callbacks run outside the
    /// transaction, so external state may change afterward. Results follow input
    /// order; each bool is true only when this call committed a new reservation,
    /// not when it reused another writer's.
    pub fn reserve_services_with_preflight<E>(
        &mut self,
        identities: &[ServiceIdentity],
        mut plan: impl FnMut(&ServiceIdentity, &[u16], Option<u16>) -> Result<ReservationCandidate, E>,
        mut preflight: impl FnMut(
            &mut Self,
            &[(&ServiceIdentity, &ReservationCandidate)],
        ) -> Result<(), E>,
    ) -> Result<Vec<(RegistryService, bool)>, BatchReservationError<E>> {
        if identities.is_empty() {
            return Ok(Vec::new());
        }
        self.reconcile_stale_active_leases()?;
        let mut last_conflict = None;
        let mut last_retry_identity = None;

        for _ in 0..=RESERVATION_COMMIT_RETRIES {
            last_conflict = None;
            let snapshot = reservation_snapshot(&mut self.connection, identities)?;
            let mut occupied_ports = snapshot.occupied_ports;
            let mut candidates = Vec::with_capacity(identities.len());
            for ((identity, existing), previous_port) in identities
                .iter()
                .zip(snapshot.services)
                .zip(snapshot.previous_ports)
            {
                if existing.is_some() {
                    candidates.push(None);
                    continue;
                }
                let candidate = plan(identity, &occupied_ports, previous_port)
                    .map_err(BatchReservationError::Plan)?;
                occupied_ports.push(candidate.port);
                candidates.push(Some(candidate));
            }

            let planned = identities
                .iter()
                .zip(&candidates)
                .filter_map(|(identity, candidate)| {
                    candidate.as_ref().map(|value| (identity, value))
                })
                .collect::<Vec<_>>();
            if !planned.is_empty() {
                preflight(self, &planned).map_err(BatchReservationError::Plan)?;
            }

            let now = utc_now(&self.connection)?;
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(RegistryError::from)?;
            let mut committed_ports = active_and_reserved_ports(&transaction)?;
            let mut services = Vec::with_capacity(identities.len());
            let mut retry = false;

            for (identity, candidate) in identities.iter().zip(candidates) {
                if let Some(service) = select_scoped_service(&transaction, identity)? {
                    services.push((service, false));
                    continue;
                }
                let Some(candidate) = candidate else {
                    last_retry_identity = Some(identity);
                    retry = true;
                    break;
                };
                if committed_ports.contains(&candidate.port) {
                    last_conflict = Some(candidate.port);
                    last_retry_identity = Some(identity);
                    retry = true;
                    break;
                }

                let git = identity.git.as_ref();
                transaction
                    .execute(
                        "INSERT INTO leases (
                            project, service, worktree_path, worktree_hash, git_common_dir,
                            branch, branch_label, git_commit, identity_key, port, host,
                            hostname, route_url, health_url, state, allocated_at, last_seen_at
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                            ?14, 'reserved', ?15, ?15
                         )",
                        params![
                            identity.project,
                            identity.service,
                            git.map(|git| git.worktree_path.display().to_string()),
                            git.map(|git| git.worktree_hash.as_str()),
                            git.map(|git| git.git_common_dir.display().to_string()),
                            git.map(|git| git.branch.as_str()),
                            git.map(|git| git.branch_label.as_str()),
                            git.map(|git| git.commit.as_str()),
                            identity.identity_key,
                            candidate.port,
                            candidate.host,
                            candidate.hostname,
                            candidate.route_url,
                            candidate.health_url,
                            now,
                        ],
                    )
                    .map_err(RegistryError::from)?;
                let lease_id = transaction.last_insert_rowid();
                committed_ports.push(candidate.port);
                services.push((
                    RegistryService {
                        lease_id,
                        project: identity.project.clone(),
                        service: identity.service.clone(),
                        identity_key: identity.identity_key.clone(),
                        state: String::from("reserved"),
                        host: candidate.host,
                        port: candidate.port,
                        hostname: candidate.hostname,
                        route_url: candidate.route_url,
                        health_url: candidate.health_url,
                    },
                    true,
                ));
            }

            if retry {
                drop(transaction);
                continue;
            }
            transaction.commit().map_err(RegistryError::from)?;
            return Ok(services);
        }

        if let Some(port) = last_conflict {
            return Err(RegistryError::PortConflict { port }.into());
        }
        let identity = last_retry_identity
            .expect("exhausted reservation retries must have an invalidating identity");
        Err(RegistryError::ConcurrentReservation {
            project: identity.project.clone(),
            service: identity.service.clone(),
        }
        .into())
    }
}

struct ReservationSnapshot {
    occupied_ports: Vec<u16>,
    services: Vec<Option<RegistryService>>,
    previous_ports: Vec<Option<u16>>,
}

fn reservation_snapshot(
    connection: &mut Connection,
    identities: &[ServiceIdentity],
) -> Result<ReservationSnapshot, RegistryError> {
    let transaction = connection.transaction()?;
    let occupied_ports = active_and_reserved_ports(&transaction)?;
    let services = identities
        .iter()
        .map(|identity| select_scoped_service(&transaction, identity))
        .collect::<Result<Vec<_>, _>>()?;
    let previous_ports = identities
        .iter()
        .map(|identity| previous_identity_port(&transaction, &identity.identity_key))
        .collect::<Result<Vec<_>, _>>()?;
    transaction.commit()?;

    Ok(ReservationSnapshot {
        occupied_ports,
        services,
        previous_ports,
    })
}
