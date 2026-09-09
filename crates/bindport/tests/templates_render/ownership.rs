// SPDX-License-Identifier: MIT

use crate::support::*;

struct OwnershipFixture {
    registry_path: PathBuf,
    root: PathBuf,
    range_start: u16,
    services: Vec<&'static str>,
    output_root: &'static str,
    auto_render: bool,
    on_failure: &'static str,
}

impl OwnershipFixture {
    fn new(name: &str, services: &[&'static str]) -> Self {
        let root = temp_test_dir(&format!("{name}-root"))
            .canonicalize()
            .expect("canonical fixture root");
        let template_dir = root.join(".bindport/templates");
        fs::create_dir_all(&template_dir).expect("template dir");
        fs::write(
            template_dir.join("route.txt.j2"),
            "service={{ route.service }} version={{ vars.version }}\n",
        )
        .expect("write route template");

        Self {
            registry_path: temp_registry_path(&format!("{name}-registry")),
            root,
            range_start: free_loopback_port().clamp(20_000, 65_520),
            services: services.to_vec(),
            output_root: "generated",
            auto_render: false,
            on_failure: "warn",
        }
    }

    fn write_config(&self, target: &str, version: &str) {
        let services = self
            .services
            .iter()
            .map(|service| format!("[[services]]\nname = \"{service}\"\n"))
            .collect::<String>();
        fs::write(
            self.root.join(".bindport.toml"),
            format!(
                "project = \"output-ownership\"\ndefault_range = \"{}-{}\"\nskip_ports = []\n{services}[[outputs]]\nname = \"routes\"\ntemplate = \"route\"\nroot = \"{}\"\ntarget = \"{target}\"\nauto_render = {}\non_failure = \"{}\"\n[outputs.vars]\nversion = \"{version}\"\n",
                self.range_start,
                self.range_start + 7,
                self.output_root,
                self.auto_render,
                self.on_failure
            ),
        )
        .expect("write output config");
    }

    fn bindport(&self, args: &[&str]) -> std::process::Output {
        bindport_with_registry(&self.registry_path)
            .current_dir(&self.root)
            .args(args)
            .output()
            .expect("run bindport")
    }

    fn status(&self) -> Value {
        let output = bindport_with_registry(&self.registry_path)
            .args(["status", "--json"])
            .output()
            .expect("status json");
        assert!(
            output.status.success(),
            "status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("status json")
    }

    fn render_order(&self) -> Vec<String> {
        self.status()["services"]
            .as_array()
            .expect("services")
            .iter()
            .map(|service| {
                service["service"]
                    .as_str()
                    .expect("service name")
                    .to_owned()
            })
            .collect()
    }

    fn generated(&self, relative: &str) -> PathBuf {
        self.root.join(self.output_root).join(relative)
    }

    fn export_output_files(&self) -> Value {
        let output = bindport_with_registry(&self.registry_path)
            .args(["registry", "export"])
            .output()
            .expect("registry export");
        assert!(
            output.status.success(),
            "export failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let export: Value = serde_json::from_slice(&output.stdout).expect("export json");
        export["output_files"].clone()
    }
}

fn assert_success(output: &std::process::Output, context: &str) -> String {
    assert!(
        output.status.success(),
        "{context} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_failure(output: &std::process::Output, needle: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "expected failure, got: {stderr}");
    assert!(stderr.contains(needle), "unexpected stderr: {stderr}");
    stderr
}

fn canonical_string(path: &Path) -> String {
    path.canonicalize()
        .expect("canonical path")
        .display()
        .to_string()
}

fn service_output<'a>(status: &'a Value, service: &str) -> &'a Value {
    let entry = status["services"]
        .as_array()
        .expect("services")
        .iter()
        .find(|entry| entry["service"] == service)
        .expect("service entry");
    &entry["outputs"][0]
}

#[cfg(unix)]
fn make_read_only(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).expect("read-only dir");
    let probe = dir.join(".write-probe");
    if fs::write(&probe, "").is_ok() {
        let _ = fs::remove_file(&probe);
        make_writable(dir);
        return false;
    }
    true
}

#[cfg(unix)]
fn make_writable(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).expect("writable dir");
}

