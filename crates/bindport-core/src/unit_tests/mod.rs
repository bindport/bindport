// SPDX-License-Identifier: MIT

use super::*;
use std::{
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

mod config;
mod identity;
mod outputs;
mod ports;
mod services;
mod validation;

fn temp_test_dir(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("bindport-core-{name}-{}-{now}", std::process::id()));

    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
