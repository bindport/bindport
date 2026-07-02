// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn hook_plan_reports_source_without_granting_trust() {
    let project_hooks = HooksConfig {
        commands: Some(vec![hook_command("reload")]),
        ..HooksConfig::default()
    };
    let cwd = Path::new("/workspace/demo");
    let project_plan = configured_hook_plan(
        cwd,
        &hook_resolved_config(ConfigSource::Project, project_hooks.clone(), None),
    )
    .expect("project hook plan");
    assert_eq!(
        project_plan.hooks[0].source,
        "project config `/workspace/demo/bindport.toml`"
    );

    let local_command = hook_command("local-reload");
    let local_commands = configured_hook_plan(
        cwd,
        &hook_resolved_config(
            ConfigSource::Project,
            HooksConfig {
                commands: Some(vec![hook_command("project-reload")]),
                ..HooksConfig::default()
            },
            Some(HooksConfig {
                commands: Some(vec![local_command]),
                ..HooksConfig::default()
            }),
        ),
    )
    .expect("local hook plan");
    assert_eq!(local_commands.hooks[0].name, "local-reload");
    assert_eq!(
        local_commands.hooks[0].source,
        "local override config `/workspace/demo/.bindport.local.toml`"
    );

    let fallback = configured_hook_plan(
        cwd,
        &hook_resolved_config(
            ConfigSource::Fallback,
            HooksConfig {
                commands: Some(vec![hook_command("fallback-reload")]),
                ..HooksConfig::default()
            },
            None,
        ),
    )
    .expect("fallback hook plan");
    assert_eq!(
        fallback.hooks[0].source,
        "fallback config `/home/user/.config/bindport/config.toml`"
    );
}

#[test]
fn hook_trust_status_requires_exact_user_scoped_match() {
    let cwd = Path::new("/workspace/demo");
    let plan = configured_hook_plan(
        cwd,
        &hook_resolved_config(
            ConfigSource::Project,
            HooksConfig {
                commands: Some(vec![hook_command("reload")]),
                ..HooksConfig::default()
            },
            None,
        ),
    )
    .expect("hook plan");
    let hook = &plan.hooks[0];
    let subjects = hook_trust_subjects(cwd);
    let mut store = HookTrustStore::default();

    assert_eq!(
        hook_trust_status(hook, &store, &subjects),
        HookTrustStatus::Pending
    );

    upsert_hook_trust_entry(
        &mut store,
        &subjects,
        HookTrustScope::Worktree,
        hook,
        HookDecision::Approved,
    )
    .expect("approve hook");
    assert_eq!(
        hook_trust_status(hook, &store, &subjects),
        HookTrustStatus::Approved {
            scope: HookTrustScope::Worktree
        }
    );

    let mut changed_hook = hook.clone();
    changed_hook.definition.push_str("changed\n");
    assert_eq!(
        hook_trust_status(&changed_hook, &store, &subjects),
        HookTrustStatus::Changed
    );

    upsert_hook_trust_entry(
        &mut store,
        &subjects,
        HookTrustScope::Worktree,
        hook,
        HookDecision::Denied,
    )
    .expect("deny hook");
    assert_eq!(
        hook_trust_status(hook, &store, &subjects),
        HookTrustStatus::Denied {
            scope: HookTrustScope::Worktree
        }
    );
}

#[test]
fn hook_trust_decisions_reset_by_scope_and_name() {
    let cwd = Path::new("/workspace/demo");
    let plan = configured_hook_plan(
        cwd,
        &hook_resolved_config(
            ConfigSource::Project,
            HooksConfig {
                commands: Some(vec![hook_command("reload")]),
                ..HooksConfig::default()
            },
            None,
        ),
    )
    .expect("hook plan");
    let hook = &plan.hooks[0];
    let subjects = HookTrustSubjects {
        worktree: String::from("path:/workspace/demo"),
        repo: Some(String::from("repo:/workspace/demo/.git")),
    };
    let mut store = HookTrustStore::default();

    upsert_hook_trust_entry(
        &mut store,
        &subjects,
        HookTrustScope::Worktree,
        hook,
        HookDecision::Approved,
    )
    .expect("approve worktree");
    upsert_hook_trust_entry(
        &mut store,
        &subjects,
        HookTrustScope::Repo,
        hook,
        HookDecision::Denied,
    )
    .expect("deny repo");
    assert_eq!(store.entries.len(), 2);

    let names = BTreeSet::from([String::from("reload")]);
    assert_eq!(
        reset_hook_trust_entries(&mut store, &subjects, HookTrustScope::Worktree, &names),
        1
    );
    assert_eq!(store.entries.len(), 1);
    assert_eq!(
        reset_hook_trust_entries(
            &mut store,
            &subjects,
            HookTrustScope::Repo,
            &BTreeSet::new()
        ),
        1
    );
    assert!(store.entries.is_empty());

    let no_repo_subjects = HookTrustSubjects {
        worktree: String::from("path:/workspace/demo"),
        repo: None,
    };
    assert_eq!(
        reset_hook_trust_entries(
            &mut store,
            &no_repo_subjects,
            HookTrustScope::Repo,
            &BTreeSet::new()
        ),
        0
    );
    assert!(
        upsert_hook_trust_entry(
            &mut store,
            &no_repo_subjects,
            HookTrustScope::Repo,
            hook,
            HookDecision::Approved,
        )
        .expect_err("repo subject required")
        .contains("repo scope")
    );
    assert!(unix_timestamp_string().parse::<u64>().is_ok());
}

