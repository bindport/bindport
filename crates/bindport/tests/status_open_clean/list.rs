// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn list_json_groups_registry_services_by_project() {
    let registry_path = temp_registry_path("list-json-registry");
    let first_root = temp_test_dir("list-json-first");
    let second_root = temp_test_dir("list-json-second");
    let first_port = 29_870;
    let second_port = 29_871;

    fs::write(
        first_root.join(".bindport.toml"),
        format!(
            "project = \"alpha\"\ndefault_range = \"{first_port}-{first_port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"web.alpha.localhost\"\nroute_url = \"https://{{hostname}}\"\n"
        ),
    )
    .expect("write first config");
    fs::write(
        second_root.join(".bindport.toml"),
        format!(
            "project = \"beta\"\ndefault_range = \"{second_port}-{second_port}\"\nskip_ports = []\n[[services]]\nname = \"api\"\n"
        ),
    )
    .expect("write second config");

    for root in [&first_root, &second_root] {
        let output = bindport_with_registry(&registry_path)
            .current_dir(root)
            .args(["--", "sh", "-c", "true"])
            .output()
            .expect("run bindport");
        assert!(
            output.status.success(),
            "bindport failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let list_output = bindport_with_registry(&registry_path)
        .args(["list", "--json"])
        .output()
        .expect("list json");
    assert!(
        list_output.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list = serde_json::from_slice::<Value>(&list_output.stdout).expect("list json");

    assert_eq!(list["schema_version"], "0.1");
    assert_eq!(list["project_count"], 2);
    assert_eq!(list["service_count"], 2);
    assert_eq!(list["projects"][0]["project"], "alpha");
    assert_eq!(list["projects"][0]["stopped"], 1);
    assert_eq!(list["projects"][0]["services"][0]["service"], "web");
    assert_eq!(list["projects"][0]["services"][0]["port"], first_port);
    assert_eq!(
        list["projects"][0]["services"][0]["route_url"],
        "https://web.alpha.localhost"
    );
    assert_eq!(list["projects"][1]["project"], "beta");
    assert_eq!(list["projects"][1]["services"][0]["service"], "api");

    let plain_output = bindport_with_registry(&registry_path)
        .args(["list"])
        .output()
        .expect("list text");
    assert!(plain_output.status.success());
    let stdout = String::from_utf8(plain_output.stdout).expect("list text");
    assert!(stdout.contains("alpha (1 services: 0 active, 1 stopped"));
    assert!(stdout.contains("stopped\tweb\t127.0.0.1:"));
    assert!(stdout.contains("https://web.alpha.localhost"));
    assert!(stdout.contains("beta (1 services: 0 active, 1 stopped"));
    assert!(stdout.contains("stopped\tapi\t127.0.0.1:"));
}
