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
        &root,
        &["open", "web", "--print"],
        Duration::from_secs(5),
    );

    assert_eq!(stdout.trim(), "https://web.localhost");

    let status = wait_for_child(&mut child, Duration::from_secs(3)).expect("service exits");
    assert!(status.success());
}

#[test]
fn open_and_hostname_print_reserved_route_metadata() {
    let registry_path = temp_registry_path("open-reserved-url-registry");
    let root = temp_test_dir("open-reserved-url-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"open-reserved\"\ndefault_range = \"29482-29483\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"reserved.web.localhost\"\nroute_url = \"https://{hostname}\"\n",
    )
    .expect("write reserved open config");

    let reserve = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve", "web"])
        .output()
        .expect("reserve web");
    assert!(
        reserve.status.success(),
        "reserve failed: {}",
        String::from_utf8_lossy(&reserve.stderr)
    );

    for args in [
        &["open", "web", "--print"][..],
        &["open", "web", "--print", "--registry-wide"][..],
    ] {
        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args(args)
            .output()
            .expect("open reserved web");
        assert!(
            output.status.success(),
            "open failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"https://reserved.web.localhost\n");
    }

    let hostname = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hostname", "web"])
        .output()
        .expect("lookup hostname");
    assert!(hostname.status.success());
    assert_eq!(hostname.stdout, b"reserved.web.localhost\n");
    assert!(hostname.stderr.is_empty());
}

#[test]
fn open_and_hostname_select_the_current_git_worktree() {
    let registry_path = temp_registry_path("open-worktree-url-registry");
    let root = temp_test_dir("open-worktree-url-root");
    init_git_repo(&root, "main");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"open-worktrees\"\ndefault_range = \"29484-29491\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"{branch_label}.web.localhost\"\nroute_url = \"https://{hostname}\"\n",
    )
    .expect("write worktree open config");
    run_git(&root, ["add", ".bindport.toml"]);
    run_git(&root, ["commit", "-m", "add bindport config"]);

    let second_root = temp_path("open-worktree-url-second");
    let second_arg = second_root.display().to_string();
    run_git(
        &root,
        ["worktree", "add", &second_arg, "-b", "feature/beta"],
    );

    for worktree in [&root, &second_root] {
        let reserve = bindport_with_registry(&registry_path)
            .current_dir(worktree)
            .args(["reserve", "web"])
            .output()
            .expect("reserve worktree service");
        assert!(
            reserve.status.success(),
            "reserve failed in {}: {}",
            worktree.display(),
            String::from_utf8_lossy(&reserve.stderr)
        );
    }

    for (worktree, expected_hostname) in [
        (&root, "main.web.localhost"),
        (&second_root, "feature-beta.web.localhost"),
    ] {
        let open = bindport_with_registry(&registry_path)
            .current_dir(worktree)
            .args(["open", "web", "--print"])
            .output()
            .expect("open scoped service");
        assert!(
            open.status.success(),
            "open failed in {}: {}",
            worktree.display(),
            String::from_utf8_lossy(&open.stderr)
        );
        assert_eq!(
            String::from_utf8(open.stdout).expect("open stdout"),
            format!("https://{expected_hostname}\n")
        );

        let hostname = bindport_with_registry(&registry_path)
            .current_dir(worktree)
            .args(["hostname", "web"])
            .output()
            .expect("lookup scoped hostname");
        assert!(hostname.status.success());
        assert_eq!(
            String::from_utf8(hostname.stdout).expect("hostname stdout"),
            format!("{expected_hostname}\n")
        );
    }

    let registry_wide = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "open",
            "web",
            "--print",
            "--registry-wide",
            "--project",
            "open-worktrees",
        ])
        .output()
        .expect("open registry-wide service");
    assert!(!registry_wide.status.success());
    let stderr = String::from_utf8_lossy(&registry_wide.stderr);
    assert!(stderr.contains("omit --registry-wide to select the current worktree"));
    assert!(!stderr.contains("pass --project"));
    assert!(
        stderr.contains(
            &fs::canonicalize(&root)
                .expect("canonical root")
                .display()
                .to_string()
        )
    );
    assert!(
        stderr.contains(
            &fs::canonicalize(&second_root)
                .expect("canonical second root")
                .display()
                .to_string()
        )
    );
}

#[test]
fn hostname_fails_when_the_scoped_service_has_no_hostname_metadata() {
    let registry_path = temp_registry_path("hostname-missing-registry");
    let root = temp_test_dir("hostname-missing-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"hostname-missing\"\ndefault_range = \"29492-29493\"\nskip_ports = []\n[[services]]\nname = \"web\"\n",
    )
    .expect("write hostname config");

    let reserve = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["reserve", "web"])
        .output()
        .expect("reserve web");
    assert!(reserve.status.success());

    let hostname = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["hostname", "web"])
        .output()
        .expect("lookup missing hostname");
    assert!(!hostname.status.success());
    assert!(hostname.stdout.is_empty());
    assert!(String::from_utf8_lossy(&hostname.stderr).contains("has no hostname metadata"));
}
