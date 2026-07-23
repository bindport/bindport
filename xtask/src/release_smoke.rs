// SPDX-License-Identifier: MIT

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const PACKAGE_TIMEOUT: Duration = Duration::from_secs(180);
const BACKGROUND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct ReleaseTarget {
    matrix_name: &'static str,
    raw_asset: &'static str,
    npm_platform: &'static str,
    npm_dir: &'static str,
    npm_name: &'static str,
    os: &'static str,
    cpu: &'static str,
}

const TARGETS: [ReleaseTarget; 4] = [
    ReleaseTarget {
        matrix_name: "linux-x64",
        raw_asset: "bindport-linux-x64",
        npm_platform: "linux-x64",
        npm_dir: "bindport-linux-x64",
        npm_name: "@bindport/linux-x64",
        os: "linux",
        cpu: "x64",
    },
    ReleaseTarget {
        matrix_name: "linux-arm64",
        raw_asset: "bindport-linux-arm64",
        npm_platform: "linux-arm64",
        npm_dir: "bindport-linux-arm64",
        npm_name: "@bindport/linux-arm64",
        os: "linux",
        cpu: "arm64",
    },
    ReleaseTarget {
        matrix_name: "macos-x64",
        raw_asset: "bindport-macos-x64",
        npm_platform: "darwin-x64",
        npm_dir: "bindport-darwin-x64",
        npm_name: "@bindport/darwin-x64",
        os: "darwin",
        cpu: "x64",
    },
    ReleaseTarget {
        matrix_name: "macos-arm64",
        raw_asset: "bindport-macos-arm64",
        npm_platform: "darwin-arm64",
        npm_dir: "bindport-darwin-arm64",
        npm_name: "@bindport/darwin-arm64",
        os: "darwin",
        cpu: "arm64",
    },
];

pub(crate) fn release_smoke(args: &[String]) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| String::from("xtask manifest has no repository parent"))?
        .canonicalize()
        .map_err(|error| format!("canonicalize repository root: {error}"))?;
    let Some(binary) = parse_binary_arg(args, &root)? else {
        println!("Usage: xtask release-smoke [--binary <path>]");
        return Ok(());
    };
    if !binary.is_file() {
        return Err(format!(
            "release smoke binary not found: {} (run `cargo build --release --locked` first)",
            binary.display()
        ));
    }
    let binary = binary
        .canonicalize()
        .map_err(|error| format!("canonicalize release binary: {error}"))?;
    let mut temp = SmokeTemp::new()?;
    create_isolated_homes(temp.path())?;

    println!("release smoke: validating source and channel metadata");
    let version = workspace_version(&root, &temp)?;
    validate_cargo_source_shape(&root, &temp)?;
    validate_release_mapping(&root, &version)?;
    run_existing_package_checks(&root, &temp, &version)?;

    println!("release smoke: staging local release artifacts for v{version}");
    let staged = stage_artifacts(&root, &temp, &binary, &version)?;

    println!("release smoke: installing staged host npm tarballs offline");
    let wrapper = install_staged_npm(&temp, &staged, &version)?;

    println!("release smoke: exercising the complete local v0.8 flow");
    exercise_user_flow(&temp, &wrapper, &version)?;
    temp.cleanup()?;

    println!(
        "release smoke passed on {}-{}; foreign targets were metadata-mapped only",
        env::consts::OS,
        env::consts::ARCH
    );
    println!("live registries, GitHub Releases, and the external Homebrew tap were not contacted");
    Ok(())
}

fn parse_binary_arg(args: &[String], root: &Path) -> Result<Option<PathBuf>, String> {
    let mut binary = root.join("target/release/bindport");
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--binary" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| String::from("release-smoke: --binary requires a path"))?;
                binary = root.join(value);
                index += 2;
            }
            "-h" | "--help" => return Ok(None),
            unknown => {
                return Err(format!("release-smoke: unknown argument `{unknown}`"));
            }
        }
    }
    Ok(Some(binary))
}

fn workspace_version(root: &Path, temp: &SmokeTemp) -> Result<String, String> {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"]);
    isolated_env(&mut command, temp);
    let output = checked_output(command, "cargo metadata", PACKAGE_TIMEOUT)?;
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parse cargo metadata: {error}"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| String::from("cargo metadata has no packages array"))?;
    let package = packages
        .iter()
        .find(|package| package["name"] == "bindport")
        .ok_or_else(|| String::from("cargo metadata is missing the bindport package"))?;
    package["version"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| String::from("cargo metadata bindport package has no version"))
}

