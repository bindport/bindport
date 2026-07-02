use super::*;

#[derive(Debug, Clone)]
pub(crate) struct EffectiveHook {
    pub(crate) name: String,
    pub(crate) events: Vec<HookEvent>,
    pub(crate) command: Vec<String>,
    pub(crate) timeout: Duration,
    pub(crate) timeout_ms: u64,
    pub(crate) source: String,
    pub(crate) definition: String,
    pub(crate) hook_hash: String,
    pub(crate) target: HookTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTrustScope {
    Worktree,
    Repo,
}

impl HookTrustScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::Repo => "repo",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "worktree" => Some(Self::Worktree),
            "repo" => Some(Self::Repo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HookPlan {
    pub(crate) hooks: Vec<EffectiveHook>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookTarget {
    pub(crate) kind: HookTargetKind,
    pub(crate) display: String,
    pub(crate) fingerprint: String,
    pub(crate) hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTargetKind {
    LocalFile,
    MissingFile,
    Opaque,
}

impl HookTargetKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LocalFile => "local_file",
            Self::MissingFile => "missing_file",
            Self::Opaque => "opaque",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookDecision {
    Approved,
    Denied,
}

impl HookDecision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "approved" => Some(Self::Approved),
            "denied" => Some(Self::Denied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTrustStatus {
    Approved { scope: HookTrustScope },
    Denied { scope: HookTrustScope },
    Changed,
    Pending,
}

impl HookTrustStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Approved { .. } => "approved",
            Self::Denied { .. } => "denied",
            Self::Changed => "changed",
            Self::Pending => "pending",
        }
    }

    pub(crate) const fn is_approved(self) -> bool {
        matches!(self, Self::Approved { .. })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HookStatus {
    pub(crate) hook: EffectiveHook,
    pub(crate) trust: HookTrustStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookRunMode {
    Run,
    DryRun,
}

#[derive(Debug)]
pub(crate) enum HookExecutionError {
    Spawn { command: String, source: io::Error },
    Wait { command: String, source: io::Error },
    Timeout { command: String, timeout: Duration },
    Failed { command: String, status: ExitStatus },
}

impl std::fmt::Display for HookExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn { command, source } => {
                write!(f, "failed to spawn hook `{command}`: {source}")
            }
            Self::Wait { command, source } => {
                write!(f, "failed waiting for hook `{command}`: {source}")
            }
            Self::Timeout { command, timeout } => {
                write!(
                    f,
                    "hook `{command}` timed out after {}ms",
                    timeout.as_millis()
                )
            }
            Self::Failed { command, status } => {
                write!(f, "hook `{command}` exited with {status}")
            }
        }
    }
}

impl std::error::Error for HookExecutionError {}
