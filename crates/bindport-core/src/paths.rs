use super::*;

pub(crate) fn absolute_path(cwd: &Path, path: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };

    path.canonicalize().unwrap_or(path)
}