#[test]
fn render_refuses_unowned_targets_before_writing_any_file() {
    let fixture = OwnershipFixture::new("render-prevalidate", &["web", "api"]);
    fixture.write_config("{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "--all"]), "reserve all");
    let order = fixture.render_order();
    let first = fixture.generated(&format!("{}.txt", order[0]));
    let second = fixture.generated(&format!("{}.txt", order[1]));
    fs::create_dir_all(second.parent().expect("parent")).expect("output root");
    fs::write(&second, "user-owned\n").expect("unowned file");

    let stderr = assert_failure(
        &fixture.bindport(&["render", "routes"]),
        "refusing to overwrite unowned output file",
    );
    assert!(stderr.contains(&second.display().to_string()));
    assert!(
        !first.exists(),
        "no file should be written when another target is unowned"
    );
    assert_eq!(
        fs::read_to_string(&second).expect("unowned file"),
        "user-owned\n"
    );

    fs::remove_file(&second).expect("remove unowned file");
    assert_success(&fixture.bindport(&["render", "routes"]), "render after fix");
    assert!(first.is_file());
    assert!(second.is_file());
    assert_eq!(fixture.status()["outputs"][0]["rendered"], 2);
}

#[cfg(unix)]
#[test]
fn render_records_ownership_of_files_written_before_a_later_io_failure() {
    let fixture = OwnershipFixture::new("render-partial-new", &["web", "api"]);
    fixture.write_config("{{ route.service }}/route.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "--all"]), "reserve all");
    let order = fixture.render_order();
    let first = fixture.generated(&format!("{}/route.txt", order[0]));
    let second_dir = fixture.generated(&order[1]);
    fs::create_dir_all(&second_dir).expect("second dir");
    if !make_read_only(&second_dir) {
        return;
    }

    let stderr = assert_failure(&fixture.bindport(&["render", "routes"]), "route.txt");
    assert!(!stderr.contains("unowned"), "unexpected stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(&first).expect("first file"),
        format!("service={} version=1", order[0])
    );
    let status = fixture.status();
    assert_eq!(service_output(&status, &order[0])["status"], "rendered");
    assert_eq!(
        service_output(&status, &order[0])["path"],
        canonical_string(&first)
    );

    make_writable(&second_dir);
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render after restoring permissions",
    );
    assert!(second_dir.join("route.txt").is_file());
    assert_eq!(fixture.status()["outputs"][0]["rendered"], 2);
}

#[cfg(unix)]
#[test]
fn render_records_ownership_of_owned_files_updated_before_a_later_io_failure() {
    let fixture = OwnershipFixture::new("render-partial-owned", &["web", "api"]);
    fixture.write_config("{{ route.service }}/route.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "--all"]), "reserve all");
    assert_success(&fixture.bindport(&["render", "routes"]), "initial render");
    let order = fixture.render_order();
    let first = fixture.generated(&format!("{}/route.txt", order[0]));
    let second_dir = fixture.generated(&order[1]);
    let second = second_dir.join("route.txt");
    assert!(first.is_file());
    assert!(second.is_file());
    if !make_read_only(&second_dir) {
        return;
    }

    fixture.write_config("{{ route.service }}/route.txt", "2");
    let stderr = assert_failure(&fixture.bindport(&["render", "routes"]), "route.txt");
    assert!(!stderr.contains("unowned"), "unexpected stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(&first).expect("first file"),
        format!("service={} version=2", order[0])
    );
    assert_eq!(
        fs::read_to_string(&second).expect("second file"),
        format!("service={} version=1", order[1])
    );
    let status = fixture.status();
    assert_eq!(service_output(&status, &order[0])["status"], "rendered");
    assert_eq!(service_output(&status, &order[1])["status"], "rendered");

    make_writable(&second_dir);
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render after restoring permissions",
    );
    assert_eq!(
        fs::read_to_string(&first).expect("first file"),
        format!("service={} version=2", order[0])
    );
    assert_eq!(
        fs::read_to_string(&second).expect("second file"),
        format!("service={} version=2", order[1])
    );
}

#[test]
fn render_removes_superseded_files_when_the_target_changes() {
    let fixture = OwnershipFixture::new("render-superseded", &["web"]);
    let old = fixture.generated("old-web.txt");
    let new = fixture.generated("new-web.txt");
    fixture.write_config("old-{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "web"]), "reserve");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render old target",
    );
    assert!(old.is_file());

    fixture.write_config("new-{{ route.service }}.txt", "1");
    let diff = assert_success(
        &fixture.bindport(&["render", "routes", "--diff"]),
        "diff changed target",
    );
    assert!(
        diff.contains("diff routes: 1 added, 0 modified, 1 removed, 0 unchanged"),
        "unexpected diff output: {diff}"
    );
    assert!(diff.contains("diff --bindport removed generated/old-web.txt"));
    assert!(old.is_file());
    assert!(!new.exists());

    let stdout = assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render changed target",
    );
    assert!(stdout.contains("rendered routes: 1 files"));
    assert!(stdout.contains("removed routes: 1 files"));
    assert!(!old.exists(), "superseded file should be removed");
    assert!(new.is_file());
    let status = fixture.status();
    assert_eq!(status["outputs"][0]["rendered"], 1);
    assert_eq!(status["outputs"][0]["removed"], 0);
    assert_eq!(
        service_output(&status, "web")["path"],
        canonical_string(&new)
    );

    fixture.write_config("old-{{ route.service }}.txt", "1");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render renamed back",
    );
    assert!(old.is_file());
    assert!(!new.exists());
    assert_eq!(
        service_output(&fixture.status(), "web")["path"],
        canonical_string(&old)
    );
}