#[test]
fn hooks_command_errors_preserve_error_variants() {
    let config = ConfigError::UnknownFormat {
        path: PathBuf::from("/tmp/bindport.txt"),
    };
    assert!(matches!(
        HooksCommandError::from(config),
        HooksCommandError::Config(_)
    ));
    assert!(matches!(
        HooksCommandError::from(io::Error::other("hook io")),
        HooksCommandError::Io(_)
    ));
}

#[test]
fn hooks_command_parser_covers_selectors_scopes_and_errors() {
    let options = parse_hooks_command(&[]).expect("default hooks status");
    assert_eq!(options.command, HooksCommand::Status);
    assert_eq!(options.scope, HookTrustScope::Worktree);
    assert!(!options.all);
    assert!(options.name.is_none());

    let options =
        parse_hooks_command(&strings(["trust", "reload", "--scope", "repo"])).expect("trust");
    assert_eq!(options.command, HooksCommand::Trust);
    assert_eq!(options.scope, HookTrustScope::Repo);
    assert_eq!(options.name.as_deref(), Some("reload"));

    let options = parse_hooks_command(&strings(["deny", "--all"])).expect("deny all");
    assert_eq!(options.command, HooksCommand::Deny);
    assert!(options.all);

    let options =
        parse_hooks_command(&strings(["reset", "reload", "--help"])).expect("help override");
    assert_eq!(options.command, HooksCommand::Help);
    assert_eq!(options.name.as_deref(), Some("reload"));

    let options = parse_hooks_command(&strings(["help"])).expect("help");
    assert_eq!(options.command, HooksCommand::Help);

    for args in [
        strings(["unknown"]),
        strings(["trust", "--scope"]),
        strings(["trust", "--scope", "global", "reload"]),
        strings(["trust", "--bad", "reload"]),
        strings(["trust", "one", "two"]),
        strings(["trust", "reload", "--all"]),
        strings(["trust"]),
        strings(["status", "--all"]),
        strings(["status", "reload"]),
    ] {
        assert!(
            parse_hooks_command(&args).is_err(),
            "expected parse failure for {args:?}"
        );
    }
}

#[test]
fn hooks_status_json_reports_configured_hook_metadata() {
    let cwd = Path::new("/workspace/demo");
    let config = hook_resolved_config(
        ConfigSource::Project,
        HooksConfig {
            commands: Some(vec![HookCommandConfig {
                name: Some(String::from("reload")),
                events: Some(vec![HookEvent::RouteStarted, HookEvent::RoutesRemoved]),
                command: Some(vec![String::from("traefik"), String::from("reload")]),
                timeout_ms: Some(1_500),
                ..HookCommandConfig::default()
            }]),
            ..HooksConfig::default()
        },
        None,
    );
    let state_home = temp_test_dir("hooks-status-state");
    with_state_home(&state_home, || {
        let status = hooks_status_json(cwd, &config);
        let items = status["items"].as_array().expect("items");
        let item = items.first().expect("hook status");

        assert_eq!(item["name"], "reload");
        assert_eq!(item["status"], "pending");
        assert_eq!(item["trust"], "pending");
        assert_eq!(item["events"][0], "route_started");
        assert_eq!(item["events"][1], "routes_removed");
        assert_eq!(item["command"][0], "traefik");
        assert_eq!(item["command_display"], "traefik reload");
        assert_eq!(item["timeout_ms"], 1_500);
        assert_eq!(item["target"]["kind"], "opaque");
        assert!(
            item["hook_hash"]
                .as_str()
                .is_some_and(|hash| !hash.is_empty())
        );
        assert!(
            item["target"]["hash"]
                .as_str()
                .is_some_and(|hash| !hash.is_empty())
        );
    });
}

#[test]
fn hook_status_display_helpers_are_stable() {
    assert_eq!(
        hook_trust_status_display(HookTrustStatus::Approved {
            scope: HookTrustScope::Repo,
        }),
        "approved (repo)"
    );
    assert_eq!(
        hook_trust_status_display(HookTrustStatus::Denied {
            scope: HookTrustScope::Worktree,
        }),
        "denied (worktree)"
    );
    assert_eq!(
        hook_events_display(&[HookEvent::RouteStarted, HookEvent::RoutesRemoved]),
        "route_started, routes_removed"
    );
}
