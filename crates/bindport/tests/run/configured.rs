// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn configured_service_command_expands_port_arguments() {
    let registry_path = temp_registry_path("configured-command-registry");
    let root = temp_test_dir("configured-command-root");
    let range_start = free_loopback_port();
    let range_end = range_start.saturating_add(10);
    fs::write(
        root.join(".bindport.toml"),
        format!(
            r#"project = "storybook-project"
default_range = "{range_start}-{range_end}"
skip_ports = []

[[services]]
name = "storybook"
command = ["sh", "-c", "printf '%s|%s|%s' \"$PORT\" \"$1\" \"$2\"", "sh"]
args = ["--port", "{{port}}"]
"#
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "storybook"])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let parts = stdout.split('|').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[1], "--port");
    assert_eq!(parts[0], parts[2]);
    let assigned_port = parts[0].parse::<u16>().expect("assigned port");
    assert!(
        (range_start..=range_end).contains(&assigned_port),
        "assigned port {assigned_port} outside configured range {range_start}-{range_end}"
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["service"], "storybook");
    assert!(
        status["services"][0]["command"]
            .as_str()
            .expect("command")
            .ends_with(&format!("--port {assigned_port}"))
    );
}
#[test]
fn explicit_child_command_overrides_configured_service_command() {
    let registry_path = temp_registry_path("configured-command-override-registry");
    let root = temp_test_dir("configured-command-override-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"override-project\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\ncommand = [\"sh\", \"-c\", \"exit 99\"]\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web", "--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, port.to_string().as_bytes());
}
#[test]
fn run_subcommand_service_argument_overrides_env_and_config() {
    let registry_path = temp_registry_path("identity-precedence");
    let root = temp_test_dir("identity-precedence-root");
    fs::write(
        root.join(".bindport.toml"),
        "project = \"config-project\"\nservice = \"config-service\"\ndefault_range = \"29120-29120\"\n",
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(BINDPORT_PROJECT_ENV, "env-project")
        .env(BINDPORT_SERVICE_ENV, "env-service")
        .args([
            "run",
            "cli-service",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PORT\"",
        ])
        .output()
        .expect("run bindport");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"29120");

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");

    assert_eq!(status["services"][0]["project"], "env-project");
    assert_eq!(status["services"][0]["service"], "cli-service");
}
#[test]
fn service_config_injects_env_templates_and_route_metadata() {
    let registry_path = temp_registry_path("service-env-registry");
    let root = temp_test_dir("service-env-root");
    let port = free_loopback_port();
    init_git_repo(&root, "feature/tree");
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"example-app\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"{{branch}}.{{project}}.localhost\"\nhealth_url = \"{{route_url}}/health\"\nenv.BINDPORT_ASSIGNED_PORT = \"{{port}}\"\nenv.BINDPORT_ROUTE = \"{{route_url}}\"\nenv.BINDPORT_HEALTH = \"{{health_url}}\"\nenv.BINDPORT_DIRECT_URL = \"{{url}}\"\nenv.HOSTNAME = \"0.0.0.0\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s|%s' \"$BINDPORT_ASSIGNED_PORT\" \"$BINDPORT_ROUTE\" \"$BINDPORT_HEALTH\" \"$BINDPORT_DIRECT_URL\" \"$HOSTNAME\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!(
            "{port}|http://feature-tree.example-app.localhost|http://feature-tree.example-app.localhost/health|http://127.0.0.1:{port}|0.0.0.0"
        )
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run bindport status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let service = &status["services"][0];

    assert_eq!(service["project"], "example-app");
    assert_eq!(service["service"], "web");
    assert_eq!(service["hostname"], "feature-tree.example-app.localhost");
    assert_eq!(
        service["route_url"],
        "http://feature-tree.example-app.localhost"
    );
    assert_eq!(
        service["health_url"],
        "http://feature-tree.example-app.localhost/health"
    );
    assert_eq!(service["port"], port);
}
#[test]
fn service_config_rejects_execution_sensitive_env_names() {
    let registry_path = temp_registry_path("service-env-deny-registry");
    let root = temp_test_dir("service-env-deny-root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"env-deny\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nenv.NODE_OPTIONS = \"--require ./evil.js\"\nenv.LD_AUDIT = \"./audit.so\"\nenv.GCONV_PATH = \"./gconv\"\nenv.SAFE_VALUE = \"allowed\"\n"
        ),
    )
    .expect("write service config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s' \"${NODE_OPTIONS-unset}\" \"${LD_AUDIT-unset}\" \"${GCONV_PATH-unset}\" \"$SAFE_VALUE\"",
        ])
        .output()
        .expect("run bindport");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"unset|unset|unset|allowed");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `NODE_OPTIONS`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `LD_AUDIT`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ignoring restricted service env `GCONV_PATH`"),
        "stderr did not warn about restricted env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn configured_service_path_sets_child_cwd_and_registry_cwd() {
    let registry_path = temp_registry_path("service-cwd-registry");
    let root = temp_test_dir("service-cwd-root");
    let service_root = root.join("apps").join("web");
    fs::create_dir_all(&service_root).expect("service root");
    let service_root = service_root.canonicalize().expect("canonical service root");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"service-cwd\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\ncommand = [\"sh\", \"-c\", \"pwd -P\"]\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web"])
        .output()
        .expect("run configured service");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout").trim(),
        service_root.display().to_string()
    );

    let status_output = bindport_with_registry(&registry_path)
        .args(["status", "--json"])
        .output()
        .expect("run status");
    let status = serde_json::from_slice::<Value>(&status_output.stdout).expect("status json");
    let expected_cwd = service_root.display().to_string();
    assert_eq!(status["services"][0]["cwd"], expected_cwd);
    assert_eq!(status["runs"][0]["cwd"], expected_cwd);
}

