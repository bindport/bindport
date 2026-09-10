// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn reserve_all_supports_out_of_declaration_order_sibling_startup() {
    let registry_path = temp_registry_path("sibling-reserve-all-registry");
    let root = temp_test_dir("sibling-reserve-all-root")
        .canonicalize()
        .expect("root");
    let (mut range_guards, range_start, range_end) = guarded_port_range();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "sibling-reserve-all"
default_range = "{range_start}-{range_end}"
skip_ports = []

[[services]]
name = "api"
command = ["sh", "-c", "printf '%s' \"$PORT\""]

[[services]]
name = "web"
command = ["sh", "-c", "printf '%s' \"$API_PORT\""]
env.API_PORT = "{{services.api.port}}"
"#
        ),
    )
    .expect("write config");

    range_guards.truncate(1);
    let reserve = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve", "--all"])
        .output()
        .expect("reserve all");
    assert!(
        reserve.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&reserve.stderr)
    );
    let status = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["status", "--json"])
        .output()
        .expect("status");
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    assert_eq!(services.len(), 2);
    assert!(
        services
            .iter()
            .all(|service| service["state"] == "reserved")
    );
    let reserved_ports = services
        .iter()
        .map(|service| service["port"].as_u64().expect("reserved port"))
        .collect::<BTreeSet<_>>();
    assert_eq!(reserved_ports.len(), 2);
    assert!(
        reserved_ports
            .iter()
            .all(|port| { (u64::from(range_start + 1)..=u64::from(range_end)).contains(port) })
    );
    let api_port_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["port", "api"])
        .output()
        .expect("api port");
    let api_port = String::from_utf8(api_port_output.stdout)
        .expect("port stdout")
        .trim()
        .parse::<u16>()
        .expect("api port");

    let web = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web"])
        .output()
        .expect("run web first");
    assert!(
        web.status.success(),
        "web failed: {}",
        String::from_utf8_lossy(&web.stderr)
    );
    assert_eq!(web.stdout, api_port.to_string().as_bytes());

    let api = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "api"])
        .output()
        .expect("run api second");
    assert!(
        api.status.success(),
        "api failed: {}",
        String::from_utf8_lossy(&api.stderr)
    );
    assert_eq!(api.stdout, api_port.to_string().as_bytes());
}