#[test]
fn clean_removes_superseded_files_after_a_target_change() {
    let fixture = OwnershipFixture::new("clean-superseded", &["web"]);
    let old = fixture.generated("old-web.txt");
    let new = fixture.generated("new-web.txt");
    fixture.write_config("old-{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "web"]), "reserve");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render old target",
    );
    fixture.write_config("new-{{ route.service }}.txt", "1");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render new target",
    );
    assert_success(&fixture.bindport(&["release", "web"]), "release");
    assert_success(
        &fixture.bindport(&["clean", "--stopped", "--yes"]),
        "clean stopped",
    );

    let stdout = assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render after clean",
    );
    assert!(stdout.contains("removed routes: 1 files"));
    assert!(!new.exists());
    assert!(!old.exists(), "old target must not survive cleanup");
}

#[test]
fn render_preserves_externally_modified_superseded_files() {
    let fixture = OwnershipFixture::new("render-superseded-modified", &["web"]);
    let old = fixture.generated("old-web.txt");
    let new = fixture.generated("new-web.txt");
    fixture.write_config("old-{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "web"]), "reserve");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render old target",
    );
    fs::write(&old, "manual edit\n").expect("modify old file");

    fixture.write_config("new-{{ route.service }}.txt", "1");
    let stderr = assert_failure(
        &fixture.bindport(&["render", "routes"]),
        "externally modified",
    );
    assert!(stderr.contains(&old.display().to_string()));
    assert_eq!(fs::read_to_string(&old).expect("old file"), "manual edit\n");
    assert!(!new.exists());
    let status = fixture.status();
    assert_eq!(service_output(&status, "web")["status"], "error");
    assert_eq!(
        service_output(&status, "web")["reason"],
        "external_modified"
    );
    assert_eq!(
        service_output(&status, "web")["path"],
        canonical_string(&old)
    );

    let repair = assert_success(
        &fixture.bindport(&["render", "routes", "--repair"]),
        "repair",
    );
    assert!(repair.contains("repaired routes: 0 files"));
    assert!(repair.contains("preserved routes: 1 externally modified files"));
    assert_eq!(fs::read_to_string(&old).expect("old file"), "manual edit\n");
    assert!(!new.exists());

    fs::remove_file(&old).expect("remove modified file");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render after removal",
    );
    assert!(new.is_file());
    let status = fixture.status();
    assert_eq!(status["outputs"][0]["rendered"], 1);
    assert_eq!(status["outputs"][0]["error"], 0);
    assert_eq!(
        service_output(&status, "web")["path"],
        canonical_string(&new)
    );
}