fn validate_cargo_source_shape(root: &Path, temp: &SmokeTemp) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.current_dir(root).args([
        "package",
        "-p",
        "bindport",
        "--locked",
        "--allow-dirty",
        "--list",
    ]);
    isolated_env(&mut command, temp);
    let output = checked_output(command, "cargo package --list", PACKAGE_TIMEOUT)?;
    let files = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo package file list is not UTF-8: {error}"))?;
    for required in ["Cargo.toml", "Cargo.lock", "README.md", "src/main.rs"] {
        if !files.lines().any(|file| file == required) {
            return Err(format!(
                "cargo install source package is missing `{required}`; package contents were:\n{files}"
            ));
        }
    }
    if files.lines().any(|file| file.starts_with("target/")) {
        return Err(String::from(
            "cargo install source package unexpectedly contains target/ output",
        ));
    }
    Ok(())
}

fn validate_release_mapping(root: &Path, version: &str) -> Result<(), String> {
    let wrapper = read_json(&root.join("npm/bindport/package.json"))?;
    expect_json_string(&wrapper, "name", "bindport", "npm wrapper")?;
    expect_json_string(&wrapper, "version", version, "npm wrapper")?;
    if wrapper["bin"]["bindport"] != "bin/bindport" {
        return Err(String::from(
            "npm wrapper bin.bindport must map to bin/bindport",
        ));
    }
    let optional = wrapper["optionalDependencies"]
        .as_object()
        .ok_or_else(|| String::from("npm wrapper has no optionalDependencies object"))?;

    let workflow = fs::read_to_string(root.join(".github/workflows/release.yml"))
        .map_err(|error| format!("read release workflow: {error}"))?;
    for target in TARGETS {
        if optional.get(target.npm_name).and_then(Value::as_str) != Some(version) {
            return Err(format!(
                "npm wrapper optional dependency {} must be {version}",
                target.npm_name
            ));
        }
        let package = read_json(&root.join("npm").join(target.npm_dir).join("package.json"))?;
        expect_json_string(&package, "name", target.npm_name, target.npm_dir)?;
        expect_json_string(&package, "version", version, target.npm_dir)?;
        expect_single_json_string(&package, "os", target.os, target.npm_dir)?;
        expect_single_json_string(&package, "cpu", target.cpu, target.npm_dir)?;
        let files = package["files"]
            .as_array()
            .ok_or_else(|| format!("{} has no files array", target.npm_name))?;
        if !files.iter().any(|value| value == "bin/bindport") {
            return Err(format!(
                "{} package files must contain bin/bindport",
                target.npm_name
            ));
        }

        let block = workflow_matrix_block(&workflow, target.matrix_name)?;
        require_contains(
            block,
            &format!("asset_name: {}", target.raw_asset),
            &format!("release workflow {} matrix row", target.matrix_name),
        )?;
        require_contains(
            block,
            &format!("npm_platform: {}", target.npm_platform),
            &format!("release workflow {} matrix row", target.matrix_name),
        )?;
    }
    for required in [
        "scripts/stage-cli-assets.sh dist",
        "sha256sum -c ./*.sha256",
    ] {
        require_contains(&workflow, required, "release workflow artifact assembly")?;
    }
    Ok(())
}

fn run_existing_package_checks(root: &Path, temp: &SmokeTemp, version: &str) -> Result<(), String> {
    let checks: [(&str, &str, &[&str]); 5] = [
        (
            "npm package metadata",
            "node",
            &["scripts/npm-package-utils.js", "validate", version],
        ),
        (
            "cargo-binstall metadata",
            "node",
            &["scripts/check-binstall-metadata.js"],
        ),
        ("CLI assets", "scripts/check-cli-assets.sh", &[]),
        (
            "Homebrew formula mapping",
            "scripts/check-homebrew-formula.sh",
            &[],
        ),
        ("npm wrapper matrix", "scripts/npm-smoke.sh", &[]),
    ];

    for (label, program, args) in checks {
        let mut command = Command::new(root.join(program));
        if program == "node" {
            command = Command::new(program);
        }
        command.current_dir(root).args(args);
        isolated_env(&mut command, temp);
        let output = checked_output(command, label, PACKAGE_TIMEOUT)?;
        print_check_output(&output)?;
    }
    Ok(())
}

struct StagedArtifacts {
    wrapper_tarball: PathBuf,
    native_tarball: PathBuf,
}

