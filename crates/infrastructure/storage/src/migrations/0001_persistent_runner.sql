CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_unix_millis INTEGER NOT NULL
) STRICT;

CREATE TABLE profiles (
    id INTEGER PRIMARY KEY,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('addon_export', 'simc_file')),
    source_bytes BLOB NOT NULL CHECK (length(source_bytes) BETWEEN 1 AND 2097152),
    generated_bytes BLOB NOT NULL CHECK (length(generated_bytes) BETWEEN 1 AND 2097152),
    profile_json TEXT NOT NULL CHECK (json_valid(profile_json)),
    created_unix_millis INTEGER NOT NULL
) STRICT;

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE RESTRICT,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'canceled', 'interrupted')),
    cpu_preset TEXT NOT NULL CHECK (cpu_preset IN ('efficient', 'balanced', 'maximum')),
    executable_path TEXT NOT NULL,
    runtime_revision TEXT NOT NULL,
    executable_sha256 TEXT NOT NULL CHECK (length(executable_sha256) = 64),
    simc_version TEXT NOT NULL,
    game_version TEXT NOT NULL,
    normalized_schema_version INTEGER NOT NULL CHECK (normalized_schema_version > 0),
    rule_revision TEXT NOT NULL,
    timeout_millis INTEGER NOT NULL CHECK (timeout_millis BETWEEN 1 AND 86400000),
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    failure TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL
) STRICT;

CREATE TABLE job_batches (
    id INTEGER PRIMARY KEY,
    job_id INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'canceled', 'interrupted')),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('addon_export', 'simc_file')),
    source_bytes BLOB NOT NULL CHECK (length(source_bytes) BETWEEN 1 AND 2097152),
    generated_bytes BLOB NOT NULL CHECK (length(generated_bytes) BETWEEN 1 AND 2097152),
    generated_sha256 TEXT NOT NULL CHECK (length(generated_sha256) = 64),
    cache_key TEXT NOT NULL CHECK (length(cache_key) = 64),
    artifact_directory TEXT,
    manifest_sha256 TEXT,
    cache_hit INTEGER NOT NULL DEFAULT 0 CHECK (cache_hit IN (0, 1)),
    failure TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    UNIQUE(job_id, ordinal)
) STRICT;

CREATE TABLE job_attempts (
    id INTEGER PRIMARY KEY,
    batch_id INTEGER NOT NULL REFERENCES job_batches(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    state TEXT NOT NULL CHECK (state IN ('running', 'succeeded', 'failed', 'canceled', 'interrupted')),
    started_unix_millis INTEGER NOT NULL,
    finished_unix_millis INTEGER,
    failure TEXT,
    artifact_directory TEXT,
    cache_hit INTEGER NOT NULL DEFAULT 0 CHECK (cache_hit IN (0, 1)),
    stdout_log_truncated INTEGER NOT NULL DEFAULT 0 CHECK (stdout_log_truncated IN (0, 1)),
    stderr_log_truncated INTEGER NOT NULL DEFAULT 0 CHECK (stderr_log_truncated IN (0, 1)),
    UNIQUE(batch_id, sequence)
) STRICT;

CREATE TABLE attempt_logs (
    attempt_id INTEGER NOT NULL REFERENCES job_attempts(id) ON DELETE CASCADE,
    stream TEXT NOT NULL CHECK (stream IN ('stdout', 'stderr')),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    bytes BLOB NOT NULL CHECK (length(bytes) > 0),
    PRIMARY KEY(attempt_id, stream, sequence)
) STRICT;

CREATE TABLE cache_entries (
    cache_key TEXT PRIMARY KEY CHECK (length(cache_key) = 64),
    artifact_directory TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64),
    created_unix_millis INTEGER NOT NULL,
    verified_unix_millis INTEGER NOT NULL
) STRICT;

CREATE INDEX job_batches_dispatch_idx ON job_batches(state, job_id, ordinal);
CREATE INDEX job_attempts_batch_idx ON job_attempts(batch_id, sequence);
CREATE INDEX jobs_state_idx ON jobs(state, id);