#[test]
fn auto_render_removes_superseded_files_for_route_dependent_targets() {
    let mut fixture = OwnershipFixture::new("auto-render-superseded", &["web"]);
    fixture.auto_render = true;
    fixture.write_config("{{ route.state }}-{{ route.service }}.txt", "1");

    assert_success(
        &fixture.bindport(&["run", "web", "--", "sh", "-c", "true"]),
        "run",
    );

    assert!(fixture.generated("stopped-web.txt").is_file());
    assert!(
        !fixture.generated("active-web.txt").exists(),
        "active-state file should be superseded by the stopped-state file"
    );
    let status = fixture.status();
    assert_eq!(status["outputs"][0]["rendered"], 1);
    assert_eq!(status["outputs"][0]["removed"], 0);
}

#[test]
fn render_leaves_files_in_a_superseded_output_root_alone() {
    let mut fixture = OwnershipFixture::new("render-superseded-root", &["web"]);
    fixture.write_config("{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "web"]), "reserve");
    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render first root",
    );
    let first_root_file = fixture.generated("web.txt");
    assert!(first_root_file.is_file());

    fixture.output_root = "generated-next";
    fixture.write_config("{{ route.service }}.txt", "1");
    let stdout = assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render second root",
    );
    assert!(!stdout.contains("removed routes"));
    assert!(
        first_root_file.is_file(),
        "foreign-root file must be untouched"
    );
    assert!(fixture.generated("web.txt").is_file());
    assert_eq!(fixture.status()["outputs"][0]["rendered"], 2);
}

#[test]
fn run_preflight_blocks_start_when_a_superseded_file_is_externally_modified() {
    let mut fixture = OwnershipFixture::new("preflight-superseded", &["web"]);
    fixture.auto_render = true;
    fixture.on_failure = "block";
    let old = fixture.generated("old-web.txt");
    let new = fixture.generated("new-web.txt");
    let marker = fixture.root.join("child-ran");
    let marker_arg = marker.display().to_string();
    fixture.write_config("old-{{ route.service }}.txt", "1");
    assert_success(
        &fixture.bindport(&["run", "web", "--", "sh", "-c", "true"]),
        "first run",
    );
    assert!(old.is_file());
    fs::write(&old, "manual edit\n").expect("modify old file");

    fixture.write_config("new-{{ route.service }}.txt", "1");
    let stderr = assert_failure(
        &fixture.bindport(&[
            "run",
            "web",
            "--",
            "sh",
            "-c",
            "printf ran > \"$1\"",
            "sh",
            &marker_arg,
        ]),
        "refusing to abandon externally modified output file",
    );
    assert!(stderr.contains(&old.display().to_string()));
    assert!(
        !marker.exists(),
        "child must not start when preflight fails"
    );
    assert_eq!(fs::read_to_string(&old).expect("old file"), "manual edit\n");
    assert!(!new.exists());
}

