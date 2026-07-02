// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn open_prints_best_url_for_active_service() {
    let registry_path = temp_registry_path("open-service-url-registry");
    let root = temp_test_dir("open-service-url-root");
    let marker_path = temp_path("open-service-url-marker");
    let marker_arg = marker_path.display().to_string();
    fs::write(
        root.join(".bindport.toml"),
        "project = \"open-project\"\ndefault_range = \"29480-29481\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"web.localhost\"\nroute_url = \"https://{hostname}\"\n",
    )
    .expect("write open config");

    let mut child = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf ready > \"$1\"; sleep 2",
            "sh",
            &marker_arg,
        ])
        .spawn()
        .expect("spawn bindport service");

    wait_for_file_contains(&marker_path, "ready", Duration::from_secs(5));
    let stdout = wait_for_open_url(
        &registry_path,
        &["open", "web", "--print"],
        Duration::from_secs(5),
    );

    assert_eq!(stdout.trim(), "https://web.localhost");

    let status = wait_for_child(&mut child, Duration::from_secs(3)).expect("service exits");
    assert!(status.success());
}
