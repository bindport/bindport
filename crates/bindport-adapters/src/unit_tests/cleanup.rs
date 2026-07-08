// SPDX-License-Identifier: MIT

use super::*;
use crate::hash::legacy_content_hash;

#[test]
fn content_hash_uses_sha256_and_accepts_legacy_hashes() {
    let contents = "rendered output";
    let hash = content_hash(contents);

    assert_eq!(hash.len(), 64);
    assert!(content_hash_matches(contents, &hash));
    assert!(content_hash_matches(
        contents,
        &legacy_content_hash(contents)
    ));
}

#[test]
fn remove_owned_output_files_deletes_matching_files() {
    let root = temp_test_dir("remove-owned");
    let output = test_render_plan("routes/demo.yml", "owned").output;
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "owned").expect("owned file");

    let removed = remove_owned_output_files(
        &[RemovableOutputFile {
            route_key: String::from("route-1"),
            path: path.clone(),
            content_hash: content_hash("owned"),
        }],
        &root,
        &output,
    )
    .expect("remove owned file");

    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].route_key, "route-1");
    assert_eq!(removed[0].path, path);
    assert_eq!(removed[0].status, OutputFileRemovalStatus::Removed);
    assert!(!removed[0].path.exists());
}

#[test]
fn remove_owned_output_files_reports_missing_files() {
    let root = temp_test_dir("remove-missing");
    let output = test_render_plan("routes/missing.yml", "owned").output;
    let path = root.join(".bindport/out/routes/missing.yml");

    let removed = remove_owned_output_files(
        &[RemovableOutputFile {
            route_key: String::from("route-1"),
            path: path.clone(),
            content_hash: content_hash("owned"),
        }],
        &root,
        &output,
    )
    .expect("remove missing file");

    assert_eq!(removed[0].status, OutputFileRemovalStatus::Missing);
    assert_eq!(removed[0].path, path);
}

#[test]
fn remove_owned_output_files_reports_targets_outside_root() {
    let root = temp_test_dir("remove-outside-root");
    let output = test_render_plan("routes/demo.yml", "owned").output;
    let outside = root.join("outside/demo.yml");

    let removed = remove_owned_output_files(
        &[RemovableOutputFile {
            route_key: String::from("route-1"),
            path: outside.clone(),
            content_hash: content_hash("owned"),
        }],
        &root,
        &output,
    )
    .expect("outside root is reported");

    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].route_key, "route-1");
    assert_eq!(removed[0].path, outside);
    assert_eq!(removed[0].status, OutputFileRemovalStatus::OutsideRoot);
}

#[cfg(unix)]
#[test]
fn remove_owned_output_files_rejects_directory_target() {
    let root = temp_test_dir("remove-directory-target");
    let mut output = test_render_plan("routes/demo.yml", "owned").output;
    output.root = Some(String::from("/"));

    let error = remove_owned_output_files(
        &[RemovableOutputFile {
            route_key: String::from("route-1"),
            path: PathBuf::from("/"),
            content_hash: content_hash("owned"),
        }],
        &root,
        &output,
    )
    .expect_err("directory target");

    assert!(matches!(error, OutputFileError::UnsafeRoot { .. }));
}

#[test]
fn remove_owned_output_files_preserves_externally_modified_files() {
    let root = temp_test_dir("remove-modified");
    let output = test_render_plan("routes/demo.yml", "owned").output;
    let path = root.join(".bindport/out/routes/demo.yml");
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(&path, "changed").expect("changed file");

    let removed = remove_owned_output_files(
        &[RemovableOutputFile {
            route_key: String::from("route-1"),
            path: path.clone(),
            content_hash: content_hash("owned"),
        }],
        &root,
        &output,
    )
    .expect("check modified file");

    assert_eq!(removed[0].status, OutputFileRemovalStatus::ExternalModified);
    assert_eq!(
        fs::read_to_string(&path).expect("preserved file"),
        "changed"
    );
}

#[cfg(unix)]
#[test]
fn remove_owned_output_files_rejects_symlink_below_output_root() {
    let root = temp_test_dir("remove-symlink-root");
    let outside = temp_test_dir("remove-symlink-outside");
    let output = test_render_plan("routes/demo.yml", "owned").output;
    let routes_dir = root.join(".bindport/out/routes");
    let path = routes_dir.join("demo.yml");
    let outside_path = outside.join("demo.yml");
    fs::create_dir_all(&routes_dir).expect("routes dir");
    fs::write(&path, "owned").expect("owned file");
    fs::remove_file(&path).expect("remove original file");
    fs::remove_dir(&routes_dir).expect("remove original dir");
    fs::write(&outside_path, "owned").expect("outside file");
    std::os::unix::fs::symlink(&outside, &routes_dir).expect("symlink routes dir");

    let error = remove_owned_output_files(
        &[RemovableOutputFile {
            route_key: String::from("route-1"),
            path: path.clone(),
            content_hash: content_hash("owned"),
        }],
        &root,
        &output,
    )
    .expect_err("symlink below root");

    assert!(matches!(
        error,
        OutputFileError::SymlinkInPath { path: error_path } if error_path == routes_dir
    ));
    assert!(outside_path.is_file());
}

#[test]
fn output_file_errors_have_readable_display_and_io_sources() {
    let root = PathBuf::from("/tmp/bindport-test-root");
    let errors = [
        OutputFileError::UnsafeRoot {
            root: String::from("../root"),
        },
        OutputFileError::UnsafeTarget {
            target: String::from("../target"),
        },
        OutputFileError::TargetEscapesRoot {
            target: String::from("other/demo.yml"),
            root: root.clone(),
        },
        OutputFileError::SymlinkInPath {
            path: root.join("link"),
        },
        OutputFileError::UnownedTarget {
            path: root.join("route.yml"),
        },
        OutputFileError::ExternalModified {
            path: root.join("route.yml"),
        },
    ];

    for error in errors {
        assert!(!error.to_string().is_empty());
        assert!(std::error::Error::source(&error).is_none());
    }

    let io_error = OutputFileError::Io {
        path: root.join("route.yml"),
        source: io::Error::other("disk full"),
    };
    assert!(io_error.to_string().contains("disk full"));
    assert!(std::error::Error::source(&io_error).is_some());
}
