use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIdentity {
    pub worktree_path: PathBuf,
    pub worktree_hash: String,
    pub git_common_dir: PathBuf,
    pub branch: String,
    pub branch_label: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub project: String,
    pub service: String,
    pub git: Option<GitIdentity>,
    pub identity_key: String,
}

impl ServiceIdentity {
    pub fn port_scan_start(&self, range: PortRange) -> Option<u16> {
        stable_port_scan_start(&self.identity_key, range)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IdentitySources<'a> {
    pub cwd: &'a Path,
    pub command: &'a [String],
    pub cli_project: Option<&'a str>,
    pub cli_service: Option<&'a str>,
    pub env_project: Option<&'a str>,
    pub env_service: Option<&'a str>,
    pub config_project: Option<&'a str>,
    pub config_service: Option<&'a str>,
}
pub fn resolve_identity(sources: IdentitySources<'_>) -> ServiceIdentity {
    resolve_identity_in_scope(sources, sources.cwd)
}

pub fn resolve_identity_in_scope(
    sources: IdentitySources<'_>,
    no_git_scope: &Path,
) -> ServiceIdentity {
    let git = detect_git_identity(sources.cwd);
    let package = package_inference(sources.cwd, git.as_ref());
    let project = first_non_empty([
        sources.cli_project,
        sources.env_project,
        sources.config_project,
    ])
    .map(str::to_owned)
    .or_else(|| package.project_name())
    .unwrap_or_else(|| infer_project_name(sources.cwd, git.as_ref()));
    let service = first_non_empty([
        sources.cli_service,
        sources.env_service,
        sources.config_service,
    ])
    .map(str::to_owned)
    .or_else(|| package.service_name())
    .unwrap_or_else(|| infer_service_name(sources.command));
    let identity_key = identity_key(&project, &service, no_git_scope, git.as_ref());

    ServiceIdentity {
        project,
        service,
        git,
        identity_key,
    }
}

pub fn detect_git_identity(cwd: &Path) -> Option<GitIdentity> {
    let worktree_path = git_output(cwd, ["rev-parse", "--show-toplevel"])?;
    let worktree_path = absolute_path(cwd, PathBuf::from(worktree_path));
    let git_common_dir = git_output(cwd, ["rev-parse", "--git-common-dir"])?;
    let git_common_dir = absolute_path(cwd, PathBuf::from(git_common_dir));
    let commit = git_output(cwd, ["rev-parse", "--short", "HEAD"])?;
    let branch = git_output(cwd, ["branch", "--show-current"])
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| format!("detached-{commit}"));
    let branch_label = normalize_branch_label(&branch);
    let worktree_hash = stable_path_hash(&worktree_path);

    Some(GitIdentity {
        worktree_path,
        worktree_hash,
        git_common_dir,
        branch,
        branch_label,
        commit,
    })
}

pub fn normalize_branch_label(branch: &str) -> String {
    let mut label = String::new();
    let mut previous_was_separator = false;

    for character in branch.chars() {
        if character.is_ascii_alphanumeric() {
            label.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator && !label.is_empty() {
            label.push('-');
            previous_was_separator = true;
        }
    }

    while label.ends_with('-') {
        label.pop();
    }

    if label.is_empty() {
        String::from("branch")
    } else {
        label
    }
}

pub(crate) fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> Option<String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=")
        .arg("-c")
        .arg("core.pager=cat")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}
pub(crate) fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<&str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

pub(crate) fn infer_project_name(cwd: &Path, git: Option<&GitIdentity>) -> String {
    git.map(|git| git.worktree_path.as_path())
        .unwrap_or(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

pub(crate) fn infer_service_name(command: &[String]) -> String {
    command
        .first()
        .and_then(|program| Path::new(program).file_stem())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("command")
        .to_owned()
}
pub(crate) fn identity_key(
    project: &str,
    service: &str,
    cwd: &Path,
    git: Option<&GitIdentity>,
) -> String {
    let (path_hash, branch_label) = git
        .map(|git| (git.worktree_hash.as_str(), git.branch_label.as_str()))
        .unwrap_or_else(|| ("no-git", "no-branch"));
    let path_hash = if path_hash == "no-git" {
        stable_path_hash(&absolute_path(cwd, cwd.to_path_buf()))
    } else {
        path_hash.to_owned()
    };

    format!(
        "v1:p{}:{project}:s{}:{service}:w{path_hash}:b{}:{branch_label}",
        project.len(),
        service.len(),
        branch_label.len()
    )
}

pub(crate) fn stable_path_hash(path: &Path) -> String {
    let path = path.to_string_lossy();

    format!("{:016x}", stable_hash(path.as_bytes()))
}
