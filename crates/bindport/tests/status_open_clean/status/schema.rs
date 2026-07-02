// SPDX-License-Identifier: MIT

use crate::support::*;

#[test]
fn status_schema_document_matches_current_contract() {
    let schema =
        serde_json::from_str::<Value>(include_str!("../../../../../docs/status.schema.json"))
            .expect("status schema json");

    assert_eq!(schema["properties"]["schema_version"]["const"], "0.4");
    assert_eq!(schema["additionalProperties"].as_bool(), Some(false));

    let top_level_required = schema["required"]
        .as_array()
        .expect("top-level required fields")
        .iter()
        .map(|field| field.as_str().expect("required field"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        top_level_required,
        BTreeSet::from([
            "generated_at",
            "hooks",
            "outputs",
            "runs",
            "schema_version",
            "services",
        ])
    );

    let service_required = schema["$defs"]["service"]["required"]
        .as_array()
        .expect("service required fields")
        .iter()
        .map(|field| field.as_str().expect("service required field"))
        .collect::<BTreeSet<_>>();
    assert!(
        BTreeSet::from([
            "project",
            "service",
            "state",
            "port",
            "host",
            "url",
            "hostname",
            "route_url",
            "health_url",
            "worktree_path",
            "worktree_hash",
            "git_common_dir",
            "branch",
            "branch_label",
            "commit",
            "identity_key",
            "pid",
            "command",
            "cwd",
            "started_at",
            "exited_at",
            "exit_code",
            "health",
            "outputs",
            "proxy",
        ])
        .is_subset(&service_required)
    );
}