#[test]
fn render_diff_reports_swapped_targets_once_without_touching_files_or_rows() {
    let fixture = OwnershipFixture::new("render-diff-swap", &["web", "api"]);
    let web = fixture.generated("web.txt");
    let api = fixture.generated("api.txt");
    fixture.write_config("{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "--all"]), "reserve all");
    assert_success(&fixture.bindport(&["render", "routes"]), "initial render");
    let rows_before = fixture.export_output_files();

    fixture.write_config(
        "{% if route.service == 'web' %}api{% else %}web{% endif %}.txt",
        "2",
    );
    let diff = assert_success(
        &fixture.bindport(&["render", "routes", "--diff"]),
        "diff swapped targets",
    );
    assert!(
        diff.contains("diff routes: 0 added, 2 modified, 0 removed, 0 unchanged"),
        "unexpected diff output: {diff}"
    );
    assert_eq!(diff.matches("diff --bindport modified").count(), 2);
    assert!(
        !diff.contains("diff --bindport removed"),
        "swapped paths must not be reported as removed: {diff}"
    );
    assert_eq!(
        fs::read_to_string(&web).expect("web file"),
        "service=web version=1"
    );
    assert_eq!(
        fs::read_to_string(&api).expect("api file"),
        "service=api version=1"
    );
    assert_eq!(fixture.export_output_files(), rows_before);

    assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render swapped targets",
    );
    assert_eq!(
        fs::read_to_string(&web).expect("web file"),
        "service=api version=2"
    );
    assert_eq!(
        fs::read_to_string(&api).expect("api file"),
        "service=web version=2"
    );
    let status = fixture.status();
    assert_eq!(status["outputs"][0]["rendered"], 2);
    assert_eq!(
        service_output(&status, "web")["path"],
        canonical_string(&api)
    );
    assert_eq!(
        service_output(&status, "api")["path"],
        canonical_string(&web)
    );
}

#[test]
fn render_diff_reports_a_transferred_path_once_and_abandoned_paths_as_removed() {
    let fixture = OwnershipFixture::new("render-diff-transfer", &["web", "api"]);
    let web = fixture.generated("web.txt");
    let api = fixture.generated("api.txt");
    fixture.write_config("{{ route.service }}.txt", "1");
    assert_success(&fixture.bindport(&["reserve", "--all"]), "reserve all");
    assert_success(&fixture.bindport(&["render", "routes"]), "initial render");
    assert_success(&fixture.bindport(&["release", "web"]), "release web");
    assert_success(
        &fixture.bindport(&["clean", "--stopped", "--yes"]),
        "clean web",
    );
    let rows_before = fixture.export_output_files();

    fixture.write_config("web.txt", "2");
    let diff = assert_success(
        &fixture.bindport(&["render", "routes", "--diff"]),
        "diff transferred target",
    );
    assert!(
        diff.contains("diff routes: 0 added, 1 modified, 1 removed, 0 unchanged"),
        "unexpected diff output: {diff}"
    );
    assert!(diff.contains("diff --bindport modified web.txt"));
    assert!(diff.contains("diff --bindport removed generated/api.txt"));
    assert!(
        !diff.contains("diff --bindport removed generated/web.txt"),
        "transferred path reported twice: {diff}"
    );
    assert_eq!(
        fs::read_to_string(&web).expect("web file"),
        "service=web version=1"
    );
    assert_eq!(
        fs::read_to_string(&api).expect("api file"),
        "service=api version=1"
    );
    assert_eq!(fixture.export_output_files(), rows_before);

    let stdout = assert_success(
        &fixture.bindport(&["render", "routes"]),
        "render transferred target",
    );
    assert!(stdout.contains("rendered routes: 1 files"));
    assert_eq!(
        fs::read_to_string(&web).expect("web file"),
        "service=api version=2"
    );
    assert!(!api.exists(), "abandoned path must be removed");
    let status = fixture.status();
    assert_eq!(status["outputs"][0]["rendered"], 1);
    assert_eq!(
        service_output(&status, "api")["path"],
        canonical_string(&web)
    );
}
