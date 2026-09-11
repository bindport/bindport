// SPDX-License-Identifier: MIT

use crate::support::*;

struct BatchFixture {
    root: PathBuf,
    registry_path: PathBuf,
    range: (u16, u16),
    _busy_port: Vec<TcpListener>,
}

impl BatchFixture {
    fn new(name: &str) -> Self {
        let (mut guards, start, end) = guarded_port_range();
        guards.truncate(1);
        let root = temp_test_dir(name).canonicalize().expect("canonical root");
        fs::create_dir_all(root.join(".bindport/templates")).expect("templates directory");
        fs::write(
            root.join(".bindport/templates/route.txt.j2"),
            "{{ route.service }} {{ route.state }} {{ route.port }} {{ route.hostname }} {{ snapshot.route_count }}\n",
        )
        .expect("route template");
        let fixture = Self {
            registry_path: root.join("registry.sqlite3"),
            root,
            range: (start, end),
            _busy_port: guards,
        };
        fixture.configure("");
        fixture
    }

    fn configure(&self, extra: &str) {
        fs::write(
            self.root.join(".bindport.toml"),
            format!(
                "project = \"batch-preflight\"\ndefault_range = \"{}-{}\"\nskip_ports = []\n[[services]]\nname = \"web\"\nhostname = \"{{service}}.localhost\"\n[[services]]\nname = \"api\"\nhostname = \"{{service}}.localhost\"\n{extra}",
                self.range.0, self.range.1
            ),
        )
        .expect("project config");
    }

    fn command(&self, args: &[&str]) -> std::process::Output {
        bindport_with_registry(&self.registry_path)
            .current_dir(&self.root)
            .args(args)
            .output()
            .expect("bindport command")
    }

    fn leases(&self) -> Value {
        let output = self.command(&["registry", "export"]);
        assert_success(&output);
        serde_json::from_slice::<Value>(&output.stdout).expect("export json")["leases"].clone()
    }
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn output_config(template: &str, target: &str, policy: &str, auto_render: bool) -> String {
    format!(
        "[[outputs]]\nname = \"routes\"\ntemplate = \"{template}\"\nroot = \"generated\"\ntarget = '{target}'\non_failure = \"{policy}\"\nauto_render = {auto_render}\n"
    )
}

#[test]
fn batch_missing_blocking_template_preserves_existing_leases() {
    for existing in [false, true] {
        let fixture = BatchFixture::new("batch-missing-template");
        if existing {
            assert_success(&fixture.command(&["reserve", "web"]));
        }
        let before = fixture.leases();
        fixture.configure(&output_config(
            "missing",
            "{{ route.service }}.txt",
            "block",
            true,
        ));

        let output = fixture.command(&["reserve", "--all"]);
        assert!(
            !output.status.success(),
            "missing blocking template must fail"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
        assert_eq!(fixture.leases(), before);
        assert!(!fixture.root.join("generated").exists());
    }
}

#[test]
fn batch_preflight_checks_combined_reserved_routes_for_target_collisions() {
    for existing in [false, true] {
        let fixture = BatchFixture::new("batch-colliding-targets");
        if existing {
            assert_success(&fixture.command(&["reserve", "web"]));
        }
        let before = fixture.leases();
        fixture.configure(&output_config(
            "route",
            "{% if route.state == \"reserved\" %}shared.txt{% else %}{{ route.service }}.txt{% endif %}",
            "block",
            true,
        ));

        let output = fixture.command(&["reserve", "--all"]);
        assert!(
            !output.status.success(),
            "colliding batch must fail before commit"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("multiple routes render to target")
        );
        assert_eq!(fixture.leases(), before);
        assert!(!fixture.root.join("generated").exists());
    }
}

#[test]
fn batch_preflight_does_not_write_earlier_outputs_before_later_failure() {
    let fixture = BatchFixture::new("batch-late-preflight-error");
    let mut outputs = output_config("route", "{{ route.service }}.txt", "block", true);
    outputs.push_str(
        "[[outputs]]\nname = \"missing\"\ntemplate = \"absent\"\nroot = \"generated\"\ntarget = \"absent.txt\"\non_failure = \"block\"\n",
    );
    fixture.configure(&outputs);

    let output = fixture.command(&["reserve", "--all"]);
    assert!(
        !output.status.success(),
        "later preflight failure must fail batch"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("absent"));
    assert_eq!(fixture.leases(), serde_json::json!([]));
    assert!(!fixture.root.join("generated").exists());
}

#[test]
fn batch_preflight_preserves_unowned_targets() {
    let fixture = BatchFixture::new("batch-unowned-target");
    fs::create_dir(fixture.root.join("generated")).expect("output directory");
    let target = fixture.root.join("generated/web.txt");
    fs::write(&target, "user-owned content").expect("unowned target");
    fixture.configure(&output_config(
        "route",
        "{{ route.service }}.txt",
        "block",
        true,
    ));

    let output = fixture.command(&["reserve", "--all"]);
    assert!(!output.status.success(), "unowned target must block batch");
    assert_eq!(fixture.leases(), serde_json::json!([]));
    assert_eq!(
        fs::read_to_string(target).expect("unowned target"),
        "user-owned content"
    );
    assert!(!fixture.root.join("generated/api.txt").exists());
}

#[test]
fn batch_warn_and_manual_outputs_do_not_block_reservation() {
    for (policy, auto_render) in [("warn", true), ("block", false)] {
        let fixture = BatchFixture::new("batch-nonblocking-output");
        fixture.configure(&output_config(
            "missing",
            "missing.txt",
            policy,
            auto_render,
        ));
        let output = fixture.command(&["reserve", "--all"]);
        assert_success(&output);
        assert_eq!(fixture.leases().as_array().expect("leases").len(), 2);
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).contains("missing"),
            auto_render
        );
    }
}

