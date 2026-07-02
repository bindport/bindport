// SPDX-License-Identifier: MIT

use super::*;

pub fn wait_for_file_contains(path: &Path, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && contents.contains(needle)
        {
            return contents;
        }

        if Instant::now() >= deadline {
            panic!(
                "{} did not contain `{needle}` within {timeout:?}",
                path.display()
            );
        }

        thread::sleep(Duration::from_millis(25));
    }
}

pub fn wait_for_open_url(registry_path: &Path, args: &[&str], timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;

    loop {
        let output = bindport_with_registry(registry_path)
            .args(args)
            .output()
            .expect("run bindport open");

        if output.status.success() {
            return String::from_utf8(output.stdout).expect("open stdout");
        }

        if Instant::now() >= deadline {
            panic!(
                "bindport open did not succeed within {timeout:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        thread::sleep(Duration::from_millis(25));
    }
}
