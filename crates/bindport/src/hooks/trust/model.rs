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