#[cfg(unix)]
#[test]
fn configured_service_missing_and_non_directory_paths_fail_before_spawn() {
    for (case, service_path, expected_error) in [
        ("missing", "apps/missing", "is unavailable"),
        ("file", "apps/file", "is not a directory"),
    ] {
        let registry_path = temp_registry_path(&format!("service-path-{case}-registry"));
        let root = temp_test_dir(&format!("service-path-{case}-root"));
        fs::create_dir_all(root.join("apps")).expect("apps root");
        if case == "file" {
            fs::write(root.join(service_path), "not a directory").expect("service path file");
        }
        let marker = root.join("spawned");
        let port = free_loopback_port();
        fs::write(
            root.join(".bindport.toml"),
            format!(
                "project = \"service-path-{case}\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"{service_path}\"\n"
            ),
        )
        .expect("write config");

        let output = bindport_with_registry(&registry_path)
            .current_dir(&root)
            .args([
                "run",
                "web",
                "--",
                "sh",
                "-c",
                "printf spawned > \"$1\"",
                "sh",
                marker.to_str().expect("marker path"),
            ])
            .output()
            .expect("run configured service");

        assert!(!output.status.success());
        assert!(!marker.exists(), "child spawned for {case} service path");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected_error) && stderr.contains(service_path),
            "unexpected {case} path error: {stderr}"
        );
    }
}

#[cfg(unix)]
#[test]
fn configured_service_symlink_outside_project_fails_before_spawn() {
    let registry_path = temp_registry_path("service-path-symlink-registry");
    let root = temp_test_dir("service-path-symlink-root")
        .canonicalize()
        .expect("canonical project root");
    let outside = temp_test_dir("service-path-symlink-outside")
        .canonicalize()
        .expect("canonical outside directory");
    fs::create_dir_all(root.join("apps")).expect("apps root");
    std::os::unix::fs::symlink(&outside, root.join("apps").join("web"))
        .expect("service path symlink");
    let marker = root.join("spawned-outside-project");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"service-path-symlink\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args([
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf spawned > \"$1\"",
            "sh",
            marker.to_str().expect("marker path"),
        ])
        .output()
        .expect("run configured service");

    assert!(!output.status.success());
    assert!(!marker.exists(), "child spawned outside project root");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("resolves outside project root"),
        "unexpected symlink path error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn configured_service_prefers_nearest_nested_local_bin() {
    let registry_path = temp_registry_path("local-bin-precedence-registry");
    let root = temp_test_dir("local-bin-precedence-root");
    let service_root = root.join("packages").join("group").join("web");
    let bin_dirs = [
        root.join("node_modules").join(".bin"),
        root.join("packages")
            .join("group")
            .join("node_modules")
            .join(".bin"),
        service_root.join("node_modules").join(".bin"),
    ];
    for (bin_dir, label) in bin_dirs.iter().zip(["root", "group", "service"]) {
        fs::create_dir_all(bin_dir).expect("local bin dir");
        write_executable(
            &bin_dir.join("context-tool"),
            &format!("#!/bin/sh\nprintf '{label}'\n"),
        );
    }
    fs::write(
        root.join("package.json"),
        r#"{"name":"local-bin-precedence","workspaces":["packages/*"]}"#,
    )
    .expect("write workspace package");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"local-bin-precedence\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"packages/group/web\"\ncommand = [\"context-tool\"]\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .args(["run", "web"])
        .output()
        .expect("run configured service");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"service");
}