fn stage_artifacts(
    root: &Path,
    temp: &SmokeTemp,
    binary: &Path,
    version: &str,
) -> Result<StagedArtifacts, String> {
    let dist = temp.path().join("dist");
    fs::create_dir_all(&dist).map_err(|error| format!("create staged dist: {error}"))?;
    let host = host_target()?;

    for target in TARGETS {
        let asset = dist.join(target.raw_asset);
        if target.raw_asset == host.raw_asset {
            fs::copy(binary, &asset).map_err(|error| {
                format!("stage host release binary {}: {error}", target.raw_asset)
            })?;
            make_executable(&asset)?;
        } else {
            fs::write(
                &asset,
                format!("metadata-only placeholder for {}\n", target.raw_asset),
            )
            .map_err(|error| format!("stage foreign target placeholder: {error}"))?;
        }
        write_checksum(&asset)?;
    }

    let mut cli_assets = Command::new(root.join("scripts/stage-cli-assets.sh"));
    cli_assets.current_dir(root).arg(&dist);
    isolated_env(&mut cli_assets, temp);
    checked_output(cli_assets, "stage CLI assets", PACKAGE_TIMEOUT)?;

    let mut native_package = Command::new(root.join("scripts/npm-stage-platform-package.sh"));
    native_package
        .current_dir(root)
        .args([host.npm_platform])
        .arg(binary)
        .arg(&dist);
    isolated_env(&mut native_package, temp);
    let output = checked_output(native_package, "stage host npm package", PACKAGE_TIMEOUT)?;
    print_check_output(&output)?;

    let mut wrapper_package = Command::new("npm");
    wrapper_package
        .current_dir(root.join("npm/bindport"))
        .args(["pack", "--pack-destination"])
        .arg(&dist);
    isolated_env(&mut wrapper_package, temp);
    let output = checked_output(wrapper_package, "stage npm wrapper", PACKAGE_TIMEOUT)?;
    print_check_output(&output)?;

    let wrapper_tarball = dist.join(format!("bindport-{version}.tgz"));
    let native_tarball = dist.join(format!("bindport-{}-{version}.tgz", host.npm_platform));
    for tarball in [&wrapper_tarball, &native_tarball] {
        if !tarball.is_file() {
            return Err(format!("missing staged npm tarball: {}", tarball.display()));
        }
        write_checksum(tarball)?;
    }

    for target in TARGETS {
        verify_checksum(&dist.join(target.raw_asset))?;
    }
    for asset in ["bindport-completions.tar.gz", "bindport-manpage.tar.gz"] {
        verify_checksum(&dist.join(asset))?;
    }
    verify_checksum(&wrapper_tarball)?;
    verify_checksum(&native_tarball)?;

    let formula = temp.path().join("bindport.rb");
    let mut formula_command = Command::new(root.join("scripts/homebrew-formula.sh"));
    formula_command
        .current_dir(root)
        .args(["--version", version, "--dist"])
        .arg(&dist)
        .arg("--output")
        .arg(&formula);
    isolated_env(&mut formula_command, temp);
    checked_output(
        formula_command,
        "generate staged Homebrew formula",
        PACKAGE_TIMEOUT,
    )?;
    let formula_contents = fs::read_to_string(&formula)
        .map_err(|error| format!("read staged Homebrew formula: {error}"))?;
    for target in TARGETS {
        require_contains(
            &formula_contents,
            &format!("releases/download/v{version}/{}", target.raw_asset),
            "generated Homebrew formula",
        )?;
    }
    for required in [
        "completions/bash/bindport",
        "completions/zsh/_bindport",
        "completions/fish/bindport.fish",
        "man/man1/bindport.1",
    ] {
        require_contains(&formula_contents, required, "generated Homebrew formula")?;
    }

    Ok(StagedArtifacts {
        wrapper_tarball,
        native_tarball,
    })
}

fn install_staged_npm(
    temp: &SmokeTemp,
    staged: &StagedArtifacts,
    version: &str,
) -> Result<PathBuf, String> {
    let install = temp.path().join("npm-install");
    fs::create_dir_all(&install).map_err(|error| format!("create npm install root: {error}"))?;
    fs::write(
        install.join("package.json"),
        format!(
            "{{\"name\":\"bindport-release-smoke\",\"private\":true,\"version\":\"{version}\"}}\n"
        ),
    )
    .map_err(|error| format!("write npm smoke package.json: {error}"))?;

    let mut command = Command::new("npm");
    command
        .current_dir(&install)
        .args([
            "install",
            "--silent",
            "--offline",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--omit=optional",
        ])
        .arg(&staged.wrapper_tarball)
        .arg(&staged.native_tarball);
    isolated_env(&mut command, temp);
    checked_output(command, "offline staged npm install", PACKAGE_TIMEOUT)?;

    let wrapper = install.join("node_modules/.bin/bindport");
    if !wrapper.is_file() {
        return Err(format!(
            "staged npm install did not create {}",
            wrapper.display()
        ));
    }
    Ok(wrapper)
}

