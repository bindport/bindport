// SPDX-License-Identifier: MIT

mod support;

use support::*;

#[test]
fn reserve_records_reserved_service_and_reuses_identity_port() {
    let registry_path = temp_registry_path("reserve-service");
    let root = temp_test_dir("reserve-service-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"reserved-project\"\nservice = \"web\"\ndefault_range = \"29360-29361\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "reserve",
            "--route-url",
            "http://{service}.localhost:{port}",
        ])
        .output()
        .expect("reserve port");
    assert!(
        first.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_stdout = String::from_utf8(first.stdout).expect("first stdout");
    assert!(first_stdout.contains("reserved web\t127.0.0.1:"));
    assert!(first_stdout.contains("http://web.localhost:"));

    let second = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "reserve",
            "--route-url",
            "http://{service}.localhost:{port}",
        ])
        .output()
        .expect("reserve port again");
    assert!(
        second.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        String::from_utf8(second.stdout).expect("second stdout"),
        first_stdout
    );

    let status_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    assert!(status_output.status.success());
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status");
    let services = status["services"].as_array().expect("services");
    let runs = status["runs"].as_array().expect("runs");

    assert_eq!(services.len(), 1);
    assert!(runs.is_empty());
    assert_eq!(services[0]["state"], "reserved");
    assert_eq!(services[0]["project"], "reserved-project");
    assert_eq!(services[0]["service"], "web");
    assert_eq!(services[0]["pid"], Value::Null);
    assert_eq!(services[0]["command"], "reserved");
    assert!(
        services[0]["port"].as_u64().expect("port") >= 29_360
            && services[0]["port"].as_u64().expect("port") <= 29_361
    );
}

#[cfg(unix)]
#[test]
fn reserve_prunes_oldest_stale_leases_under_range_pressure() {
    let registry_path = temp_registry_path("reserve-pressure-cleanup");
    let root = temp_test_dir("reserve-pressure-cleanup-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"pressure-project\"\nservice = \"web\"\ndefault_range = \"29364-29367\"\nskip_ports = []\n",
    )
    .expect("write project config");

    for index in 0..3 {
        record_stale_registry_service(
            &registry_path,
            &format!("reserve-stale-{index}"),
            29_364 + index,
        );
    }

    let reserve = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve"])
        .output()
        .expect("reserve port");
    assert!(
        reserve.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&reserve.stderr)
    );
    assert!(
        String::from_utf8_lossy(&reserve.stderr)
            .contains("bindport: pruned 1 stale registry entries under configured range pressure")
    );

    let status_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status");
    let services = status["services"].as_array().expect("services");
    let stale_count = services
        .iter()
        .filter(|service| service["state"] == "stale")
        .count();

    assert_eq!(stale_count, 2);
    assert!(
        services
            .iter()
            .any(|service| { service["service"] == "web" && service["state"] == "reserved" })
    );
}

#[test]
fn release_frees_reserved_service_for_future_runs() {
    let registry_path = temp_registry_path("release-service");
    let root = temp_test_dir("release-service-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"release-project\"\nservice = \"web\"\ndefault_range = \"29362-29363\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let reserve = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve"])
        .output()
        .expect("reserve port");
    assert!(reserve.status.success());
    let reserved_port = String::from_utf8(reserve.stdout)
        .expect("reserve stdout")
        .lines()
        .next()
        .and_then(|line| line.rsplit_once(':'))
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .expect("reserved port");

    let release = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["release", "web"])
        .output()
        .expect("release service");
    assert!(
        release.status.success(),
        "release failed: {}",
        String::from_utf8_lossy(&release.stderr)
    );
    assert!(String::from_utf8_lossy(&release.stdout).contains("released web"));

    let status_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["status", "--json"])
        .output()
        .expect("status json");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status");
    assert_eq!(status["services"][0]["state"], "stopped");

    let run_port = run_print_port(&registry_path, &root);
    assert_eq!(run_port, reserved_port);

    let reserved_port_arg = reserved_port.to_string();
    let release_missing = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["release", reserved_port_arg.as_str()])
        .output()
        .expect("release missing");
    assert!(!release_missing.status.success());
    assert!(String::from_utf8_lossy(&release_missing.stderr).contains("no reserved lease matched"));
}