#[test]
fn batch_success_renders_all_reserved_routes_with_assigned_metadata() {
    let fixture = BatchFixture::new("batch-valid-preflight");
    fixture.configure(&output_config(
        "route",
        "{{ route.service }}.txt",
        "block",
        true,
    ));
    assert_success(&fixture.command(&["reserve", "--all"]));
    let leases = fixture.leases();
    let leases = leases.as_array().expect("leases");
    assert_eq!(leases.len(), 2);
    assert_ne!(leases[0]["port"], leases[1]["port"]);
    for lease in leases {
        let name = lease["service"].as_str().expect("service");
        let port = lease["port"].as_u64().expect("port");
        assert!(port > u64::from(fixture.range.0) && port <= u64::from(fixture.range.1));
        assert_eq!(
            fs::read_to_string(fixture.root.join(format!("generated/{name}.txt"))).expect("output"),
            format!("{name} reserved {port} {name}.localhost 2")
        );
    }
}

#[cfg(unix)]
#[test]
fn batch_only_emits_hooks_for_new_reservations() {
    for existing in [false, true] {
        let fixture = BatchFixture::new("batch-idempotent-hooks");
        if existing {
            assert_success(&fixture.command(&["reserve", "web"]));
        }
        let hooks = "[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"count\"\nevents = [\"route_started\"]\ncommand = [\"sh\", \"-c\", \"printf 'started\\n' >> hook.log\"]\n";
        fixture.configure(&format!(
            "{hooks}{}",
            output_config("missing", "missing.txt", "block", true)
        ));
        assert_success(&fixture.command(&["hooks", "trust", "count"]));
        let before_failure = fixture.leases();
        assert!(!fixture.command(&["reserve", "--all"]).status.success());
        assert_eq!(fixture.leases(), before_failure);
        assert!(!fixture.root.join("hook.log").exists());
        fixture.configure(hooks);
        let first = fixture.command(&["reserve", "--all"]);
        assert_success(&first);
        let before = fixture.leases();
        assert_eq!(
            fs::read_to_string(fixture.root.join("hook.log")).expect("hook log"),
            "started\n"
        );

        let second = fixture.command(&["reserve", "--all"]);
        assert_success(&second);
        assert_eq!(second.stdout, first.stdout);
        assert_eq!(fixture.leases(), before);
        assert_eq!(
            fs::read_to_string(fixture.root.join("hook.log")).expect("hook log"),
            "started\n"
        );

        fixture.configure(&format!(
            "{hooks}{}",
            output_config("missing", "missing.txt", "block", true)
        ));
        let reused = fixture.command(&["reserve", "--all"]);
        assert_success(&reused);
        assert!(
            reused.stderr.is_empty(),
            "no-op must not try rendering missing output"
        );
        assert_eq!(fixture.leases(), before);
        assert_eq!(
            fs::read_to_string(fixture.root.join("hook.log")).expect("hook log"),
            "started\n"
        );
    }
}