fn exercise_user_flow(temp: &SmokeTemp, wrapper: &Path, version: &str) -> Result<(), String> {
    let project = temp.path().join("project");
    for path in [
        project.join("apps/api"),
        project.join("apps/web"),
        project.join("apps/stale"),
    ] {
        fs::create_dir_all(&path)
            .map_err(|error| format!("create smoke service {}: {error}", path.display()))?;
    }
    let project = project
        .canonicalize()
        .map_err(|error| format!("canonicalize smoke project: {error}"))?;
    write_service_scripts(&project)?;

    let (range_start, range_end, port_guards) = guarded_port_range(12)?;
    let config = format!(
        r#"project = "release-smoke"
default_range = "{range_start}-{range_end}"
skip_ports = []

[dashboard]
register_service = false

[[services]]
name = "api"
path = "apps/api"
command = ["./smoke-service"]
hostname = "api.release-smoke.localhost"
route_url = "http://{{hostname}}:{{port}}"

[[services]]
name = "web"
path = "apps/web"
command = ["./smoke-service"]
hostname = "web.release-smoke.localhost"
env.API_PORT = "{{services.api.port}}"
env.API_URL = "{{services.api.route_url}}"

[[services]]
name = "stale"
path = "apps/stale"
command = ["./smoke-service"]
hostname = "stale.release-smoke.localhost"

[[outputs]]
name = "routes"
template = "bindport-json-snapshot"
root = ".bindport/generated"
target = "routes.json"
auto_render = false
delete_on = ["removed"]
"#
    );
    fs::write(project.join(".bindport.toml"), config)
        .map_err(|error| format!("write smoke config: {error}"))?;

    let version_output = bindport_output(wrapper, &project, temp, &["--version"], COMMAND_TIMEOUT)?;
    expect_success(&version_output, "staged wrapper --version")?;
    let version_stdout = utf8_stdout(&version_output, "staged wrapper --version")?;
    if !version_stdout.contains(version) {
        return Err(format!(
            "staged wrapper --version did not contain {version}: {version_stdout}"
        ));
    }
    let help_output = bindport_output(wrapper, &project, temp, &["--help"], COMMAND_TIMEOUT)?;
    expect_success(&help_output, "staged wrapper --help")?;
    let help = utf8_stdout(&help_output, "staged wrapper --help")?;
    for expected in [
        "BindPort",
        "reserve --all",
        "dashboard start",
        "port <service>",
    ] {
        require_contains(help, expected, "staged binary help")?;
    }

    drop(port_guards);
    let first_reserve = bindport_output(
        wrapper,
        &project,
        temp,
        &["reserve", "--all"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&first_reserve, "reserve --all")?;
    let second_reserve = bindport_output(
        wrapper,
        &project,
        temp,
        &["reserve", "--all"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&second_reserve, "repeated reserve --all")?;
    if first_reserve.stdout != second_reserve.stdout {
        return Err(String::from(
            "repeated reserve --all changed the staged project assignments",
        ));
    }

    let api_port = exact_port(wrapper, &project, temp, "api")?;
    let web_port = exact_port(wrapper, &project, temp, "web")?;
    let stale_port = exact_port(wrapper, &project, temp, "stale")?;
    if [api_port, web_port, stale_port]
        .iter()
        .any(|port| !(range_start..=range_end).contains(port))
    {
        return Err(String::from(
            "reserve --all assigned a port outside the test-owned range",
        ));
    }

    render_and_assert_states(
        wrapper,
        &project,
        temp,
        &[("api", api_port), ("web", web_port), ("stale", stale_port)],
        "reserved",
    )?;

    let web = bindport_output(wrapper, &project, temp, &["run", "web"], COMMAND_TIMEOUT)?;
    expect_success(&web, "out-of-order web startup")?;
    let expected_web = format!(
        "web={web_port} api={api_port} api_url=http://api.release-smoke.localhost:{api_port} cwd={}",
        project.join("apps/web").display()
    );
    if utf8_stdout(&web, "web startup")?.trim() != expected_web {
        return Err(format!(
            "cross-service env or service cwd mismatch; expected `{expected_web}`, got `{}`",
            utf8_stdout(&web, "web startup")?.trim()
        ));
    }

    let api = bindport_output(wrapper, &project, temp, &["run", "api"], COMMAND_TIMEOUT)?;
    expect_success(&api, "api startup after web")?;
    let expected_api = format!("api={api_port} cwd={}", project.join("apps/api").display());
    if utf8_stdout(&api, "api startup")?.trim() != expected_api {
        return Err(format!(
            "api port or service cwd mismatch; expected `{expected_api}`, got `{}`",
            utf8_stdout(&api, "api startup")?.trim()
        ));
    }

    let stale = bindport_output(wrapper, &project, temp, &["run", "stale"], COMMAND_TIMEOUT)?;
    if stale.status.success() {
        return Err(String::from(
            "stale fixture unexpectedly exited normally; it must terminate its BindPort parent",
        ));
    }
    wait_for_service_state(wrapper, &project, temp, "stale", "stale")?;

    render_and_assert_states(
        wrapper,
        &project,
        temp,
        &[("api", api_port), ("web", web_port)],
        "stopped",
    )?;
    assert_rendered_service_state(&project, "stale", stale_port, "stale")?;

    exercise_dashboard(wrapper, &project, temp)?;
    exercise_cleanup(wrapper, &project, temp)?;
    Ok(())
}

fn write_service_scripts(project: &Path) -> Result<(), String> {
    let scripts = [
        (
            "apps/api/smoke-service",
            "#!/bin/sh\nset -eu\nprintf 'api=%s cwd=%s\\n' \"$PORT\" \"$PWD\"\n",
        ),
        (
            "apps/web/smoke-service",
            "#!/bin/sh\nset -eu\nprintf 'web=%s api=%s api_url=%s cwd=%s\\n' \"$PORT\" \"$API_PORT\" \"$API_URL\" \"$PWD\"\n",
        ),
        (
            "apps/stale/smoke-service",
            "#!/bin/sh\nset -eu\nkill -9 \"$PPID\"\n",
        ),
    ];
    for (relative, contents) in scripts {
        let path = project.join(relative);
        fs::write(&path, contents).map_err(|error| format!("write {}: {error}", path.display()))?;
        make_executable(&path)?;
    }
    Ok(())
}

fn render_and_assert_states(
    wrapper: &Path,
    project: &Path,
    temp: &SmokeTemp,
    services: &[(&str, u16)],
    state: &str,
) -> Result<(), String> {
    let render = bindport_output(
        wrapper,
        project,
        temp,
        &["render", "routes"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&render, "render routes")?;
    require_contains(
        utf8_stdout(&render, "render routes")?,
        "rendered routes: 1 files",
        "render command output",
    )?;
    for (service, port) in services {
        assert_rendered_service_state(project, service, *port, state)?;
    }
    Ok(())
}

fn assert_rendered_service_state(
    project: &Path,
    service: &str,
    port: u16,
    state: &str,
) -> Result<(), String> {
    let path = project.join(".bindport/generated/routes.json");
    let document = read_json(&path)?;
    let routes = document["routes"]
        .as_array()
        .ok_or_else(|| format!("{} has no routes array", path.display()))?;
    let route = routes
        .iter()
        .find(|route| route["service"] == service)
        .ok_or_else(|| format!("rendered route snapshot is missing service `{service}`"))?;
    if route["port"].as_u64() != Some(u64::from(port)) || route["state"] != state {
        return Err(format!(
            "rendered `{service}` expected port {port} state {state}, got port {} state {}",
            route["port"], route["state"]
        ));
    }
    Ok(())
}

fn exercise_dashboard(wrapper: &Path, project: &Path, temp: &SmokeTemp) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("reserve dashboard smoke port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read dashboard smoke port: {error}"))?
        .port();
    drop(listener);
    let port_arg = port.to_string();
    let start = bindport_output(
        wrapper,
        project,
        temp,
        &[
            "dashboard",
            "start",
            "--port",
            &port_arg,
            "--auth",
            "disabled",
            "--no-register-service",
        ],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&start, "dashboard start")?;
    let start_stdout = utf8_stdout(&start, "dashboard start")?;
    require_contains(start_stdout, "dashboard started:", "dashboard start output")?;
    let pid = start_stdout
        .split_whitespace()
        .last()
        .ok_or_else(|| String::from("dashboard start output has no pid"))?
        .parse::<u32>()
        .map_err(|error| format!("dashboard start pid is not numeric: {error}"))?;
    let mut guard = BackgroundGuard::new(pid);
    wait_for_dashboard_health(port)?;

    let status = bindport_output(
        wrapper,
        project,
        temp,
        &["dashboard", "status"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&status, "dashboard status")?;
    require_contains(
        utf8_stdout(&status, "dashboard status")?,
        "dashboard running:",
        "dashboard status output",
    )?;

    let stop = bindport_output(
        wrapper,
        project,
        temp,
        &["dashboard", "stop"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&stop, "dashboard stop")?;
    require_contains(
        utf8_stdout(&stop, "dashboard stop")?,
        "dashboard stopped",
        "dashboard stop output",
    )?;
    wait_for_process_exit(pid, BACKGROUND_TIMEOUT)?;
    guard.disarm();
    Ok(())
}

fn exercise_cleanup(wrapper: &Path, project: &Path, temp: &SmokeTemp) -> Result<(), String> {
    let dry_run = bindport_output(
        wrapper,
        project,
        temp,
        &["clean", "--all", "--dry-run", "--json", "--yes"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&dry_run, "cleanup dry-run")?;
    let preview: Value = serde_json::from_slice(&dry_run.stdout)
        .map_err(|error| format!("parse cleanup dry-run JSON: {error}"))?;
    if preview["dry_run"] != true
        || preview["states"]["stopped"].as_u64().unwrap_or(0) < 2
        || preview["states"]["stale"].as_u64().unwrap_or(0) < 1
    {
        return Err(format!(
            "cleanup dry-run did not include both stopped and stale entries: {preview}"
        ));
    }

    let clean = bindport_output(
        wrapper,
        project,
        temp,
        &["clean", "--all", "--json", "--yes"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&clean, "stopped/stale cleanup")?;
    let report: Value = serde_json::from_slice(&clean.stdout)
        .map_err(|error| format!("parse cleanup JSON: {error}"))?;
    if report["dry_run"] != false
        || report["states"]["stopped"].as_u64().unwrap_or(0) < 2
        || report["states"]["stale"].as_u64().unwrap_or(0) < 1
    {
        return Err(format!(
            "cleanup did not remove both stopped and stale entries: {report}"
        ));
    }

    let status = status_json(wrapper, project, temp)?;
    if !status["services"]
        .as_array()
        .is_some_and(|services| services.is_empty())
    {
        return Err(format!(
            "registry still has services after stopped/stale cleanup: {}",
            status["services"]
        ));
    }
    Ok(())
}

fn exact_port(
    wrapper: &Path,
    project: &Path,
    temp: &SmokeTemp,
    service: &str,
) -> Result<u16, String> {
    let output = bindport_output(
        wrapper,
        project,
        temp,
        &["port", service, "--project", "release-smoke"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&output, &format!("port {service}"))?;
    if !output.stderr.is_empty()
        || output.stdout.last() != Some(&b'\n')
        || output.stdout[..output.stdout.len().saturating_sub(1)]
            .iter()
            .any(|byte| !byte.is_ascii_digit())
    {
        return Err(format!(
            "port {service} must print only a decimal port and newline; stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .map_err(|error| format!("parse port {service}: {error}"))
}

fn wait_for_service_state(
    wrapper: &Path,
    project: &Path,
    temp: &SmokeTemp,
    service: &str,
    expected: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + BACKGROUND_TIMEOUT;
    loop {
        let status = status_json(wrapper, project, temp)?;
        if status["services"]
            .as_array()
            .and_then(|services| services.iter().find(|item| item["service"] == service))
            .is_some_and(|item| item["state"] == expected)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "service `{service}` did not reach state `{expected}` within {:?}",
                BACKGROUND_TIMEOUT
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn status_json(wrapper: &Path, project: &Path, temp: &SmokeTemp) -> Result<Value, String> {
    let output = bindport_output(
        wrapper,
        project,
        temp,
        &["status", "--json"],
        COMMAND_TIMEOUT,
    )?;
    expect_success(&output, "status --json")?;
    serde_json::from_slice(&output.stdout).map_err(|error| format!("parse status JSON: {error}"))
}

fn bindport_output(
    wrapper: &Path,
    project: &Path,
    temp: &SmokeTemp,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let mut command = Command::new(wrapper);
    command.current_dir(project).args(args);
    isolated_env(&mut command, temp);
    timed_output(command, &format!("bindport {}", args.join(" ")), timeout)
}

fn guarded_port_range(count: u16) -> Result<(u16, u16, Vec<TcpListener>), String> {
    for _ in 0..100 {
        let first = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("select test-owned port range: {error}"))?;
        let start = first
            .local_addr()
            .map_err(|error| format!("read test-owned port range: {error}"))?
            .port();
        let Some(end) = start.checked_add(count.saturating_sub(1)) else {
            continue;
        };
        let mut listeners = vec![first];
        let mut complete = true;
        for port in (start + 1)..=end {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => listeners.push(listener),
                Err(_) => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            return Ok((start, end, listeners));
        }
    }
    Err(format!(
        "could not reserve a contiguous {count}-port test-owned range"
    ))
}

fn wait_for_dashboard_health(port: u16) -> Result<(), String> {
    let deadline = Instant::now() + BACKGROUND_TIMEOUT;
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    loop {
        if let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(200)) {
            stream
                .set_read_timeout(Some(Duration::from_millis(500)))
                .map_err(|error| format!("set dashboard health timeout: {error}"))?;
            stream
                .write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .map_err(|error| format!("write dashboard health request: {error}"))?;
            let mut response = String::new();
            stream
                .read_to_string(&mut response)
                .map_err(|error| format!("read dashboard health response: {error}"))?;
            if response.starts_with("HTTP/1.1 200 OK") {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "dashboard did not become healthy on test-owned port {port} within {:?}",
                BACKGROUND_TIMEOUT
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn host_target() -> Result<ReleaseTarget, String> {
    let npm_platform = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("macos", "aarch64") => "darwin-arm64",
        (os, arch) => {
            return Err(format!(
                "release smoke supports Linux/macOS x64/arm64, got {os}-{arch}"
            ));
        }
    };
    TARGETS
        .iter()
        .copied()
        .find(|target| target.npm_platform == npm_platform)
        .ok_or_else(|| format!("release target mapping missing for {npm_platform}"))
}

fn workflow_matrix_block<'a>(workflow: &'a str, name: &str) -> Result<&'a str, String> {
    let marker = format!("          - name: {name}\n");
    let start = workflow
        .find(&marker)
        .ok_or_else(|| format!("release workflow is missing matrix row `{name}`"))?;
    let rest = &workflow[start + marker.len()..];
    let end = rest.find("\n          - name: ").unwrap_or(rest.len());
    Ok(&rest[..end])
}

fn expect_json_string(
    value: &Value,
    key: &str,
    expected: &str,
    context: &str,
) -> Result<(), String> {
    if value[key].as_str() == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "{context} `{key}` must be `{expected}`, got {}",
            value[key]
        ))
    }
}

fn expect_single_json_string(
    value: &Value,
    key: &str,
    expected: &str,
    context: &str,
) -> Result<(), String> {
    let values = value[key]
        .as_array()
        .ok_or_else(|| format!("{context} `{key}` must be an array"))?;
    if values.len() == 1 && values[0].as_str() == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "{context} `{key}` must be [`{expected}`], got {}",
            value[key]
        ))
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let contents = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_slice(&contents).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn require_contains(contents: &str, expected: &str, context: &str) -> Result<(), String> {
    if contents.contains(expected) {
        Ok(())
    } else {
        Err(format!("{context} is missing `{expected}`"))
    }
}

fn write_checksum(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let digest = Sha256::digest(bytes);
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("artifact name is not UTF-8: {}", path.display()))?;
    fs::write(
        path.with_file_name(format!("{name}.sha256")),
        format!("{hash}  {name}\n"),
    )
    .map_err(|error| format!("write checksum for {}: {error}", path.display()))
}

fn verify_checksum(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("artifact name is not UTF-8: {}", path.display()))?;
    let sidecar = path.with_file_name(format!("{name}.sha256"));
    let expected = fs::read_to_string(&sidecar)
        .map_err(|error| format!("read {}: {error}", sidecar.display()))?;
    let mut fields = expected.split_whitespace();
    let expected_hash = fields
        .next()
        .ok_or_else(|| format!("{} has no checksum", sidecar.display()))?;
    let expected_name = fields
        .next()
        .ok_or_else(|| format!("{} has no artifact name", sidecar.display()))?;
    if expected_name != name || fields.next().is_some() {
        return Err(format!(
            "{} must contain exactly `<sha256>  {name}`",
            sidecar.display()
        ));
    }
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected_hash {
        return Err(format!(
            "checksum mismatch for {name}: expected {expected_hash}, got {actual}"
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|error| format!("read permissions for {}: {error}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("make {} executable: {error}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Err(String::from("release smoke requires a Unix host"))
}

fn create_isolated_homes(root: &Path) -> Result<(), String> {
    for directory in [
        "home",
        "xdg-config",
        "xdg-state",
        "xdg-data",
        "xdg-cache",
        "npm-cache",
        "tmp",
    ] {
        fs::create_dir_all(root.join(directory))
            .map_err(|error| format!("create isolated {directory}: {error}"))?;
    }
    fs::write(root.join("npmrc"), "")
        .map_err(|error| format!("create isolated npm config: {error}"))?;
    Ok(())
}

fn isolated_env(command: &mut Command, temp: &SmokeTemp) {
    command
        .env("HOME", temp.path().join("home"))
        .env("XDG_CONFIG_HOME", temp.path().join("xdg-config"))
        .env("XDG_STATE_HOME", temp.path().join("xdg-state"))
        .env("XDG_DATA_HOME", temp.path().join("xdg-data"))
        .env("XDG_CACHE_HOME", temp.path().join("xdg-cache"))
        .env(
            "BINDPORT_REGISTRY_PATH",
            temp.path().join("xdg-state/bindport/release-smoke.sqlite"),
        )
        .env("npm_config_cache", temp.path().join("npm-cache"))
        .env("NPM_CONFIG_USERCONFIG", temp.path().join("npmrc"))
        .env("TMPDIR", temp.path().join("tmp"))
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("BINDPORT_BINARY")
        .env_remove("BINDPORT_PROJECT")
        .env_remove("BINDPORT_SERVICE")
        .env_remove("BINDPORT_DASHBOARD_TOKEN");
}

fn checked_output(command: Command, label: &str, timeout: Duration) -> Result<Output, String> {
    let output = timed_output(command, label, timeout)?;
    expect_success(&output, label)?;
    Ok(output)
}

fn timed_output(mut command: Command, label: &str, timeout: Duration) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|error| format!("start {label}: {error}"))?;
    wait_with_timeout(child, label, timeout)
}

fn wait_with_timeout(mut child: Child, label: &str, timeout: Duration) -> Result<Output, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("collect {label} output: {error}"));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                child.kill().ok();
                child.wait().ok();
                return Err(format!("{label} timed out after {timeout:?}"));
            }
            Err(error) => {
                child.kill().ok();
                child.wait().ok();
                return Err(format!("wait for {label}: {error}"));
            }
        }
    }
}

fn expect_success(output: &Output, label: &str) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        limited_output(&output.stdout),
        limited_output(&output.stderr)
    ))
}

