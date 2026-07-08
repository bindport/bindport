// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn write_render_plan_writes_new_files_under_root() {
    let root = temp_test_dir("write-plan-new");
    let plan = test_render_plan("routes/demo.yml", "first");

    let written = write_render_plan(&plan, &root, &[]).expect("write plan");

    assert_eq!(written.len(), 1);
    assert_eq!(written[0].path, root.join(".bindport/out/routes/demo.yml"));
    assert_eq!(
        fs::read_to_string(&written[0].path).expect("rendered file"),
        "first"
    );
    assert_eq!(written[0].content_hash, content_hash("first"));
}

#[cfg(unix)]
#[test]
fn write_render_plan_allows_symlinked_base_directory() {
    let real_root = temp_test_dir("write-plan-real-base");
    let link_parent = temp_test_dir("write-plan-link-parent");
    let link_root = link_parent.join("base-link");
    std::os::unix::fs::symlink(&real_root, &link_root).expect("symlink base dir");
    let plan = test_render_plan("routes/demo.yml", "first");

    let written = write_render_plan(&plan, &link_root, &[]).expect("write plan");

    assert_eq!(
        fs::read_to_string(&written[0].path).expect("rendered file"),
        "first"
    );
    assert_eq!(
        written[0].path,
        link_root.join(".bindport/out/routes/demo.yml")
    );
}

#[cfg(unix)]
#[test]
fn write_render_plan_rejects_symlink_below_output_root() {
    let root = temp_test_dir("write-plan-symlink-target");
    let outside = temp_test_dir("write-plan-symlink-outside");
    let symlink_path = root.join(".bindport/out/routes");
    fs::create_dir_all(symlink_path.parent().expect("parent")).expect("parent dir");
    std::os::unix::fs::symlink(&outside, &symlink_path).expect("symlink target dir");
    let plan = test_render_plan("routes/demo.yml", "first");

    let error = write_render_plan(&plan, &root, &[]).expect_err("symlink below root");

    assert!(matches!(
        error,
        OutputFileError::SymlinkInPath { path } if path == symlink_path
    ));
}

#[test]
fn write_render_plan_refuses_unowned_existing_file() {
    let root = temp_test_dir("write-plan-unowned");
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "external").expect("external file");
    let plan = test_render_plan("routes/demo.yml", "first");

    let error = write_render_plan(&plan, &root, &[]).expect_err("unowned file");

    assert!(matches!(
        error,
        OutputFileError::UnownedTarget { path: error_path } if error_path == path
    ));
}

#[test]
fn write_render_plan_overwrites_owned_file_when_hash_matches() {
    let root = temp_test_dir("write-plan-owned");
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "old").expect("old file");
    let plan = test_render_plan("routes/demo.yml", "new");

    let written = write_render_plan(
        &plan,
        &root,
        &[OutputFileOwnership {
            path: path.clone(),
            content_hash: content_hash("old"),
        }],
    )
    .expect("overwrite owned file");

    assert_eq!(written[0].content_hash, content_hash("new"));
    assert_eq!(fs::read_to_string(&path).expect("rendered file"), "new");
}

#[test]
fn verify_render_plan_targets_checks_ownership_without_writing() {
    let root = temp_test_dir("verify-plan-owned");
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "old").expect("old file");
    let plan = test_render_plan("routes/demo.yml", "new");

    let planned = verify_render_plan_targets(
        &plan,
        &root,
        &[OutputFileOwnership {
            path: path.clone(),
            content_hash: content_hash("old"),
        }],
    )
    .expect("owned target verifies");

    assert_eq!(planned[0].path, path);
    assert_eq!(
        fs::read_to_string(&planned[0].path).expect("unchanged"),
        "old"
    );

    let unowned = verify_render_plan_targets(&plan, &root, &[]).expect_err("unowned target");
    assert!(matches!(
        unowned,
        OutputFileError::UnownedTarget { path: error_path } if error_path == planned[0].path
    ));
}

#[test]
fn diff_render_plan_reports_changes_without_writing() {
    let root = temp_test_dir("diff-plan-owned");
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "old\nstable\n").expect("old file");
    let plan = test_render_plan("routes/demo.yml", "new\nstable\n");

    let diffed = diff_render_plan(
        &plan,
        &root,
        &[OutputFileOwnership {
            path: path.clone(),
            content_hash: content_hash("old\nstable\n"),
        }],
    )
    .expect("diff plan");

    assert_eq!(diffed.len(), 1);
    assert_eq!(diffed[0].status, OutputFileDiffStatus::Modified);
    assert_eq!(diffed[0].old_contents.as_deref(), Some("old\nstable\n"));
    assert_eq!(diffed[0].new_contents, "new\nstable\n");
    assert_eq!(
        fs::read_to_string(&path).expect("unchanged file"),
        "old\nstable\n"
    );
}

