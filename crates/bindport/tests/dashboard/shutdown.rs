// SPDX-License-Identifier: MIT

use super::service_support::*;
use crate::support::*;
use rusqlite::Connection;

fn run_state(fixture: &ServiceFixture, pid: u32) -> (String, bool) {
    Connection::open(&fixture.registry).expect("registry connection")
        .query_row(
            "SELECT leases.state, runs.exited_at IS NOT NULL FROM runs JOIN leases ON leases.id = runs.lease_id WHERE runs.pid = ?1",
            [pid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).expect("dashboard run")
}

fn assert_recorded_stop(fixture: &ServiceFixture, pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = run_state(fixture, pid);
        if state == ("stopped".into(), true) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "dashboard did not record stop: {state:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn background_dashboard_stop_finishes_its_registered_run() {
    let fixture = ServiceFixture::new("dashboard-background-shutdown");
    let started = fixture.finish(
        fixture
            .command("start")
            .arg("--register-service")
            .spawn()
            .expect("start"),
    );
    assert_success(&started);
    let (pid, port) = endpoint(&String::from_utf8_lossy(&started.stdout));
    assert_eq!(run_state(&fixture, pid), ("active".into(), false));
    assert_success(&fixture.run("stop"));
    assert_recorded_stop(&fixture, pid);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

fn foreground_signal_records_stop(signal: libc::c_int) {
    let fixture = ServiceFixture::new("dashboard-foreground-shutdown");
    let mut dashboard = start_foreground(
        fixture.command("serve"),
        &fixture.root.join("foreground.stdout"),
    );
    let pid = dashboard.child.id();
    assert_eq!(run_state(&fixture, pid), ("active".into(), false));
    // SAFETY: the child handle owns the live dashboard started by this test.
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, signal) }, 0);
    let status =
        wait_for_child(&mut dashboard.child, Duration::from_secs(5)).expect("dashboard exit");
    assert_eq!(run_state(&fixture, pid), ("stopped".into(), true));
    assert!(status.success(), "{status}");
}

#[test]
fn foreground_sigterm_records_stopped() {
    foreground_signal_records_stop(libc::SIGTERM);
}

#[test]
fn foreground_sigint_records_stopped() {
    foreground_signal_records_stop(libc::SIGINT);
}

#[test]
fn foreground_terminal_ctrl_c_records_stopped() {
    use std::os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    };

    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty initializes both output descriptors on success.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // SAFETY: each successful openpty descriptor is transferred exactly once.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // SAFETY: the termios storage and owned slave descriptor remain valid.
    unsafe {
        let mut settings = std::mem::zeroed::<libc::termios>();
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut settings), 0);
        settings.c_lflag |= libc::ISIG;
        settings.c_cc[libc::VINTR] = 3;
        assert_eq!(
            libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &settings),
            0
        );
    }
    let fixture = ServiceFixture::new("dashboard-terminal-shutdown");
    let mut command = fixture.command("serve");
    command.stdin(Stdio::from(slave));
    // SAFETY: the post-fork closure uses only session/terminal syscalls and
    // reports errors before exec. The slave is already installed as stdin.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut dashboard = start_foreground(command, &fixture.root.join("terminal.stdout"));
    let pid = dashboard.child.id();
    let mut terminal = fs::File::from(master);
    terminal.write_all(&[3]).expect("terminal Ctrl-C");
    let status = wait_for_child(&mut dashboard.child, Duration::from_secs(5))
        .expect("terminal dashboard exit");
    assert_eq!(run_state(&fixture, pid), ("stopped".into(), true));
    assert!(status.success(), "{status}");
}

#[test]
fn killed_dashboard_remains_stale_not_orderly_stopped() {
    let fixture = ServiceFixture::new("dashboard-abrupt-shutdown");
    let mut dashboard = start_foreground(
        fixture.command("serve"),
        &fixture.root.join("foreground.stdout"),
    );
    let pid = dashboard.child.id();
    dashboard.child.kill().expect("kill test dashboard");
    dashboard.child.wait().expect("reap test dashboard");
    assert_eq!(run_state(&fixture, pid), ("active".into(), false));
    let output = bindport_with_registry(&fixture.registry)
        .current_dir(&fixture.root)
        .env("XDG_STATE_HOME", &fixture.state_home)
        .args(["status", "--json"])
        .output()
        .expect("status");
    assert_success(&output);
    let status: Value = serde_json::from_slice(&output.stdout).expect("status JSON");
    assert_eq!(status["services"][0]["state"], "stale");
}
