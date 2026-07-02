use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageInference {
    pub(crate) root: Option<PackageMetadata>,
    pub(crate) nearest: Option<PackageMetadata>,
}

impl PackageInference {
    pub(crate) fn project_name(&self) -> Option<String> {
        self.root
            .as_ref()
            .or(self.nearest.as_ref())
            .map(|package| package.identity_name.clone())
    }

    pub(crate) fn service_name(&self) -> Option<String> {
        self.nearest
            .as_ref()
            .map(|package| package.identity_name.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageMetadata {
    pub(crate) identity_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceRoot {
    pub(crate) path: PathBuf,
    pub(crate) metadata: PackageMetadata,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PnpmWorkspaceConfig {
    pub(crate) packages: Option<Vec<String>>,
}

pub(crate) fn package_inference(cwd: &Path, git: Option<&GitIdentity>) -> PackageInference {
    let git_boundary = git.map(|git| git.worktree_path.as_path());
    let workspace_root = nearest_workspace_root(cwd, git_boundary);
    let package_boundary = workspace_root
        .as_ref()
        .map(|workspace| workspace.path.as_path())
        .or(git_boundary);
    let root = workspace_root
        .as_ref()
        .map(|workspace| workspace.metadata.clone())
        .or_else(|| git.and_then(|git| read_package_metadata(&git.worktree_path)));
    let nearest = nearest_package_metadata(cwd, package_boundary);

    PackageInference { root, nearest }
}

pub(crate) fn nearest_workspace_root(cwd: &Path, boundary: Option<&Path>) -> Option<WorkspaceRoot> {
    let cwd = absolute_path(cwd, cwd.to_path_buf());

    for directory in cwd.ancestors() {
        if let Some(boundary) = boundary
            && !directory.starts_with(boundary)
        {
            break;
        }

        if is_workspace_root(directory) {
            return Some(WorkspaceRoot {
                path: directory.to_path_buf(),
                metadata: workspace_root_metadata(directory),
            });
        }

        if Some(directory) == boundary {
            break;
        }
    }

    None
}

pub(crate) fn is_workspace_root(directory: &Path) -> bool {
    package_json_has_workspaces(directory) || pnpm_workspace_has_packages(directory)
}

pub(crate) fn package_json_has_workspaces(directory: &Path) -> bool {
    let contents = fs::read_to_string(directory.join("package.json")).ok();
    let Some(value) = contents
        .as_deref()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(contents).ok())
    else {
        return false;
    };

    workspace_packages_present(value.get("workspaces"))
}

pub(crate) fn workspace_packages_present(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Array(packages)) => packages.iter().any(non_empty_json_string),
        Some(serde_json::Value::Object(workspace)) => workspace
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|packages| packages.iter().any(non_empty_json_string)),
        _ => false,
    }
}

pub(crate) fn non_empty_json_string(value: &serde_json::Value) -> bool {
    value
        .as_str()
        .is_some_and(|package| !package.trim().is_empty())
}

pub(crate) fn pnpm_workspace_has_packages(directory: &Path) -> bool {
    let contents = fs::read_to_string(directory.join("pnpm-workspace.yaml")).ok();
    let Some(config) = contents
        .as_deref()
        .and_then(|contents| serde_yaml_ng::from_str::<PnpmWorkspaceConfig>(contents).ok())
    else {
        return false;
    };

    config
        .packages
        .is_some_and(|packages| packages.iter().any(|package| !package.trim().is_empty()))
}

pub(crate) fn workspace_root_metadata(directory: &Path) -> PackageMetadata {
    read_package_metadata(directory).unwrap_or_else(|| PackageMetadata {
        identity_name: directory_identity_name(directory),
    })
}

pub(crate) fn nearest_package_metadata(
    cwd: &Path,
    boundary: Option<&Path>,
) -> Option<PackageMetadata> {
    let cwd = absolute_path(cwd, cwd.to_path_buf());

    for directory in cwd.ancestors() {
        if let Some(boundary) = boundary
            && !directory.starts_with(boundary)
        {
            break;
        }

        if let Some(package) = read_package_metadata(directory) {
            return Some(package);
        }

        if Some(directory) == boundary {
            break;
        }
    }

    None
}

pub(crate) fn read_package_metadata(directory: &Path) -> Option<PackageMetadata> {
    let contents = fs::read_to_string(directory.join("package.json")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
    let name = value.get("name")?.as_str()?;
    let identity_name = package_identity_name(name)?;

    Some(PackageMetadata { identity_name })
}

pub(crate) fn directory_identity_name(directory: &Path) -> String {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(package_identity_name)
        .unwrap_or(None)
        .unwrap_or_else(|| String::from("workspace"))
}

pub(crate) fn package_identity_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let name = if let Some(scoped) = name.strip_prefix('@') {
        scoped.split_once('/').map(|(_, package)| package)?
    } else {
        name
    };
    let name = name.trim();

    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}
