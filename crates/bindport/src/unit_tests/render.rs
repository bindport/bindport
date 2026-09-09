// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn render_command_parser_and_output_selection_validate_combinations() {
    let (command, options) =
        parse_render_command(&strings(["traefik", "--dry-run"])).expect("render command");
    assert_eq!(command, RenderCommand::Render);
    assert_eq!(options.output.as_deref(), Some("traefik"));
    assert!(options.dry_run);

    let (_, options) = parse_render_command(&strings(["--diff"])).expect("render diff");
    assert!(options.diff);
    let (_, options) = parse_render_command(&strings(["--verbose"])).expect("render verbose");
    assert!(options.verbose);
    let (_, options) = parse_render_command(&strings(["-v"])).expect("render verbose short");
    assert!(options.verbose);

    let (command, _) = parse_render_command(&strings(["--help"])).expect("render help");
    assert_eq!(command, RenderCommand::Help);
    assert!(parse_render_command(&strings(["--all", "traefik"])).is_err());
    assert!(parse_render_command(&strings(["--dry-run", "--repair"])).is_err());
    assert!(parse_render_command(&strings(["--dry-run", "--diff"])).is_err());
    assert!(parse_render_command(&strings(["--diff", "--repair"])).is_err());
    assert!(parse_render_command(&strings(["traefik", "debug"])).is_err());

    let outputs = vec![EffectiveOutputConfig {
        name: String::from("traefik"),
        template: String::from("bindport-traefik"),
        root: None,
        target: String::from("{{ route.slug }}.yml"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: Vec::new(),
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 0,
        vars: BTreeMap::new(),
    }];
    let selected = selected_outputs(outputs.clone(), Some("traefik")).expect("selected");
    assert_eq!(selected.len(), 1);
    assert!(selected_outputs(outputs, Some("missing")).is_err());
}

#[test]
fn diagnostic_log_env_values_are_explicit() {
    assert!(diagnostic_log_env_value_enabled("debug"));
    assert!(diagnostic_log_env_value_enabled("info,debug"));
    assert!(diagnostic_log_env_value_enabled(" verbose "));
    assert!(diagnostic_log_env_value_enabled("1"));
    assert!(!diagnostic_log_env_value_enabled(""));
    assert!(!diagnostic_log_env_value_enabled("info"));
    assert!(!diagnostic_log_env_value_enabled("false"));
}

#[test]
fn lifecycle_removal_candidates_include_superseded_paths_for_planned_routes() {
    let mut output = test_output_config("routes");
    let base_dir = PathBuf::from("/workspace/demo");
    let generated = base_dir.join("generated");
    let render_config = OutputRenderConfig::from(&output);
    let scope = OutputFileScope::new(generated.clone(), base_dir.clone(), None, None);
    let ownership = vec![
        bindport_registry::OutputFileOwnership {
            route_key: String::from("route-a"),
            path: generated.join("old-a.txt"),
            content_hash: String::from("hash-a"),
        },
        bindport_registry::OutputFileOwnership {
            route_key: String::from("route-b"),
            path: generated.join("b.txt"),
            content_hash: String::from("hash-b"),
        },
        bindport_registry::OutputFileOwnership {
            route_key: String::from("route-gone"),
            path: generated.join("gone.txt"),
            content_hash: String::from("hash-gone"),
        },
    ];
    let current_route_keys = BTreeSet::from([String::from("route-a"), String::from("route-b")]);
    let planned_paths = BTreeMap::from([
        (String::from("route-a"), generated.join("new-a.txt")),
        (String::from("route-b"), generated.join("b.txt")),
    ]);
    let delete_route_keys = BTreeSet::new();
    let candidates = |output: &EffectiveOutputConfig| {
        lifecycle_removal_candidates(&LifecycleRemoval {
            output,
            scope: &scope,
            ownership: &ownership,
            current_route_keys: &current_route_keys,
            planned_paths: &planned_paths,
            delete_route_keys: &delete_route_keys,
            base_dir: &base_dir,
            render_config: &render_config,
        })
    };

    let superseded_only = candidates(&output);
    assert_eq!(superseded_only.len(), 1);
    assert_eq!(superseded_only[0].route_key, "route-a");
    assert_eq!(superseded_only[0].path, generated.join("old-a.txt"));
    assert_eq!(superseded_only[0].content_hash, "hash-a");

    output.delete_on = vec![OutputDeleteState::Removed];
    let with_removed = candidates(&output);
    assert_eq!(
        with_removed
            .iter()
            .map(|candidate| candidate.route_key.as_str())
            .collect::<Vec<_>>(),
        vec!["route-a", "route-gone"]
    );
}

#[test]
fn lifecycle_diff_candidates_skip_paths_the_new_plan_still_writes() {
    let mut output = test_output_config("routes");
    output.delete_on = vec![OutputDeleteState::Removed];
    let base_dir = PathBuf::from("/workspace/demo");
    let generated = base_dir.join("generated");
    let render_config = OutputRenderConfig::from(&output);
    let scope = OutputFileScope::new(generated.clone(), base_dir.clone(), None, None);
    let ownership = vec![
        bindport_registry::OutputFileOwnership {
            route_key: String::from("route-a"),
            path: generated.join("a.txt"),
            content_hash: String::from("hash-a"),
        },
        bindport_registry::OutputFileOwnership {
            route_key: String::from("route-b"),
            path: generated.join("b.txt"),
            content_hash: String::from("hash-b"),
        },
        bindport_registry::OutputFileOwnership {
            route_key: String::from("route-gone"),
            path: generated.join("gone.txt"),
            content_hash: String::from("hash-gone"),
        },
    ];
    let current_route_keys = BTreeSet::from([String::from("route-a"), String::from("route-b")]);
    let planned_paths = BTreeMap::from([
        (String::from("route-a"), generated.join("b.txt")),
        (String::from("route-b"), generated.join("a.txt")),
    ]);
    let delete_route_keys = BTreeSet::new();
    let removal = LifecycleRemoval {
        output: &output,
        scope: &scope,
        ownership: &ownership,
        current_route_keys: &current_route_keys,
        planned_paths: &planned_paths,
        delete_route_keys: &delete_route_keys,
        base_dir: &base_dir,
        render_config: &render_config,
    };

    let runtime = lifecycle_removal_candidates(&removal);
    assert_eq!(
        runtime
            .iter()
            .map(|candidate| candidate.route_key.as_str())
            .collect::<Vec<_>>(),
        vec!["route-a", "route-b", "route-gone"]
    );

    let diffed = lifecycle_diff_candidates(&removal);
    assert_eq!(diffed.len(), 1);
    assert_eq!(diffed[0].route_key, "route-gone");
    assert_eq!(diffed[0].path, generated.join("gone.txt"));
}
