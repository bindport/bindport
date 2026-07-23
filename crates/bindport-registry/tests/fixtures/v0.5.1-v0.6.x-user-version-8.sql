-- Schema shipped by v0.5.1 and v0.6.0-v0.6.2 (user_version = 8).
-- The reserved row represents the reservation state shipped in v0.6.0.
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
    hostname TEXT,
    route_url TEXT,
    health_url TEXT,
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
    process_start_time INTEGER,
    command TEXT NOT NULL,
    cwd TEXT NOT NULL,
    started_at TEXT NOT NULL,
    exited_at TEXT,
    exit_code INTEGER
);
CREATE INDEX runs_lease_id_idx ON runs(lease_id);
CREATE TABLE output_files (
    id INTEGER PRIMARY KEY,
    output_name TEXT NOT NULL,
    route_key TEXT NOT NULL,
    rendered_path TEXT NOT NULL,
    status TEXT NOT NULL,
    reason TEXT,
    content_hash TEXT,
    template_hash TEXT,
    lease_id INTEGER,
    run_id INTEGER,
    rendered_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(output_name, route_key)
);
CREATE INDEX output_files_output_path_idx ON output_files(output_name, rendered_path);
CREATE INDEX output_files_route_key_idx ON output_files(route_key);
CREATE TABLE output_render_state (
    output_name TEXT PRIMARY KEY,
    last_render_at_ms INTEGER NOT NULL
);

INSERT INTO leases VALUES (
    101, 'migration-project', 'active', __ROOT__ || '/active',
    'active-hash', __ROOT__ || '/.git', 'feature/migrations',
    'feature-migrations', 'abcdef1', 'v1:migration-active', 29101,
    '127.0.0.1', 'active.localhost', 'http://active.localhost',
    'https://active.localhost/health', 'active',
    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL
);
INSERT INTO leases VALUES (
    102, 'migration-project', 'stopped', __ROOT__ || '/stopped',
    'stopped-hash', __ROOT__ || '/.git', 'main', 'main', 'abcdef2',
    'v1:migration-stopped', 29102, '127.0.0.1', 'stopped.localhost',
    'http://stopped.localhost', 'https://stopped.localhost/health', 'stopped',
    '2026-01-01T00:01:00Z', '2026-01-01T00:02:00Z',
    '2026-01-01T00:02:00Z'
);
INSERT INTO leases VALUES (
    103, 'migration-project', 'reserved', __ROOT__ || '/reserved',
    'reserved-hash', __ROOT__ || '/.git', 'feature/reserved',
    'feature-reserved', 'abcdef3', 'v1:migration-reserved', 29103,
    '127.0.0.1', 'reserved.localhost', 'http://reserved.localhost',
    'https://reserved.localhost/health', 'reserved',
    '2026-01-01T00:05:00Z', '2026-01-01T00:05:00Z', NULL
);
INSERT INTO runs VALUES (
    201, 101, __TEST_PID__, NULL, __TEST_COMMAND__, __ROOT__ || '/active',
    '2026-01-01T00:00:00Z', NULL, NULL
);
INSERT INTO runs VALUES (
    202, 102, __TEST_PID__, 4242, 'finished migration fixture',
    __ROOT__ || '/stopped', '2026-01-01T00:01:00Z',
    '2026-01-01T00:02:00Z', 0
);
INSERT INTO output_files VALUES (
    301, 'traefik', 'v1:migration-active',
    __ROOT__ || '/outputs/active.yml', 'rendered', NULL, 'active-content',
    'template-v1', 101, 201, '2026-01-01T00:03:00Z',
    '2026-01-01T00:03:00Z'
);
INSERT INTO output_files VALUES (
    302, 'caddy', 'v1:migration-stopped',
    __ROOT__ || '/outputs/stopped.caddy', 'error', 'external_modified',
    'stopped-content', 'template-v1', 102, 202,
    '2026-01-01T00:04:00Z', '2026-01-01T00:04:00Z'
);
INSERT INTO output_render_state VALUES ('traefik', 1767225780000);
PRAGMA user_version = 8;
