use super::*;

pub(crate) fn hook_target(cwd: &Path, command: &[String]) -> HookTarget {
    let Some(program) = command.first().map(String::as_str) else {
        return opaque_hook_target("<empty>");
    };

    if !path_like_command(program) {
        return opaque_hook_target(program);
    }

    let path = PathBuf::from(program);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let display_path = path.display().to_string();

    match fs::read(&path) {
        Ok(contents) => {
            let resolved = path
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(&path));
            let fingerprint = format!(
                "file:{}:{}:{}",
                program,
                contents.len(),
                stable_hex_hash(&contents)
            );
            HookTarget {
                kind: HookTargetKind::LocalFile,
                display: resolved.display().to_string(),
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
        Err(_) => {
            let fingerprint = format!("missing:{program}");
            HookTarget {
                kind: HookTargetKind::MissingFile,
                display: display_path,
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
    }
}

pub(crate) fn opaque_hook_target(program: &str) -> HookTarget {
    let fingerprint = format!("opaque:{program}");
    HookTarget {
        kind: HookTargetKind::Opaque,
        display: program.to_string(),
        hash: stable_hex_hash(fingerprint.as_bytes()),
        fingerprint,
    }
}

pub(crate) fn path_like_command(program: &str) -> bool {
    program.contains('/') || program.contains('\\') || program.starts_with('.')
}

pub(crate) fn path_clean_display_path(path: &Path) -> PathBuf {
    path.components().collect()
}