#[test]
fn diff_render_plan_reports_added_files_without_writing() {
    let root = temp_test_dir("diff-plan-added");
    let path = root.join(".bindport/out/routes/demo.yml");
    let plan = test_render_plan("routes/demo.yml", "new\n");

    let diffed = diff_render_plan(&plan, &root, &[]).expect("diff plan");

    assert_eq!(diffed[0].status, OutputFileDiffStatus::Added);
    assert_eq!(diffed[0].path, path);
    assert!(!diffed[0].path.exists());
}

#[test]
fn diff_removable_output_files_reports_deletes_without_deleting() {
    let root = temp_test_dir("diff-remove-owned");
    let plan = test_render_plan("routes/demo.yml", "old\n");
    let path = root.join(".bindport/out/routes/demo.yml");
    let written = write_render_plan(&plan, &root, &[]).expect("write plan");

    let diffed = diff_removable_output_files(
        &[RemovableOutputFile {
            route_key: written[0].route_key.clone(),
            path: path.clone(),
            content_hash: written[0].content_hash.clone(),
        }],
        &root,
        &plan.output,
    )
    .expect("diff removable files");

    assert_eq!(diffed.len(), 1);
    assert_eq!(diffed[0].status, OutputFileRemovalStatus::Removed);
    assert_eq!(diffed[0].old_contents.as_deref(), Some("old\n"));
    assert_eq!(fs::read_to_string(&path).expect("file remains"), "old\n");
}

#[test]
fn write_render_plan_refuses_externally_modified_owned_file() {
    let root = temp_test_dir("write-plan-modified");
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "changed").expect("changed file");
    let plan = test_render_plan("routes/demo.yml", "new");

    let error = write_render_plan(
        &plan,
        &root,
        &[OutputFileOwnership {
            path: path.clone(),
            content_hash: content_hash("old"),
        }],
    )
    .expect_err("externally modified file");

    assert!(matches!(
        error,
        OutputFileError::ExternalModified { path: error_path } if error_path == path
    ));
}

#[test]
fn render_plan_paths_support_rootless_targets_and_report_escapes() {
    let root = temp_test_dir("plan-path-rootless");
    let mut plan = test_render_plan("routes/demo.yml", "body");
    plan.output.root = None;
    plan.output.target = String::from("routes/{{ route.slug }}.yml");

    let planned = render_plan_paths(&plan, &root).expect("rootless path");
    assert_eq!(planned[0].path, root.join("routes/demo.yml"));

    plan.files[0].target = String::from("other/demo.yml");
    let error = render_plan_paths(&plan, &root).expect_err("target escapes literal root");
    assert!(matches!(
        error,
        OutputFileError::TargetEscapesRoot { ref target, root: ref error_root }
            if target == "other/demo.yml" && error_root == &root.join("routes")
    ));
}

#[test]
fn render_plan_paths_rejects_absolute_root_outside_base() {
    let root = temp_test_dir("plan-path-absolute-root");
    let mut plan = test_render_plan("routes/demo.yml", "body");
    plan.output.root = Some(String::from("/tmp/bindport-outside-root"));

    let error = render_plan_paths(&plan, &root).expect_err("absolute root outside base");

    assert!(matches!(
        error,
        OutputFileError::UnsafeRoot { ref root } if root == "/tmp/bindport-outside-root"
    ));
}

#[test]
fn render_plan_paths_rejects_absolute_root_inside_base() {
    let root = temp_test_dir("plan-path-absolute-root-inside");
    let mut plan = test_render_plan("routes/demo.yml", "body");
    plan.output.root = Some(root.join(".bindport/out").display().to_string());

    let error = render_plan_paths(&plan, &root).expect_err("absolute root inside base");

    assert!(matches!(error, OutputFileError::UnsafeRoot { .. }));
}

#[cfg(unix)]
#[test]
fn render_plan_paths_rejects_directory_root_as_target() {
    let root = temp_test_dir("plan-path-root-target");
    let mut plan = test_render_plan("", "body");
    plan.output.root = Some(String::from("/"));

    let error = render_plan_paths(&plan, &root).expect_err("directory target");

    assert!(matches!(
        error,
        OutputFileError::UnsafeRoot { ref root } if root == "/"
    ));
}

#[test]
fn write_render_plan_rejects_targets_that_escape_root() {
    let root = temp_test_dir("write-plan-escape");
    let plan = test_render_plan("../demo.yml", "escape");

    let error = write_render_plan(&plan, &root, &[]).expect_err("unsafe target");

    assert!(matches!(error, OutputFileError::UnsafeTarget { .. }));
}

#[cfg(unix)]
#[test]
fn write_render_plan_creates_output_files_private() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_test_dir("write-plan-private-mode");
    let plan = test_render_plan("routes/private.env", "secret=true\n");

    let written = write_render_plan(&plan, &root, &[]).expect("write plan");

    let mode = fs::metadata(&written[0].path)
        .expect("output metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
