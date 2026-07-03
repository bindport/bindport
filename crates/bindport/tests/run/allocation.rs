// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn runner_reuses_previous_identity_port_when_available() {
    let registry_path = temp_registry_path("sticky-registry");
    let root = temp_test_dir("sticky-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29300-29301\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    let second_port = run_print_port(&registry_path, &root);

    assert_eq!(second_port, first_port);
}
#[test]
fn runner_falls_back_when_previous_identity_port_is_active() {
    let registry_path = temp_registry_path("sticky-occupied-registry");
    let root = temp_test_dir("sticky-occupied-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"sticky-project\"\nservice = \"web\"\ndefault_range = \"29310-29311\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let first_port = run_print_port(&registry_path, &root);
    reserve_registry_port(&registry_path, first_port);
    let second_port = run_print_port(&registry_path, &root);

    assert_ne!(second_port, first_port);
    assert!(matches!(second_port, 29_310 | 29_311));
}

#[cfg(unix)]
#[test]
fn runner_prunes_oldest_stale_leases_under_range_pressure() {
    let registry_path = temp_registry_path("runner-pressure-cleanup-registry");
    let root = temp_test_dir("runner-pressure-cleanup-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"pressure-project\"\nservice = \"web\"\ndefault_range = \"29320-29323\"\nskip_ports = []\n",
    )
    .expect("write project config");

    for index in 0..3 {
        record_stale_registry_service(&registry_path, &format!("stale-{index}"), 29_320 + index);
    }

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
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
            .any(|service| { service["service"] == "web" && service["state"] == "stopped" })
    );
}

#[cfg(unix)]
#[test]
fn runner_retries_once_when_assigned_port_is_claimed_after_spawn() {
    let registry_path = temp_registry_path("allocation-retry-registry");
    let root = temp_test_dir("allocation-retry-root");
    let marker_path = temp_path("allocation-retry-marker");
    let pid_path = temp_path("allocation-retry-pid");
    let marker_arg = marker_path.display().to_string();
    let pid_arg = pid_path.display().to_string();

    fs::write(
        root.join(".bindport.toml"),
        "project = \"retry-project\"\nservice = \"web\"\ndefault_range = \"29400-29401\"\nskip_ports = []\n",
    )
    .expect("write project config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            concat!(
                "if [ ! -f \"$1\" ]; then ",
                "python3 -c 'import os,socket,sys,time; from pathlib import Path; ",
                "s=socket.socket(); ",
                "s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); ",
                "s.bind((\"127.0.0.1\", int(sys.argv[1]))); ",
                "s.listen(); ",
                "Path(sys.argv[2]).write_text(str(os.getpid())); ",
                "Path(sys.argv[3]).write_text(sys.argv[1]); ",
                "time.sleep(5)' \"$PORT\" \"$2\" \"$1\" & ",
                "i=0; ",
                "while [ ! -f \"$1\" ] && [ \"$i\" -lt 100 ]; do ",
                "i=$((i + 1)); sleep 0.02; ",
                "done; ",
                "[ -f \"$1\" ] || exit 99; ",
                "exit 98; ",
                "fi; ",
                "printf '%s' \"$PORT\"",
            ),
            "sh",
            &marker_arg,
            &pid_arg,
        ])
        .output()
        .expect("run bindport");

    terminate_process_from_file(&pid_path);

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let first_port = fs::read_to_string(&marker_path)
        .expect("first port marker")
        .parse::<u16>()
        .expect("first port");
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let second_port = stdout.parse::<u16>().expect("second port");
    let stderr = String::from_utf8(output.stderr).expect("stderr");

    assert_ne!(second_port, first_port);
    assert!(matches!(first_port, 29_400 | 29_401));
    assert!(matches!(second_port, 29_400 | 29_401));
    assert!(stderr.contains("retrying with another port"));

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let runs = status["runs"].as_array().expect("runs");
    let mut exit_codes = runs
        .iter()
        .map(|run| run["exit_code"].as_i64().expect("exit code"))
        .collect::<Vec<_>>();
    exit_codes.sort_unstable();

    assert_eq!(runs.len(), 2);
    assert_eq!(exit_codes, [0, 98]);
    assert_eq!(status["services"][0]["exit_code"], 0);
    assert_eq!(
        status["services"][0]["port"]
            .as_u64()
            .expect("service port"),
        u64::from(second_port)
    );
}
