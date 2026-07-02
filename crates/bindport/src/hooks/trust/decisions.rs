use super::*;

pub(crate) fn upsert_hook_trust_entry(
    store: &mut HookTrustStore,
    subjects: &HookTrustSubjects,
    scope: HookTrustScope,
    hook: &EffectiveHook,
    decision: HookDecision,
) -> Result<(), String> {
    let Some(subject) = subjects.subject(scope) else {
        return Err(String::from(
            "repo scope is only available inside a git repository",
        ));
    };
    store.entries.retain(|entry| {
        !(entry.scope == scope && entry.subject == subject && entry.name == hook.name)
    });
    store.entries.push(HookTrustEntry {
        subject: subject.to_string(),
        scope,
        name: hook.name.clone(),
        decision,
        definition: hook.definition.clone(),
        target: hook.target.fingerprint.clone(),
        hook_hash: hook.hook_hash.clone(),
        target_hash: hook.target.hash.clone(),
        updated_at: unix_timestamp_string(),
    });

    Ok(())
}

pub(crate) fn reset_hook_trust_entries(
    store: &mut HookTrustStore,
    subjects: &HookTrustSubjects,
    scope: HookTrustScope,
    names: &BTreeSet<String>,
) -> usize {
    let Some(subject) = subjects.subject(scope) else {
        return 0;
    };
    let before = store.entries.len();
    store.entries.retain(|entry| {
        !(entry.scope == scope
            && entry.subject == subject
            && (names.is_empty() || names.contains(&entry.name)))
    });

    before - store.entries.len()
}

pub(crate) fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| String::from("0"))
}
