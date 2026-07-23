# Configuration Stability

BindPort v0.8 treats the current configuration shape as the candidate for the
v1 stability freeze. It is ready to freeze, but it is not described as v1-final
until the v1 release makes that promise.

The machine-readable contract is
[`config.schema.json`](../config.schema.json). It covers the known keys and
value kinds accepted by project config, user fallback config, and machine-local
overrides. TOML remains the reference syntax; JSON and YAML deserialize into the
same hand-written Rust model. YAML keeps its existing 256 KiB limit and rejects
anchors and aliases.

`default_range` is the only supported port-range config key. It is an inclusive
`START-END` range whose start must be at least 1; port 0 is not a usable
assignment. Unknown top-level keys are ignored and reported by config
diagnostics. Unknown nested keys retain their current behavior of being
ignored. The schema permits additional
properties to represent that loader behavior; its named properties are the
stable-candidate public shape.

## Current Deprecations

No config keys are currently deprecated. Therefore `bindport config validate`
does not emit deprecation warnings for any key. It continues to report unknown
top-level keys as unknown, not deprecated.

BindPort has no automatic config rewrite or migration behavior. There is no
active pre-1.0 warning or removal window because there is no approved
pre-1.0 deprecation.

## Before The v1 Freeze

A pre-1.0 minor release can still change config behavior, but a key is not
considered deprecated merely because a replacement is proposed. Before any
future deprecation is implemented, its approved contract must document:

- the exact deprecated key path and replacement;
- the release that starts warning and the release that may remove it;
- runtime compatibility during that warning window;
- behavior when old and replacement keys are both present; and
- the release-note and `config validate` diagnostic text.

Only keys designated through that process receive actionable deprecation
warnings. The current designated set is empty. BindPort will not invent aliases
or precedence rules from unknown keys.

## After The v1 Freeze

Backward-compatible additive changes are allowed after the freeze. Existing v1
config must continue to load with the same meaning.

Removing or renaming a key, narrowing an accepted type or value set, or changing
an existing key's meaning requires deprecation first and removal through an
announced major version. The deprecation must keep the old form working for its
documented warning window, identify the exact replacement in `config validate`,
define old/new conflict behavior, and name the earliest removal version. Release
notes must carry the same migration instructions. BindPort does not promise to
edit users' config files automatically.
