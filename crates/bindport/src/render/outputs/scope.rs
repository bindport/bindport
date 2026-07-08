use super::*;

pub(crate) fn output_file_scope(
    base_dir: &Path,
    render_config: &OutputRenderConfig,
) -> Result<OutputFileScope, RenderCommandError> {
    let output_root = output_root_path(base_dir, &render_config.context)?;
    let git = detect_git_identity(base_dir);

    Ok(OutputFileScope::new(
        output_root,
        base_dir.to_path_buf(),
        git.as_ref().map(|git| git.worktree_path.clone()),
        git.map(|git| git.worktree_hash),
    ))
}
