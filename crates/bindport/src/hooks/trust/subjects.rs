use super::*;

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
