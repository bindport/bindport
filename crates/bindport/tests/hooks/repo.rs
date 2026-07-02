// SPDX-License-Identifier: MIT

use crate::support::*;

#[cfg(unix)]
#[test]
fn repo_scoped_hook_trust_applies_to_matching_worktree() {
    let registry_path = temp_registry_path("hooks-repo-scope-registry");
    let repo = temp_test_dir("hooks-repo-scope-root");
    let port = free_loopback_port();
    init_git_repo(&repo, "main");
    write_executable(
        &repo.join("reload-hook"),
        "#!/bin/sh\nprintf 'repo:%s\\n' \"$BINDPORT_HOOK_EVENTS\" >> hook.log\n",
    );
    fs::write(
        repo.join(".bindport.toml"),
        format!("project = \"hooks-repo-scope\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n[hooks]\ntimeout_ms = 1000\n[[hooks.commands]]\nname = \"reload\"\nevents = [\"route_started\"]\ncommand = [\"./reload-hook\"]\n"),
    )
    .expect("write hook config");
    run_git(&repo, ["add", ".bindport.toml", "reload-hook"]);
    run_git(&repo, ["commit", "-m", "add hook fixture"]);

    let worktree = temp_test_dir("hooks-repo-scope-worktree");
    fs::remove_dir(&worktree).expect("remove empty temp dir before worktree add");
    let worktree_arg = worktree.display().to_string();
    run_git(
        &repo,
        ["worktree", "add", &worktree_arg, "-b", "feature/reuse"],
    );

    let trust = bindport_with_registry(&registry_path)
        .current_dir(&repo)
        .args(["hooks", "trust", "--scope", "repo", "reload"])
        .output()
        .expect("trust hook");
    assert!(
        trust.status.success(),
        "trust failed: {}",
        String::from_utf8_lossy(&trust.stderr)
    );

    let run = bindport_with_registry(&registry_path)
        .current_dir(&worktree)
        .args(["run", "web", "--", "sh", "-c", "true"])
        .output()
        .expect("run bindport from second worktree");
    assert!(
        run.status.success(),
        "bindport failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        fs::read_to_string(worktree.join("hook.log"))
            .expect("worktree hook log")
            .contains("repo:route_started")
    );
}