fn utf8_stdout<'a>(output: &'a Output, label: &str) -> Result<&'a str, String> {
    std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("{label} stdout is not UTF-8: {error}"))
}

fn limited_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(4_000).collect()
}

fn print_check_output(output: &Output) -> Result<(), String> {
    std::io::stdout()
        .write_all(&output.stdout)
        .map_err(|error| format!("write package check stdout: {error}"))?;
    std::io::stderr()
        .write_all(&output.stderr)
        .map_err(|error| format!("write package check stderr: {error}"))?;
    Ok(())
}

fn process_running(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn signal_process(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .args([signal, &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while process_running(pid) {
        if Instant::now() >= deadline {
            return Err(format!(
                "background dashboard pid {pid} did not exit within {timeout:?}"
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

struct BackgroundGuard {
    pid: Option<u32>,
}

impl BackgroundGuard {
    fn new(pid: u32) -> Self {
        Self { pid: Some(pid) }
    }

    fn disarm(&mut self) {
        self.pid = None;
    }
}

impl Drop for BackgroundGuard {
    fn drop(&mut self) {
        let Some(pid) = self.pid else {
            return;
        };
        if process_running(pid) {
            signal_process(pid, "-TERM");
            let deadline = Instant::now() + Duration::from_secs(1);
            while process_running(pid) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
        }
        if process_running(pid) {
            signal_process(pid, "-KILL");
        }
    }
}

struct SmokeTemp {
    path: PathBuf,
    cleaned: bool,
}

impl SmokeTemp {
    fn new() -> Result<Self, String> {
        let base = env::temp_dir();
        fs::create_dir_all(&base)
            .map_err(|error| format!("create system temp directory: {error}"))?;
        let base = base
            .canonicalize()
            .map_err(|error| format!("canonicalize system temp directory: {error}"))?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("read system time: {error}"))?
            .as_nanos();
        let path = base.join(format!(
            "bindport-release-smoke-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| format!("create release smoke temp: {error}"))?;
        let path = path
            .canonicalize()
            .map_err(|error| format!("canonicalize release smoke temp: {error}"))?;
        Ok(Self {
            path,
            cleaned: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(&mut self) -> Result<(), String> {
        fs::remove_dir_all(&self.path).map_err(|error| {
            format!(
                "remove release smoke temporary directory {}: {error}",
                self.path.display()
            )
        })?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for SmokeTemp {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "release smoke: failed to remove temporary directory {}: {error}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_mappings_are_unique_and_complete() {
        let mut raw = std::collections::BTreeMap::new();
        let mut npm = std::collections::BTreeMap::new();
        for target in TARGETS {
            assert!(raw.insert(target.raw_asset, target.matrix_name).is_none());
            assert!(npm.insert(target.npm_platform, target.npm_name).is_none());
        }
        assert_eq!(raw.len(), 4);
        assert_eq!(npm.len(), 4);
    }

    #[test]
    fn release_workflow_keeps_target_pairs_together() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root");
        let workflow = fs::read_to_string(root.join(".github/workflows/release.yml"))
            .expect("release workflow");
        for target in TARGETS {
            let block = workflow_matrix_block(&workflow, target.matrix_name).expect("matrix row");
            assert!(block.contains(&format!("asset_name: {}", target.raw_asset)));
            assert!(block.contains(&format!("npm_platform: {}", target.npm_platform)));
        }
    }
}
