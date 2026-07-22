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
fn reserve_all_records_every_named_service_idempotently_with_route_metadata() {
    let registry_path = temp_registry_path("reserve-all");
    let root = temp_test_dir("reserve-all-root");
    let range_start = free_loopback_port().clamp(20_000, 65_520);
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"all-project\"\ndefault_range = \"{range_start}-{}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\nhostname = \"{{service}}.localhost\"\nroute_url = \"http://{{hostname}}:{{port}}\"\nhealth_url = \"{{route_url}}/health\"\n\n[[services]]\nname = \"api\"\nhostname = \"{{service}}.localhost\"\nroute_url = \"http://{{hostname}}:{{port}}\"\nhealth_url = \"{{route_url}}/health\"\n",
            range_start + 7
        ),
    )
    .expect("write project config");
    let root = fs::canonicalize(root).expect("canonical root");

    let first = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve", "--all"])
        .output()
        .expect("reserve all");
    assert!(
        first.status.success(),
        "reserve all failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve", "--all"])
        .output()
        .expect("reserve all again");
    assert!(second.status.success());
    assert_eq!(second.stdout, first.stdout);

    let status_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["status", "--json"])
        .output()
        .expect("status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let services = status["services"].as_array().expect("services");
    assert_eq!(services.len(), 2);
    assert!(status["runs"].as_array().expect("runs").is_empty());
    for service in services {
        assert_eq!(service["project"], "all-project");
        assert_eq!(service["state"], "reserved");
        let name = service["service"].as_str().expect("service name");
        assert_eq!(service["hostname"], format!("{name}.localhost"));
        assert_eq!(
            service["health_url"],
            format!(
                "http://{name}.localhost:{}/health",
                service["port"].as_u64().expect("port")
            )
        );
    }

    let export_output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["registry", "export"])
        .output()
        .expect("registry export");
    let export = serde_json::from_slice::<Value>(&export_output.stdout).expect("export json");
    assert_eq!(export["leases"].as_array().expect("leases").len(), 2);
}

#[test]
fn reserve_all_keeps_same_service_names_isolated_across_worktrees() {
    let registry_path = temp_registry_path("reserve-all-worktrees");
    let first_root = temp_test_dir("reserve-all-first-root");
    let second_root = temp_test_dir("reserve-all-second-root");
    let range_start = free_loopback_port().clamp(20_000, 65_520);
    let config = format!(
        "project = \"worktree-project\"\ndefault_range = \"{range_start}-{}\"\nskip_ports = []\n\n[[services]]\nname = \"web\"\n",
        range_start + 7
    );
    fs::write(first_root.join(".bindport.toml"), &config).expect("first config");
    fs::write(second_root.join(".bindport.toml"), config).expect("second config");
    let first_root = fs::canonicalize(first_root).expect("canonical first root");
    let second_root = fs::canonicalize(second_root).expect("canonical second root");

    for root in [&first_root, &second_root] {
        let output = bindport_with_registry(&registry_path)
            .current_dir(root)
            .args(["reserve", "--all"])
            .output()
            .expect("reserve all");
        assert!(
            output.status.success(),
            "reserve all failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let lookup = |root: &Path| {
        let output = bindport_with_registry(&registry_path)
            .current_dir(root)
            .args(["port", "web"])
            .output()
            .expect("port lookup");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("port stdout")
            .trim()
            .parse::<u16>()
            .expect("decimal port")
    };

    assert_ne!(lookup(&first_root), lookup(&second_root));
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
