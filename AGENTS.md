# AGENTS.md

Behavioral guidelines for AI coding agents working in this repository.

## 1. Think Before Coding
Don't assume. Don't hide confusion. Surface tradeoffs.

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them. Do not pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what is confusing. Ask.

## 2. Simplicity First
Minimum code that solves the problem. Nothing speculative.

- No features beyond what was asked.
- No abstractions for single-use code.
- No flexibility or configurability that was not requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: Would a senior engineer say this is overcomplicated? If yes, simplify.

## 3. Surgical Changes
Touch only what you must. Clean up only your own mess.

When editing existing code:
- Do not improve adjacent code, comments, or formatting.
- Do not refactor things that are not broken.
- Match existing style, even if you would do it differently.
- If you notice unrelated dead code, mention it. Do not delete it.

When your changes create orphans:
- Remove imports, variables, or functions that YOUR changes made unused.
- Do not remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution
Define success criteria. Loop until verified.

Transform tasks into verifiable goals:
- "Add validation" becomes "Write tests for invalid inputs, then make them pass"
- "Fix the bug" becomes "Write a test that reproduces it, then make it pass"
- "Refactor X" becomes "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
1. [Step] -> verify: [check]
2. [Step] -> verify: [check]
3. [Step] -> verify: [check]

Strong success criteria let you loop independently. Weak criteria require constant clarification.

## 5. Destructive Actions Need Explicit Instruction
Don't mutate state outside the working tree without being told to.

- Includes `git commit`, `git push`, `git rebase`, `git reset --hard`, `git clean`, branch or file deletion, and dependency installs that modify lockfiles.
- Drafting an artifact is not an instruction to apply it. Writing a commit message is not an instruction to commit. Writing a script is not an instruction to run it.
- Default action after producing an artifact is to present it. The user applies it.

---

## Project Commands
- Build: `cargo build --release --locked`
- Test: `cargo test --all-targets`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format check: `cargo fmt --all -- --check`
- Type check: `cargo check --all-targets`
- Full local CI: `mise run ci`
- Dashboard dev: `mise run dev-dashboard` (static reload plus Rust server restart)
- Remote dashboard dev: set `BINDPORT_DASHBOARD_TOKEN`, then `mise run dev-dashboard-remote`

## Test Watch Outs
- macOS CI has exposed path and port assumptions: canonicalize temp paths, and use test-owned ports/listener guards instead of shared default port state.
- Split mixed-responsibility source or test files before adding behavior. Keep moved crate unit tests under `src/unit_tests`; `cargo llvm-cov` excluded `src/tests` from coverage accounting.

## Project Context
`bindport -- <command>` wraps a child process, assigns or reuses a stable port,
records SQLite lease/run state, forwards Unix signals, and retries once if an
immediate child failure leaves the assigned port occupied. Service config and
`bindport run` options can inject templated env vars and route metadata.

`bindport doctor` reports obvious registry and OS-listener conflicts, not full
process ownership diagnostics. `bindport clean` removes stopped/stale registry
entries only. `bindport dashboard` is a registry dashboard with `serve` and
background `start` / `status` / `stop`; dashboard write actions are limited to
stopped/stale cleanup and do not run, reserve, release, start, or stop wrapped
services.

## AI Artifacts
Do not commit scratch notes, plans, drafts, or transcripts to the repository. Do not reference local scratch workspaces in any committed file, including source code, comments, docstrings, or documentation. Follow local artifact conventions if the developer's environment provides them; otherwise keep these out of the tree entirely.