#[cfg(unix)]
#[test]
fn configured_service_preserves_ambient_path_after_local_bins() {
    let registry_path = temp_registry_path("ambient-path-registry");
    let root = temp_test_dir("ambient-path-root");
    let service_root = root.join("apps").join("web");
    let local_bin = service_root.join("node_modules").join(".bin");
    let ambient_bin = root.join("ambient-bin");
    fs::create_dir_all(&local_bin).expect("local bin");
    fs::create_dir_all(&ambient_bin).expect("ambient bin");
    write_executable(
        &ambient_bin.join("ambient-tool"),
        "#!/bin/sh\nprintf 'ambient'\n",
    );
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"ambient-path\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"apps/web\"\ncommand = [\"ambient-tool\"]\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(
            "PATH",
            std::env::join_paths([&ambient_bin]).expect("ambient PATH"),
        )
        .args(["run", "web"])
        .output()
        .expect("run configured service");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ambient");
}

#[cfg(unix)]
#[test]
fn configured_service_local_bin_search_stops_at_nested_workspace_root() {
    let registry_path = temp_registry_path("local-bin-boundary-registry");
    let root = temp_test_dir("local-bin-boundary-root");
    let workspace = root.join("frontend");
    let service_root = workspace.join("apps").join("web");
    let project_bin = root.join("node_modules").join(".bin");
    let ambient_bin = root.join("ambient-bin");
    fs::create_dir_all(&service_root).expect("service root");
    fs::create_dir_all(&project_bin).expect("project bin");
    fs::create_dir_all(&ambient_bin).expect("ambient bin");
    write_executable(
        &project_bin.join("boundary-tool"),
        "#!/bin/sh\nprintf 'above-boundary'\n",
    );
    write_executable(
        &ambient_bin.join("boundary-tool"),
        "#!/bin/sh\nprintf 'ambient'\n",
    );
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"frontend","workspaces":["apps/*"]}"#,
    )
    .expect("write nested workspace package");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"local-bin-boundary\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\npath = \"frontend/apps/web\"\ncommand = [\"boundary-tool\"]\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&root)
        .env(
            "PATH",
            std::env::join_paths([&ambient_bin]).expect("ambient PATH"),
        )
        .args(["run", "web"])
        .output()
        .expect("run configured service");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ambient");
}

#[cfg(unix)]
#[test]
fn service_without_path_keeps_invoker_cwd_and_ambient_lookup() {
    let registry_path = temp_registry_path("no-service-path-registry");
    let root = temp_test_dir("no-service-path-root");
    let invoker_cwd = root.join("nested");
    let local_bin = invoker_cwd.join("node_modules").join(".bin");
    let ambient_bin = root.join("ambient-bin");
    fs::create_dir_all(&local_bin).expect("local bin");
    fs::create_dir_all(&ambient_bin).expect("ambient bin");
    write_executable(
        &local_bin.join("context-tool"),
        "#!/bin/sh\nprintf 'local|%s' \"$(/bin/pwd -P)\"\n",
    );
    write_executable(
        &ambient_bin.join("context-tool"),
        "#!/bin/sh\nprintf 'ambient|%s' \"$(/bin/pwd -P)\"\n",
    );
    let invoker_cwd = invoker_cwd.canonicalize().expect("canonical invoker cwd");
    let port = free_loopback_port();
    fs::write(
        root.join(".bindport.toml"),
        format!(
            "project = \"no-service-path\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[[services]]\nname = \"web\"\ncommand = [\"context-tool\"]\n"
        ),
    )
    .expect("write config");

    let output = bindport_with_registry(&registry_path)
        .current_dir(&invoker_cwd)
        .env(
            "PATH",
            std::env::join_paths([&ambient_bin]).expect("ambient PATH"),
        )
        .args(["run", "web"])
        .output()
        .expect("run configured service");

    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!("ambient|{}", invoker_cwd.display())
    );
}
