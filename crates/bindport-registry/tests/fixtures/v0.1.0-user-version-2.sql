-- Registry schema shipped by BindPort v0.1.0 (PRAGMA user_version = 2).
CREATE TABLE leases (
    id INTEGER PRIMARY KEY,
    project TEXT NOT NULL,
    service TEXT NOT NULL,
    worktree_path TEXT,
    worktree_hash TEXT,
    git_common_dir TEXT,
    branch TEXT,
    branch_label TEXT,
    git_commit TEXT,
    identity_key TEXT,
    port INTEGER NOT NULL,
    host TEXT NOT NULL,
    state TEXT NOT NULL,
    allocated_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    released_at TEXT
);
CREATE INDEX leases_state_port_idx ON leases(state, port);
CREATE INDEX leases_identity_key_idx ON leases(identity_key);

CREATE TABLE runs (
    id INTEGER PRIMARY KEY,
    lease_id INTEGER NOT NULL REFERENCES leases(id),
    pid INTEGER NOT NULL,
    command TEXT NOT NULL,
    cwd TEXT NOT NULL,
    started_at TEXT NOT NULL,
    exited_at TEXT,
    exit_code INTEGER
);
CREATE INDEX runs_lease_id_idx ON runs(lease_id);

INSERT INTO leases VALUES (
    101, 'migration-project', 'active',
    __ROOT__ || '/active', 'active-hash', __ROOT__ || '/.git',
    'feature/migrations', 'feature-migrations', 'abcdef1',
    'v1:migration-active', 29101, '127.0.0.1', 'active',
    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL
);
INSERT INTO leases VALUES (
    102, 'migration-project', 'stopped',
    __ROOT__ || '/stopped', 'stopped-hash', __ROOT__ || '/.git',
    'main', 'main', 'abcdef2', 'v1:migration-stopped',
    29102, '127.0.0.1', 'stopped',
    '2026-01-01T00:01:00Z', '2026-01-01T00:02:00Z',
    '2026-01-01T00:02:00Z'
);

INSERT INTO runs VALUES (
    201, 101, __TEST_PID__, __TEST_COMMAND__, __ROOT__ || '/active',
    '2026-01-01T00:00:00Z', NULL, NULL
);
INSERT INTO runs VALUES (
    202, 102, __TEST_PID__, 'finished migration fixture',
    __ROOT__ || '/stopped', '2026-01-01T00:01:00Z',
    '2026-01-01T00:02:00Z', 0
);

PRAGMA user_version = 2;
