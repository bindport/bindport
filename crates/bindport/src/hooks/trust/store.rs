use super::*;

pub(crate) fn read_hook_trust_store() -> io::Result<HookTrustStore> {
    let path = hook_trust_path()?;
    if !path.is_file() {
        return Ok(HookTrustStore::default());
    }

    let contents = fs::read_to_string(path)?;
    let value = serde_json::from_str::<serde_json::Value>(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let entries = value
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| entries.iter().filter_map(parse_hook_trust_entry).collect())
        .unwrap_or_default();

    Ok(HookTrustStore { entries })
}

pub(crate) fn parse_hook_trust_entry(value: &serde_json::Value) -> Option<HookTrustEntry> {
    Some(HookTrustEntry {
        subject: value.get("subject")?.as_str()?.to_string(),
        scope: HookTrustScope::parse(value.get("scope")?.as_str()?)?,
        name: value.get("name")?.as_str()?.to_string(),
        decision: HookDecision::parse(value.get("decision")?.as_str()?)?,
        definition: value.get("definition")?.as_str()?.to_string(),
        target: value.get("target")?.as_str()?.to_string(),
        hook_hash: value.get("hook_hash")?.as_str()?.to_string(),
        target_hash: value.get("target_hash")?.as_str()?.to_string(),
        updated_at: value.get("updated_at")?.as_str()?.to_string(),
    })
}

pub(crate) fn write_hook_trust_store(store: &HookTrustStore) -> io::Result<()> {
    let path = hook_trust_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let entries = store
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "subject": entry.subject,
                "scope": entry.scope.as_str(),
                "name": entry.name,
                "decision": entry.decision.as_str(),
                "definition": entry.definition,
                "target": entry.target,
                "hook_hash": entry.hook_hash,
                "target_hash": entry.target_hash,
                "updated_at": entry.updated_at,
            })
        })
        .collect::<Vec<_>>();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": HOOK_TRUST_SCHEMA_VERSION,
        "entries": entries,
    }))
    .map_err(io::Error::other)?;

    fs::write(path, format!("{json}\n"))
}
