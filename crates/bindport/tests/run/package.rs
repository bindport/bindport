// SPDX-License-Identifier: MIT

use crate::support::*;

#[cfg(unix)]
#[test]
fn package_script_runs_bindport_next_dev_flow() {
    let registry_path = temp_registry_path("package-script-registry");
    let root = temp_test_dir("package-script-root");
    let bindport_bin_dir = root.join(".test-bin");
    let next_bin_dir = root.join("node_modules").join(".bin");

    fs::create_dir_all(&bindport_bin_dir).expect("bindport bin dir");
    fs::create_dir_all(&next_bin_dir).expect("next bin dir");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_bindport"),
        bindport_bin_dir.join("bindport"),
    )
    .expect("link bindport binary");
    write_executable(
        &next_bin_dir.join("next"),
        "#!/bin/sh\nif [ \"$1\" != \"dev\" ]; then echo \"unexpected next args: $*\" >&2; exit 64; fi\nprintf 'next-dev-port=%s\\n' \"$PORT\"\n",
    );
    fs::write(
        root.join("package.json"),
        r#"{"name":"bindport-package-script-fixture","private":true,"scripts":{"dev":"bindport -- next dev"}}"#,
    )
    .expect("write package json");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"package-script-fixture\"\nservice = \"web\"\ndefault_range = \"29420-29421\"\nskip_ports = []\n",
    )
    .expect("write config");

    let output = Command::new("npm")
        .current_dir(&root)
        .env(REGISTRY_PATH_ENV, &registry_path)
        .env_remove(BINDPORT_PROJECT_ENV)
        .env_remove(BINDPORT_SERVICE_ENV)
        .env("PATH", prepend_path(&bindport_bin_dir))
        .env("NO_UPDATE_NOTIFIER", "1")
        .env("NPM_CONFIG_AUDIT", "false")
        .env("NPM_CONFIG_FUND", "false")
        .args(["run", "--silent", "dev"])
        .output()
        .expect("run package script");

    assert!(
        output.status.success(),
        "package script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let port = stdout
        .trim()
        .strip_prefix("next-dev-port=")
        .expect("next dev port marker")
        .parse::<u16>()
        .expect("port");

    assert!(matches!(port, 29_420 | 29_421));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "package-script-fixture");
    assert_eq!(status["services"][0]["service"], "web");
    assert_eq!(status["services"][0]["command"], "next dev");
    assert_eq!(status["services"][0]["hostname"], Value::Null);
    assert_eq!(status["services"][0]["route_url"], Value::Null);
    assert_eq!(status["services"][0]["proxy"], Value::Null);
    assert_eq!(status["services"][0]["exit_code"], 0);
    assert_eq!(
        status["services"][0]["port"]
            .as_u64()
            .expect("service port"),
        u64::from(port)
    );
    assert_eq!(status["runs"][0]["exit_code"], 0);
}
