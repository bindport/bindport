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
