use super::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderDiffSummary {
    pub(crate) added: usize,
    pub(crate) modified: usize,
    pub(crate) removed: usize,
    pub(crate) unchanged: usize,
    pub(crate) missing: usize,
    pub(crate) outside_root: usize,
    pub(crate) external_modified: usize,
}

impl RenderDiffSummary {
    pub(crate) fn changed_files(self) -> usize {
        self.added + self.modified + self.removed
    }
}

pub(crate) fn print_render_diff(
    output_name: &str,
    base_dir: &Path,
    diffs: &[DiffedOutputFile],
    removals: &[DiffedRemovalOutputFile],
) -> RenderDiffSummary {
    let mut summary = RenderDiffSummary::default();

    for diff in diffs {
        match diff.status {
            OutputFileDiffStatus::Added => summary.added += 1,
            OutputFileDiffStatus::Modified => summary.modified += 1,
            OutputFileDiffStatus::Unchanged => summary.unchanged += 1,
        }
    }
    for removal in removals {
        match removal.status {
            AdapterOutputFileRemovalStatus::Removed => summary.removed += 1,
            AdapterOutputFileRemovalStatus::Missing => summary.missing += 1,
            AdapterOutputFileRemovalStatus::OutsideRoot => summary.outside_root += 1,
            AdapterOutputFileRemovalStatus::ExternalModified => summary.external_modified += 1,
        }
    }

    println!(
        "diff {output_name}: {} added, {} modified, {} removed, {} unchanged",
        summary.added, summary.modified, summary.removed, summary.unchanged
    );
    if summary.missing > 0 {
        println!("missing {output_name}: {} DB-owned files", summary.missing);
    }
    if summary.outside_root > 0 {
        println!(
            "outside root {output_name}: {} DB-owned files",
            summary.outside_root
        );
    }
    if summary.external_modified > 0 {
        println!(
            "external modified {output_name}: {} DB-owned files",
            summary.external_modified
        );
    }

    for diff in diffs {
        match diff.status {
            OutputFileDiffStatus::Added => print_file_diff(
                "added",
                &display_target(base_dir, &diff.path, Some(&diff.target)),
                "",
                &diff.new_contents,
            ),
            OutputFileDiffStatus::Modified => print_file_diff(
                "modified",
                &display_target(base_dir, &diff.path, Some(&diff.target)),
                diff.old_contents.as_deref().unwrap_or_default(),
                &diff.new_contents,
            ),
            OutputFileDiffStatus::Unchanged => {}
        }
    }
    for removal in removals {
        if removal.status == AdapterOutputFileRemovalStatus::Removed {
            print_file_diff(
                "removed",
                &display_target(base_dir, &removal.path, None),
                removal.old_contents.as_deref().unwrap_or_default(),
                "",
            );
        }
    }

    summary
}

fn display_target(base_dir: &Path, path: &Path, target: Option<&str>) -> String {
    target
        .filter(|target| !target.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            path.strip_prefix(base_dir)
                .unwrap_or(path)
                .display()
                .to_string()
        })
}

fn print_file_diff(status: &str, label: &str, old_contents: &str, new_contents: &str) {
    println!();
    println!("diff --bindport {status} {label}");
    let old_label = if status == "added" {
        "/dev/null"
    } else {
        label
    };
    let new_label = if status == "removed" {
        "/dev/null"
    } else {
        label
    };
    println!("--- {old_label}");
    println!("+++ {new_label}");
    print_line_diff(old_contents, new_contents);
}

fn print_line_diff(old_contents: &str, new_contents: &str) {
    let old_lines = old_contents.lines().collect::<Vec<_>>();
    let new_lines = new_contents.lines().collect::<Vec<_>>();
    let prefix = common_prefix_len(&old_lines, &new_lines);
    let suffix = common_suffix_len(&old_lines, &new_lines, prefix);
    let old_changed_end = old_lines.len().saturating_sub(suffix);
    let new_changed_end = new_lines.len().saturating_sub(suffix);
    let before_start = prefix.saturating_sub(3);
    let after_end = (new_changed_end + 3).min(new_lines.len());

    println!("@@");
    for line in &old_lines[before_start..prefix] {
        println!(" {line}");
    }
    for line in &old_lines[prefix..old_changed_end] {
        println!("-{line}");
    }
    for line in &new_lines[prefix..new_changed_end] {
        println!("+{line}");
    }
    for line in &new_lines[new_changed_end..after_end] {
        println!(" {line}");
    }
}

fn common_prefix_len<'a>(old_lines: &[&'a str], new_lines: &[&'a str]) -> usize {
    old_lines
        .iter()
        .zip(new_lines)
        .take_while(|(old, new)| old == new)
        .count()
}

fn common_suffix_len<'a>(old_lines: &[&'a str], new_lines: &[&'a str], prefix: usize) -> usize {
    old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count()
}
