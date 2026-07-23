// SPDX-License-Identifier: MIT

use std::env;

use crate::support::*;

#[cfg(unix)]
#[test]
fn forwards_sigterm_to_wrapped_child_and_records_exit() {
    let registry_path = temp_registry_path("signal-registry");
    let child_pid_path = temp_registry_path("signal-child-pid");
    let marker_path = temp_registry_path("signal-marker");
    let child_pid_path_arg = child_pid_path.display().to_string();
    let marker_path_arg = marker_path.display().to_string();

    let mut bindport = bindport_with_registry(&registry_path)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s\n' $$ > \"$1\"; trap 'printf forwarded > \"$2\"; exit 42' TERM INT; printf 'ready\n'; while :; do sleep 1; done",
            "sh",
            &child_pid_path_arg,
            &marker_path_arg,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run bindport");

    let stdout = bindport.stdout.take().expect("stdout pipe");
    let mut stdout = BufReader::new(stdout);
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read readiness line");
    assert_eq!(ready, "ready\n");

    let child_pid = fs::read_to_string(&child_pid_path)
        .expect("child pid file")
        .trim()
        .parse::<u32>()
        .expect("child pid");
    let active_deadline = Instant::now() + Duration::from_secs(5);
    let active = loop {
        let active_output = bindport_with_registry(&registry_path)
            .args(["status", "--json"])
            .output()
            .expect("active status");
        assert!(active_output.status.success());
        let active =
            serde_json::from_slice::<Value>(&active_output.stdout).expect("active status json");
        if active["services"][0]["pid"] == child_pid {
            break active;
        }
        if let Some(status) = bindport.try_wait().expect("poll bindport") {
            panic!("bindport exited before adopting child pid {child_pid}: {status}; {active}");
        }
        if Instant::now() >= active_deadline {
            send_signal(child_pid, libc::SIGKILL);
            let _ = bindport.kill();
            let _ = bindport.wait();
            panic!("bindport did not adopt child pid {child_pid}: {active}");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let expected_cwd = env::current_dir()
        .expect("current directory")
        .canonicalize()
        .expect("canonical current directory");
    assert_eq!(active["services"][0]["state"], "active");
    assert_eq!(active["services"][0]["pid"], child_pid);
    assert!(
        active["services"][0]["command"]
            .as_str()
            .expect("active command")
            .contains("while :")
    );
    assert_eq!(
        active["services"][0]["cwd"],
        expected_cwd.display().to_string()
    );
    assert_eq!(active["runs"][0]["pid"], child_pid);

    send_signal(bindport.id(), libc::SIGTERM);

    let status = match wait_for_child(&mut bindport, Duration::from_secs(5)) {
        Some(status) => status,
        None => {
            send_signal(child_pid, libc::SIGKILL);
            let _ = bindport.kill();
            panic!("bindport did not exit after SIGTERM");
        }
    };

    assert_eq!(status.code(), Some(42));
    assert_eq!(
        fs::read_to_string(&marker_path).expect("marker"),
        "forwarded"
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");

    assert!(status_output.status.success());

    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    assert_eq!(status["services"][0]["state"], "stopped");
    assert_eq!(status["services"][0]["exit_code"], 42);
    assert_eq!(status["runs"][0]["exit_code"], 42);
}
