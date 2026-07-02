use super::*;

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
