use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct HookTrustStore {
    pub(crate) entries: Vec<HookTrustEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookTrustEntry {
    pub(crate) subject: String,
    pub(crate) scope: HookTrustScope,
    pub(crate) name: String,
    pub(crate) decision: HookDecision,
    pub(crate) definition: String,
    pub(crate) target: String,
    pub(crate) hook_hash: String,
    pub(crate) target_hash: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct HookTrustSubjects {
    pub(crate) worktree: String,
    pub(crate) repo: Option<String>,
}

impl HookTrustSubjects {
    pub(crate) fn subject(&self, scope: HookTrustScope) -> Option<&str> {
        match scope {
            HookTrustScope::Worktree => Some(&self.worktree),
            HookTrustScope::Repo => self.repo.as_deref(),
        }
    }
}

pub(crate) fn hook_trust_subjects(cwd: &Path) -> HookTrustSubjects {
    match detect_git_identity(cwd) {
        Some(git) => HookTrustSubjects {
            worktree: format!("worktree:{}", git.worktree_path.display()),
            repo: Some(format!("repo:{}", git.git_common_dir.display())),
        },
        None => {
            let path = cwd
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(cwd));
            HookTrustSubjects {
                worktree: format!("path:{}", path.display()),
                repo: None,
            }
        }
    }
}

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

pub(crate) fn hook_trust_status(
    hook: &EffectiveHook,
    store: &HookTrustStore,
    subjects: &HookTrustSubjects,
) -> HookTrustStatus {
    for scope in [HookTrustScope::Worktree, HookTrustScope::Repo] {
        let Some(subject) = subjects.subject(scope) else {
            continue;
        };
        if let Some(entry) = store.entries.iter().find(|entry| {
            entry.scope == scope
                && entry.subject == subject
                && entry.name == hook.name
                && entry.definition == hook.definition
                && entry.target == hook.target.fingerprint
        }) {
            return match entry.decision {
                HookDecision::Approved => HookTrustStatus::Approved { scope },
                HookDecision::Denied => HookTrustStatus::Denied { scope },
            };
        }
    }

    for scope in [HookTrustScope::Worktree, HookTrustScope::Repo] {
        let Some(subject) = subjects.subject(scope) else {
            continue;
        };
        if store.entries.iter().any(|entry| {
            entry.scope == scope && entry.subject == subject && entry.name == hook.name
        }) {
            return HookTrustStatus::Changed;
        }
    }

    HookTrustStatus::Pending
}

pub(crate) fn hook_statuses_for_current_dir(
    cwd: &Path,
    config: &ResolvedConfig,
) -> Vec<HookStatus> {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        return Vec::new();
    };
    let store = read_hook_trust_store().unwrap_or_default();
    let subjects = hook_trust_subjects(cwd);

    plan.hooks
        .into_iter()
        .map(|hook| {
            let trust = hook_trust_status(&hook, &store, &subjects);
            HookStatus { hook, trust }
        })
        .collect()
}

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
