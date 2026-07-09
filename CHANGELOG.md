# Changelog

All notable changes are generated from git tags and Conventional Commit subjects.

## v0.7.0 - 2026-07-09

[Compare changes](https://github.com/bindport/bindport/compare/v0.6.2...v0.7.0)

### Features

- Add output route snapshot contract (34b3148)
- Scope rendered output ownership (f516d38)
- Add Caddy output template (402e1de)
- Add JSON snapshot output template (dfed5bf)
- Add render diff previews (a0b084f)
- Add grouped service list command (f765379)
- Add registry export command (b85ebc8)
- Add output target diagnostics to doctor (1c8c59c)
- Add render diagnostics logging (d5dd2e1)
- Add nginx and HAProxy output templates (d52d8be)


### Fixes

- Bump status schema contract to 0.7 (4f0dcd5)


### Documentation

- Add proxy output setup guide (1049c29)
- Add optional output integration patterns (1c3660b)


### Tests

- Canonicalize output repair fixture scope (cdb3601)

## v0.6.2 - 2026-07-08

[Compare changes](https://github.com/bindport/bindport/compare/v0.6.1...v0.6.2)

### Fixes

- Recover stale output ownership safely (cbd592a)

## v0.6.1 - 2026-07-07

[Compare changes](https://github.com/bindport/bindport/compare/v0.6.0...v0.6.1)

### Fixes

- Resolve monorepo hook paths from project config root (594bc41)


### Tests

- Relax hook event assertion for macOS compatibility (0dbd36e)
- Avoid exact hook event ordering assumptions (40f66de)


### Build

- Exclude xtask from release version bumps (23a0afb)


### Dependencies

- Bump sha2 from 0.10.9 to 0.11.0 in the cargo-dependencies group (d5fc3eb)

## v0.6.0 - 2026-07-05

[Compare changes](https://github.com/bindport/bindport/compare/v0.5.1...v0.6.0)

### Features

- Initialize project config by default (3e3439d)
- Add cargo-binstall release metadata (c489e09)
- Add CLI reserve and release commands (94f735c)
- Add CLI completion and manpage artifacts (b36ac82)


### Fixes

- Resolve npm wrapper platform package lookup (525774f)
- Remove unsupported identity from config examples (e2319d2)
- Harden platform process matching (5c38b4b)
- Match shell-exec child commands (1a479e2)


### Refactoring

- Split large modules by domain (2c2a09f)
- Split command domains into focused modules (661fa77)
- Split runner and diagnostics internals (e8e0a40)
- Split CLI command internals (44adc1b)
- Split code and tests into focused modules (9c4c4eb)


### Documentation

- Tighten agent guidance for code organization (07cc1d7)
- Clarify platform support (922416c)
- Add adoption setup guidance (8b71674)
- Add install channel guide (2822d41)
- Update agent guidance for docs tooling (5a15710)
- Add mdBook documentation site (a634a54)


### Tests

- Raise workspace coverage (7891941)
- Expand npm platform smoke coverage (9227701)
- Cover registry pressure cleanup (aa0845b)
- Align active process fixtures with matching commands (2d977c9)


### CI

- Serialize mise installs in workflows (8f0537e)
- Reduce Linux workflow setup time (c807f0a)
- Avoid slow mise task auto-installs (d7438ce)
- Build docs site in pull request checks (b9839e6)
- Run docs build without mise task overhead (5a8dbbf)


### Build

- Add git-cliff changelog generation (847a40d)
- Add Homebrew formula release tooling (96fb391)
- Move maintenance helpers into xtask (9b4bae4)

## v0.5.1 - 2026-07-02

[Compare changes](https://github.com/bindport/bindport/compare/v0.5.0...v0.5.1)

### Fixes

- Harden clone-and-run trust boundaries (6ad5eb0)


### Documentation

- Update v0.5.1 release guidance (cc05a9b)

## v0.5.0 - 2026-07-02

[Compare changes](https://github.com/bindport/bindport/compare/v0.4.0...v0.5.0)

### Features

- Add configured service command execution (08a6b87)
- Require explicit trust for lifecycle hooks (8bca562)
- Add loopback service health checks (9c96b85)
- Harden registry cleanup and conflict reporting (a3e285d)
- Add service URL lookup command (3f63995)


### Fixes

- Stabilize health probe test server (4d736c8)


### Documentation

- Document v0.5 release and status contract (8a127db)
- Update README for v0.5.0 release (c8e84d8)

## v0.4.0 - 2026-07-01

[Compare changes](https://github.com/bindport/bindport/compare/v0.3.0...v0.4.0)

### Features

- Add service path inference for monorepos (5a4dd56)
- Add workspace root identity inference (0985743)
- Add config explain diagnostics (6401988)
- Add config validation command (714cf7a)
- Add opt-in .env.local output (f5f0a1f)
- Add npm platform package support (f2ca203)


### Documentation

- Add monorepo configuration guides (3f60fa3)


### Tests

- Add workspace coverage gate (7328ea1)

## v0.3.0 - 2026-06-30

[Compare changes](https://github.com/bindport/bindport/compare/v0.2.0...v0.3.0)

### Features

- Add output config and local overrides (322524c)
- Add output template commands (cf8c119)
- Add output render model (f2ebb17)
- Add manual output file rendering (fc9f563)
- Auto-render outputs for wrapped commands (fe1eca4)
- Add output diagnostics for render planning (84c850e)
- Add output file lifecycle cleanup (92914d8)
- Expose output status in registry snapshots (7474411)
- Trigger output cleanup from dashboard actions (fa0cc28)
- Add render repair and output failure policy (bb74d36)
- Add route event flow for output rendering (5e51c8b)


### Fixes

- Allow symlinked output base paths (a71505a)
- Normalize rendered output path assertions (92fee0c)


### Documentation

- Add template output setup guide (ebde8ab)


### Tests

- Stabilize dashboard fallback test on macOS (450528b)
- Cover output rendering edge cases (ef97b76)


### CI

- Streamline release publishing (7e08668)

## v0.2.0 - 2026-06-29

[Compare changes](https://github.com/bindport/bindport/compare/v0.1.0...v0.2.0)

### Features

- Add read-only dashboard server (cbc91c0)
- Group dashboard services by state (22a7762)
- Add dashboard URL actions (e5f2e97)
- Add dashboard auto-refresh (311830a)
- Add dashboard service filters (17df782)
- Make dashboard configurable for remote dev (53fc925)
- Add registry cleanup command (d95d786)
- Add dashboard cleanup actions (9a3f3a6)
- Add service env templates (b8327f0)
- Add dashboard service registration (f23e2b3)


### Fixes

- Restart dashboard dev servers on Rust changes (9660b3e)


### Documentation

- Add dashboard usage guide (e6aa14d)
- Update release docs for v0.2.0 (1582be8)


### Tests

- Cover dashboard HTTP guardrails (1b0986f)
- Cover dashboard status detail rendering (ffd4ba5)
- Cover dashboard status parity (7c562c2)


### CI

- Add release distribution automation (2a74603)
- Add Dependabot and refresh automation pins (8bcab76)
- Verify release artifact checksums (cbeb78b)
- Lock Rust components for mise install (6693690)
- Bump actions/checkout in the github-actions group (3077044)
- Catch Linux-only cfg guard mistakes locally (6f0e296)
- Bump the github-actions group across 1 directory with 2 updates (6dd0a51)


### Dependencies

- Bump the cargo-dependencies group with 2 updates (c1e0852)
- Keep serde_json on approved parser pin (1710f8d)
- Bump serde_json in the cargo-dependencies group across 1 directory (386b3c2)
- Keep serde_json on ryu-backed version (9cf7c9a)

## v0.1.0 - 2026-06-13

[Compare changes](https://github.com/bindport/bindport/releases/tag/v0.1.0)

### Features

- Add initial Rust workspace scaffold (b13d48d)
- Add one-shot command runner (e7123ac)
- Add registry-backed status (8827a3b)
- Forward Unix signals to wrapped commands (9c5d556)
- Add config discovery with fallback config (ee1abad)
- Add git-backed service identity (5a05a26)
- Add sticky port allocation (6c27ca7)
- Expand doctor diagnostics (57fe210)
- Infer identity from package metadata (7fe0db5)
- Retry runner allocation after port races (d537f34)
- Prepare v0.1 runner readiness (5d77775)


### Fixes

- Prevent signal forwarding setup race (a4a0254)
- Correct CLI status and config paths (39fc416)


### Documentation

- Add initial README.md (dae6088)
- Add repository bootstrap guidance (aa32d9f)
- Document bootstrap project boundary (43d1ce9)
- Add release process notes (8ccb0c3)
- Clarify release workflow boundaries (5599d0e)


### CI

- Add bootstrap validation workflows (77cc567)
- Add filesystem security scan (6e3288b)
- Install Rust components in CI (26b26d9)
- Run dependency guard in PR checks (e6bb39d)
- Add macOS compatibility checks (abb7660)
- Add manual release prep checks (d01a249)
- Add manual release automation (a49a241)
- Use macOS mise checksum (4e0b0e8)
- Avoid mise install on macOS (c2c8f6d)


### Build

- Add npm wrapper skeleton (a841484)

