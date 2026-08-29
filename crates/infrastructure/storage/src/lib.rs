//! Transactional SQLite repositories for persistent SimShredder jobs.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use simshredder_domain::SourceKind;
use thiserror::Error;

const MIGRATION_1: &str = include_str!("migrations/0001_persistent_runner.sql");
pub const SCHEMA_VERSION: i64 = 1;
pub const MAX_LOG_BYTES_PER_STREAM: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum Error {
    #[error("SQLite storage failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("storage contract failed: {0}")]
    Contract(String),
    #[error("system clock is before the Unix epoch")]
    Clock,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistentState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Interrupted,
}

impl PersistentState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(Error::Contract(format!(
                "unknown persistent state: {value}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuPreset {
    Efficient,
    Balanced,
    Maximum,
}

impl CpuPreset {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Efficient => "efficient",
            Self::Balanced => "balanced",
            Self::Maximum => "maximum",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "efficient" => Ok(Self::Efficient),
            "balanced" => Ok(Self::Balanced),
            "maximum" => Ok(Self::Maximum),
            _ => Err(Error::Contract(format!("unknown CPU preset: {value}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl LogStream {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewJob {
    pub profile_id: i64,
    pub cpu_preset: CpuPreset,
    pub executable_path: PathBuf,
    pub runtime_revision: String,
    pub executable_sha256: String,
    pub simc_version: String,
    pub game_version: String,
    pub normalized_schema_version: u32,
    pub rule_revision: String,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug)]
pub struct NewBatch {
    pub source_kind: SourceKind,
    pub source_bytes: Vec<u8>,
    pub generated_bytes: Vec<u8>,
    pub generated_sha256: String,
    pub cache_key: String,
}

#[derive(Clone, Debug)]
pub struct ClaimedAttempt {
    pub attempt_id: i64,
    pub sequence: u32,
    pub batch_id: i64,
    pub batch_ordinal: u32,
    pub job_id: i64,
    pub source_kind: SourceKind,
    pub source_bytes: Vec<u8>,
    pub generated_bytes: Vec<u8>,
    pub cache_key: String,
    pub executable_path: PathBuf,
    pub runtime_revision: String,
    pub executable_sha256: String,
    pub simc_version: String,
    pub game_version: String,
    pub normalized_schema_version: u32,
    pub rule_revision: String,
    pub cpu_preset: CpuPreset,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSnapshot {
    pub id: i64,
    pub state: PersistentState,
    pub cpu_preset: CpuPreset,
    pub cancel_requested: bool,
    pub failure: Option<String>,
    pub succeeded_batches: u32,
    pub pending_batches: u32,
    pub created_unix_millis: i64,
    pub updated_unix_millis: i64,
    pub profile_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptSnapshot {
    pub id: i64,
    pub batch_id: i64,
    pub sequence: u32,
    pub state: PersistentState,
    pub started_unix_millis: i64,
    pub finished_unix_millis: Option<i64>,
    pub failure: Option<String>,
    pub artifact_directory: Option<PathBuf>,
    pub cache_hit: bool,
    pub stdout_log_truncated: bool,
    pub stderr_log_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheRecord {
    pub cache_key: String,
    pub artifact_directory: PathBuf,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchArtifactRecord {
    pub ordinal: u32,
    pub state: PersistentState,
    pub artifact_directory: Option<PathBuf>,
    pub manifest_sha256: Option<String>,
}

pub struct Database {
    connection: Connection,
    path: Option<PathBuf>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::Contract(format!("failed to create database directory: {error}"))
            })?;
        }
        let connection = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| Error::Contract(format!("failed to protect the database file: {error}")),
            )?;
        }
        let mut database = Self {
            connection,
            path: Some(path.to_owned()),
        };
        database.configure()?;
        database.migrate()?;
        Ok(database)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        let mut database = Self {
            connection,
            path: None,
        };
        database.configure()?;
        database.migrate()?;
        Ok(database)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }

    fn configure(&mut self) -> Result<()> {
        self.connection.pragma_update(None, "foreign_keys", "ON")?;
        self.connection
            .pragma_update(None, "busy_timeout", 5_000_i64)?;
        if self.path.is_some() {
            self.connection.pragma_update(None, "journal_mode", "WAL")?;
            self.connection.pragma_update(None, "synchronous", "FULL")?;
        }
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        let has_migrations: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",
            [],
            |row| row.get(0),
        )?;
        if !has_migrations {
            let user_table_count: i64 = self.connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            if user_table_count != 0 {
                return Err(Error::Contract(
                    "unversioned non-empty database cannot be migrated safely".into(),
                ));
            }
            let now = now_millis()?;
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_1)?;
            transaction.execute(
                "INSERT INTO schema_migrations(version, applied_unix_millis) VALUES (?1, ?2)",
                params![SCHEMA_VERSION, now],
            )?;
            transaction.commit()?;
        }
        let version = self.schema_version()?;
        if version != SCHEMA_VERSION {
            return Err(Error::Contract(format!(
                "unsupported database schema version {version}; expected {SCHEMA_VERSION}"
            )));
        }
        Ok(())
    }

    pub fn insert_profile(
        &mut self,
        source_kind: SourceKind,
        source_bytes: &[u8],
        generated_bytes: &[u8],
        profile_json: &str,
    ) -> Result<i64> {
        let now = now_millis()?;
        self.connection.execute(
            "INSERT INTO profiles(source_kind, source_bytes, generated_bytes, profile_json, created_unix_millis) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![source_kind_token(source_kind), source_bytes, generated_bytes, profile_json, now],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn enqueue_job(&mut self, job: &NewJob, batches: &[NewBatch]) -> Result<i64> {
        if batches.is_empty() {
            return Err(Error::Contract(
                "a job must contain at least one batch".into(),
            ));
        }
        validate_digest(&job.executable_sha256)?;
        if job.rule_revision.is_empty()
            || job.timeout_millis == 0
            || job.timeout_millis > 86_400_000
        {
            return Err(Error::Contract("job runtime fields are invalid".into()));
        }
        let now = now_millis()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO jobs(profile_id, state, cpu_preset, executable_path, runtime_revision, executable_sha256, simc_version, game_version, normalized_schema_version, rule_revision, timeout_millis, created_unix_millis, updated_unix_millis) VALUES (?1, 'queued', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                job.profile_id,
                job.cpu_preset.as_str(),
                path_text(&job.executable_path)?,
                job.runtime_revision,
                job.executable_sha256,
                job.simc_version,
                job.game_version,
                i64::from(job.normalized_schema_version),
                job.rule_revision,
                i64::try_from(job.timeout_millis).map_err(|_| Error::Contract("timeout is too large".into()))?,
                now,
            ],
        )?;
        let job_id = transaction.last_insert_rowid();
        for (ordinal, batch) in batches.iter().enumerate() {
            validate_digest(&batch.generated_sha256)?;
            validate_digest(&batch.cache_key)?;
            transaction.execute(
                "INSERT INTO job_batches(job_id, ordinal, state, source_kind, source_bytes, generated_bytes, generated_sha256, cache_key, created_unix_millis, updated_unix_millis) VALUES (?1, ?2, 'queued', ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![job_id, i64::try_from(ordinal).map_err(|_| Error::Contract("too many batches".into()))?, source_kind_token(batch.source_kind), batch.source_bytes, batch.generated_bytes, batch.generated_sha256, batch.cache_key, now],
            )?;
        }
        transaction.commit()?;
        Ok(job_id)
    }

    pub fn claim_next_attempt(&mut self) -> Result<Option<ClaimedAttempt>> {
        let now = now_millis()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = transaction
            .query_row(
                "SELECT b.id, b.ordinal, b.job_id, b.source_kind, b.source_bytes, b.generated_bytes, b.cache_key, j.executable_path, j.runtime_revision, j.executable_sha256, j.simc_version, j.game_version, j.normalized_schema_version, j.rule_revision, j.cpu_preset, j.timeout_millis FROM job_batches b JOIN jobs j ON j.id=b.job_id WHERE b.state='queued' AND j.state IN ('queued','running') AND j.cancel_requested=0 ORDER BY j.id, b.ordinal LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?, row.get::<_, Vec<u8>>(4)?, row.get::<_, Vec<u8>>(5)?,
                        row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?, row.get::<_, String>(10)?, row.get::<_, String>(11)?,
                        row.get::<_, i64>(12)?, row.get::<_, String>(13)?, row.get::<_, String>(14)?,
                        row.get::<_, i64>(15)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            batch_id,
            ordinal,
            job_id,
            source_kind,
            source_bytes,
            generated_bytes,
            cache_key,
            executable_path,
            runtime_revision,
            executable_sha256,
            simc_version,
            game_version,
            normalized_schema,
            rule_revision,
            cpu_preset,
            timeout_millis,
        )) = row
        else {
            transaction.commit()?;
            return Ok(None);
        };
        let sequence: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM job_attempts WHERE batch_id=?1",
            [batch_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE jobs SET state='running', updated_unix_millis=?2 WHERE id=?1",
            params![job_id, now],
        )?;
        let changed = transaction.execute("UPDATE job_batches SET state='running', failure=NULL, updated_unix_millis=?2 WHERE id=?1 AND state='queued'", params![batch_id, now])?;
        if changed != 1 {
            return Err(Error::Contract(
                "batch claim lost its state transition".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO job_attempts(batch_id, sequence, state, started_unix_millis) VALUES (?1, ?2, 'running', ?3)",
            params![batch_id, sequence, now],
        )?;
        let attempt_id = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(Some(ClaimedAttempt {
            attempt_id,
            sequence: u32::try_from(sequence)
                .map_err(|_| Error::Contract("attempt sequence overflow".into()))?,
            batch_id,
            batch_ordinal: u32::try_from(ordinal)
                .map_err(|_| Error::Contract("batch ordinal overflow".into()))?,
            job_id,
            source_kind: parse_source_kind(&source_kind)?,
            source_bytes,
            generated_bytes,
            cache_key,
            executable_path: PathBuf::from(executable_path),
            runtime_revision,
            executable_sha256,
            simc_version,
            game_version,
            normalized_schema_version: u32::try_from(normalized_schema)
                .map_err(|_| Error::Contract("normalized schema overflow".into()))?,
            rule_revision,
            cpu_preset: CpuPreset::parse(&cpu_preset)?,
            timeout_millis: u64::try_from(timeout_millis)
                .map_err(|_| Error::Contract("negative timeout".into()))?,
        }))
    }

    pub fn complete_attempt(
        &mut self,
        attempt_id: i64,
        artifact_directory: &Path,
        manifest_sha256: &str,
        cache_hit: bool,
    ) -> Result<()> {
        validate_digest(manifest_sha256)?;
        let now = now_millis()?;
        let artifact = path_text(artifact_directory)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (batch_id, job_id, cache_key): (i64, i64, String) = transaction.query_row(
            "SELECT a.batch_id, b.job_id, b.cache_key FROM job_attempts a JOIN job_batches b ON b.id=a.batch_id WHERE a.id=?1 AND a.state='running'",
            [attempt_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        transaction.execute(
            "UPDATE job_attempts SET state='succeeded', finished_unix_millis=?2, artifact_directory=?3, cache_hit=?4 WHERE id=?1",
            params![attempt_id, now, artifact, i64::from(cache_hit)],
        )?;
        transaction.execute(
            "UPDATE job_batches SET state='succeeded', artifact_directory=?2, manifest_sha256=?3, cache_hit=?4, failure=NULL, updated_unix_millis=?5 WHERE id=?1",
            params![batch_id, artifact, manifest_sha256, i64::from(cache_hit), now],
        )?;
        transaction.execute(
            "INSERT INTO cache_entries(cache_key, artifact_directory, manifest_sha256, created_unix_millis, verified_unix_millis) VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(cache_key) DO UPDATE SET artifact_directory=excluded.artifact_directory, manifest_sha256=excluded.manifest_sha256, verified_unix_millis=excluded.verified_unix_millis",
            params![cache_key, artifact, manifest_sha256, now],
        )?;
        let remaining: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM job_batches WHERE job_id=?1 AND state!='succeeded'",
            [job_id],
            |row| row.get(0),
        )?;
        let cancel_requested: i64 = transaction.query_row(
            "SELECT cancel_requested FROM jobs WHERE id=?1",
            [job_id],
            |row| row.get(0),
        )?;
        let (job_state, job_failure, clear_cancel) = if remaining == 0 {
            ("succeeded", None, 1_i64)
        } else if cancel_requested != 0 {
            ("canceled", Some("cancellation requested"), 0_i64)
        } else {
            ("running", None, 0_i64)
        };
        transaction.execute(
            "UPDATE jobs SET state=?2, failure=?3, cancel_requested=CASE WHEN ?4=1 THEN 0 ELSE cancel_requested END, updated_unix_millis=?5 WHERE id=?1",
            params![job_id, job_state, job_failure, clear_cancel, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn fail_attempt(
        &mut self,
        attempt_id: i64,
        state: PersistentState,
        failure: &str,
        artifact_directory: Option<&Path>,
    ) -> Result<()> {
        if !matches!(state, PersistentState::Failed | PersistentState::Canceled) {
            return Err(Error::Contract(
                "attempt can only fail or be canceled".into(),
            ));
        }
        let now = now_millis()?;
        let artifact = artifact_directory.map(path_text).transpose()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (batch_id, job_id): (i64, i64) = transaction.query_row(
            "SELECT a.batch_id, b.job_id FROM job_attempts a JOIN job_batches b ON b.id=a.batch_id WHERE a.id=?1 AND a.state='running'",
            [attempt_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        transaction.execute(
            "UPDATE job_attempts SET state=?2, finished_unix_millis=?3, failure=?4, artifact_directory=?5 WHERE id=?1",
            params![attempt_id, state.as_str(), now, failure, artifact],
        )?;
        transaction.execute(
            "UPDATE job_batches SET state=?2, failure=?3, artifact_directory=?4, updated_unix_millis=?5 WHERE id=?1",
            params![batch_id, state.as_str(), failure, artifact, now],
        )?;
        transaction.execute(
            "UPDATE jobs SET state=?2, failure=?3, updated_unix_millis=?4 WHERE id=?1",
            params![job_id, state.as_str(), failure, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn request_cancel(&mut self, job_id: i64) -> Result<()> {
        let now = now_millis()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let running: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM job_batches WHERE job_id=?1 AND state='running'",
            [job_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE jobs SET cancel_requested=1, state=?2, failure='cancellation requested', updated_unix_millis=?3 WHERE id=?1 AND state NOT IN ('succeeded','canceled')",
            params![job_id, if running == 0 { "canceled" } else { "running" }, now],
        )?;
        transaction.execute(
            "UPDATE job_batches SET state='canceled', failure='canceled before execution', updated_unix_millis=?2 WHERE job_id=?1 AND state='queued'",
            params![job_id, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn retry_job(&mut self, job_id: i64) -> Result<()> {
        let now = now_millis()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String =
            transaction.query_row("SELECT state FROM jobs WHERE id=?1", [job_id], |row| {
                row.get(0)
            })?;
        if !matches!(state.as_str(), "failed" | "canceled" | "interrupted") {
            return Err(Error::Contract(format!(
                "job {job_id} cannot be retried from {state}"
            )));
        }
        transaction.execute(
            "UPDATE job_batches SET state='queued', failure=NULL, artifact_directory=NULL, manifest_sha256=NULL, cache_hit=0, updated_unix_millis=?2 WHERE job_id=?1 AND state!='succeeded'",
            params![job_id, now],
        )?;
        transaction.execute(
            "UPDATE jobs SET state='queued', cancel_requested=0, failure=NULL, updated_unix_millis=?2 WHERE id=?1",
            params![job_id, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recover_interrupted(&mut self) -> Result<Vec<i64>> {
        let now = now_millis()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job_ids = {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT b.job_id FROM job_attempts a JOIN job_batches b ON b.id=a.batch_id WHERE a.state='running' ORDER BY b.job_id",
            )?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<std::result::Result<Vec<i64>, _>>()?
        };
        transaction.execute(
            "UPDATE job_attempts SET state='interrupted', finished_unix_millis=?1, failure='application stopped while attempt was running' WHERE state='running'",
            [now],
        )?;
        transaction.execute(
            "UPDATE job_batches SET state='queued', failure='previous attempt was interrupted', updated_unix_millis=?1 WHERE state='running'",
            [now],
        )?;
        for job_id in &job_ids {
            transaction.execute(
                "UPDATE jobs SET state='interrupted', failure='application stopped with an active attempt', updated_unix_millis=?2 WHERE id=?1",
                params![job_id, now],
            )?;
        }
        transaction.commit()?;
        Ok(job_ids)
    }

    pub fn resume_interrupted(&mut self, job_id: i64) -> Result<()> {
        let now = now_millis()?;
        let changed = self.connection.execute(
            "UPDATE jobs SET state='queued', cancel_requested=0, updated_unix_millis=?2 WHERE id=?1 AND state='interrupted'",
            params![job_id, now],
        )?;
        if changed != 1 {
            return Err(Error::Contract(format!("job {job_id} is not interrupted")));
        }
        Ok(())
    }

    pub fn job(&self, job_id: i64) -> Result<JobSnapshot> {
        self.connection.query_row(
            "SELECT j.id, j.state, j.cpu_preset, j.cancel_requested, j.failure, SUM(CASE WHEN b.state='succeeded' THEN 1 ELSE 0 END), SUM(CASE WHEN b.state!='succeeded' THEN 1 ELSE 0 END), j.created_unix_millis, j.updated_unix_millis, p.profile_json FROM jobs j JOIN job_batches b ON b.job_id=j.id JOIN profiles p ON p.id=j.profile_id WHERE j.id=?1 GROUP BY j.id",
            [job_id],
            |row| {
                let state: String = row.get(1)?;
                Ok((row.get(0)?, state, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get(4)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?, row.get::<_, i64>(8)?, row.get::<_, String>(9)?))
            },
        ).map_err(Error::from).and_then(|(id, state, cpu_preset, cancel, failure, succeeded, pending, created, updated, profile_json)| Ok(JobSnapshot {
            id,
            state: PersistentState::parse(&state)?,
            cpu_preset: CpuPreset::parse(&cpu_preset)?,
            cancel_requested: cancel != 0,
            failure,
            succeeded_batches: u32::try_from(succeeded).map_err(|_| Error::Contract("batch count overflow".into()))?,
            pending_batches: u32::try_from(pending).map_err(|_| Error::Contract("batch count overflow".into()))?,
            created_unix_millis: created,
            updated_unix_millis: updated,
            profile_json,
        }))
    }

    pub fn recent_jobs(&self, limit: usize) -> Result<Vec<JobSnapshot>> {
        if limit == 0 || limit > 500 {
            return Err(Error::Contract(
                "recent job limit must be between 1 and 500".into(),
            ));
        }
        let mut statement = self.connection.prepare(
            "SELECT j.id, j.state, j.cpu_preset, j.cancel_requested, j.failure, SUM(CASE WHEN b.state='succeeded' THEN 1 ELSE 0 END), SUM(CASE WHEN b.state!='succeeded' THEN 1 ELSE 0 END), j.created_unix_millis, j.updated_unix_millis, p.profile_json FROM jobs j JOIN job_batches b ON b.job_id=j.id JOIN profiles p ON p.id=j.profile_id GROUP BY j.id ORDER BY j.id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(
            [i64::try_from(limit)
                .map_err(|_| Error::Contract("recent job limit is too large".into()))?],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )?;
        let mut jobs = Vec::new();
        for row in rows {
            let (
                id,
                state,
                cpu_preset,
                cancel,
                failure,
                succeeded,
                pending,
                created,
                updated,
                profile_json,
            ) = row?;
            jobs.push(JobSnapshot {
                id,
                state: PersistentState::parse(&state)?,
                cpu_preset: CpuPreset::parse(&cpu_preset)?,
                cancel_requested: cancel != 0,
                failure,
                succeeded_batches: u32::try_from(succeeded)
                    .map_err(|_| Error::Contract("batch count overflow".into()))?,
                pending_batches: u32::try_from(pending)
                    .map_err(|_| Error::Contract("batch count overflow".into()))?,
                created_unix_millis: created,
                updated_unix_millis: updated,
                profile_json,
            });
        }
        Ok(jobs)
    }

    pub fn clone_terminal_job(&mut self, job_id: i64) -> Result<i64> {
        let now = now_millis()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String = transaction
            .query_row("SELECT state FROM jobs WHERE id=?1", [job_id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::Contract(format!("job {job_id} does not exist")))?;
        if matches!(
            PersistentState::parse(&state)?,
            PersistentState::Queued | PersistentState::Running
        ) {
            return Err(Error::Contract(format!(
                "job {job_id} is active and cannot be rerun"
            )));
        }
        transaction.execute(
            "INSERT INTO jobs(profile_id, state, cpu_preset, executable_path, runtime_revision, executable_sha256, simc_version, game_version, normalized_schema_version, rule_revision, timeout_millis, created_unix_millis, updated_unix_millis) SELECT profile_id, 'queued', cpu_preset, executable_path, runtime_revision, executable_sha256, simc_version, game_version, normalized_schema_version, rule_revision, timeout_millis, ?2, ?2 FROM jobs WHERE id=?1",
            params![job_id, now],
        )?;
        let rerun_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO job_batches(job_id, ordinal, state, source_kind, source_bytes, generated_bytes, generated_sha256, cache_key, created_unix_millis, updated_unix_millis) SELECT ?2, ordinal, 'queued', source_kind, source_bytes, generated_bytes, generated_sha256, cache_key, ?3, ?3 FROM job_batches WHERE job_id=?1 ORDER BY ordinal",
            params![job_id, rerun_id, now],
        )?;
        transaction.commit()?;
        Ok(rerun_id)
    }

    pub fn delete_terminal_jobs(&mut self, job_ids: &[i64]) -> Result<()> {
        if job_ids.is_empty() || job_ids.len() > 16 {
            return Err(Error::Contract(
                "job deletion requires between 1 and 16 jobs".into(),
            ));
        }
        let mut unique = job_ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != job_ids.len() || unique.iter().any(|id| *id <= 0) {
            return Err(Error::Contract(
                "job deletion requires unique positive identifiers".into(),
            ));
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut profile_ids = Vec::with_capacity(unique.len());
        for job_id in &unique {
            let (state, profile_id): (String, i64) = transaction
                .query_row(
                    "SELECT state, profile_id FROM jobs WHERE id=?1",
                    [job_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::Contract(format!("job {job_id} does not exist")))?;
            if matches!(
                PersistentState::parse(&state)?,
                PersistentState::Queued | PersistentState::Running
            ) {
                return Err(Error::Contract(format!(
                    "job {job_id} is active and cannot be deleted"
                )));
            }
            profile_ids.push(profile_id);
            if transaction.execute("DELETE FROM jobs WHERE id=?1", [job_id])? != 1 {
                return Err(Error::Contract(format!("job {job_id} was not deleted")));
            }
        }
        for profile_id in profile_ids {
            transaction.execute(
                "DELETE FROM profiles WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM jobs WHERE profile_id=?1)",
                [profile_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn artifact_directories_excluding_jobs(
        &self,
        excluded_job_ids: &[i64],
    ) -> Result<Vec<PathBuf>> {
        let mut statement = self.connection.prepare(
            "SELECT job_id, artifact_directory FROM job_batches WHERE artifact_directory IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut directories = Vec::new();
        for row in rows {
            let (job_id, directory) = row?;
            if !excluded_job_ids.contains(&job_id) {
                directories.push(PathBuf::from(directory));
            }
        }
        Ok(directories)
    }

    pub fn attempts_for_job(&self, job_id: i64) -> Result<Vec<AttemptSnapshot>> {
        let mut statement = self.connection.prepare(
            "SELECT a.id, a.batch_id, a.sequence, a.state, a.started_unix_millis, a.finished_unix_millis, a.failure, a.artifact_directory, a.cache_hit, a.stdout_log_truncated, a.stderr_log_truncated FROM job_attempts a JOIN job_batches b ON b.id=a.batch_id WHERE b.job_id=?1 ORDER BY b.ordinal, a.sequence",
        )?;
        let rows = statement.query_map([job_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;
        let mut attempts = Vec::new();
        for row in rows {
            let (
                id,
                batch_id,
                sequence,
                state,
                started_unix_millis,
                finished_unix_millis,
                failure,
                artifact,
                cache_hit,
                stdout_log_truncated,
                stderr_log_truncated,
            ) = row?;
            attempts.push(AttemptSnapshot {
                id,
                batch_id,
                sequence: u32::try_from(sequence)
                    .map_err(|_| Error::Contract("attempt sequence overflow".into()))?,
                state: PersistentState::parse(&state)?,
                started_unix_millis,
                finished_unix_millis,
                failure,
                artifact_directory: artifact.map(PathBuf::from),
                cache_hit: cache_hit != 0,
                stdout_log_truncated: stdout_log_truncated != 0,
                stderr_log_truncated: stderr_log_truncated != 0,
            });
        }
        Ok(attempts)
    }

    pub fn running_attempts(&self) -> Result<Vec<AttemptSnapshot>> {
        let mut statement = self.connection.prepare(
            "SELECT id, batch_id, sequence, state, started_unix_millis, finished_unix_millis, failure, artifact_directory, cache_hit, stdout_log_truncated, stderr_log_truncated FROM job_attempts WHERE state='running' ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;
        let mut attempts = Vec::new();
        for row in rows {
            let (
                id,
                batch_id,
                sequence,
                state,
                started_unix_millis,
                finished_unix_millis,
                failure,
                artifact,
                cache_hit,
                stdout_log_truncated,
                stderr_log_truncated,
            ) = row?;
            attempts.push(AttemptSnapshot {
                id,
                batch_id,
                sequence: u32::try_from(sequence)
                    .map_err(|_| Error::Contract("attempt sequence overflow".into()))?,
                state: PersistentState::parse(&state)?,
                started_unix_millis,
                finished_unix_millis,
                failure,
                artifact_directory: artifact.map(PathBuf::from),
                cache_hit: cache_hit != 0,
                stdout_log_truncated: stdout_log_truncated != 0,
                stderr_log_truncated: stderr_log_truncated != 0,
            });
        }
        Ok(attempts)
    }

    pub fn set_attempt_artifact_directory(
        &mut self,
        attempt_id: i64,
        artifact_directory: &Path,
    ) -> Result<()> {
        let artifact = path_text(artifact_directory)?;
        let changed = self.connection.execute(
            "UPDATE job_attempts SET artifact_directory=?2 WHERE id=?1 AND state IN ('running','interrupted')",
            params![attempt_id, artifact],
        )?;
        if changed != 1 {
            return Err(Error::Contract(format!(
                "attempt {attempt_id} cannot accept an artifact path"
            )));
        }
        Ok(())
    }

    pub fn append_log(&mut self, attempt_id: i64, stream: LogStream, bytes: &[u8]) -> Result<bool> {
        if bytes.is_empty() {
            return Ok(false);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let retained: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(length(bytes)), 0) FROM attempt_logs WHERE attempt_id=?1 AND stream=?2",
            params![attempt_id, stream.as_str()],
            |row| row.get(0),
        )?;
        let retained =
            usize::try_from(retained).map_err(|_| Error::Contract("negative log size".into()))?;
        let available = MAX_LOG_BYTES_PER_STREAM.saturating_sub(retained);
        let count = available.min(bytes.len());
        if count > 0 {
            let sequence: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(sequence), -1) + 1 FROM attempt_logs WHERE attempt_id=?1 AND stream=?2",
                params![attempt_id, stream.as_str()],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT INTO attempt_logs(attempt_id, stream, sequence, bytes) VALUES (?1, ?2, ?3, ?4)",
                params![attempt_id, stream.as_str(), sequence, &bytes[..count]],
            )?;
        }
        if count != bytes.len() {
            let statement = match stream {
                LogStream::Stdout => "UPDATE job_attempts SET stdout_log_truncated=1 WHERE id=?1",
                LogStream::Stderr => "UPDATE job_attempts SET stderr_log_truncated=1 WHERE id=?1",
            };
            transaction.execute(statement, [attempt_id])?;
        }
        transaction.commit()?;
        Ok(count != bytes.len())
    }

    pub fn read_log(&self, attempt_id: i64, stream: LogStream) -> Result<Vec<u8>> {
        let mut statement = self.connection.prepare(
            "SELECT bytes FROM attempt_logs WHERE attempt_id=?1 AND stream=?2 ORDER BY sequence",
        )?;
        let chunks = statement.query_map(params![attempt_id, stream.as_str()], |row| {
            row.get::<_, Vec<u8>>(0)
        })?;
        let mut output = Vec::new();
        for chunk in chunks {
            let chunk = chunk?;
            let available = MAX_LOG_BYTES_PER_STREAM.saturating_sub(output.len());
            output.extend_from_slice(&chunk[..available.min(chunk.len())]);
            if output.len() == MAX_LOG_BYTES_PER_STREAM {
                break;
            }
        }
        Ok(output)
    }

    pub fn cache_entry(&self, cache_key: &str) -> Result<Option<CacheRecord>> {
        validate_digest(cache_key)?;
        self.connection.query_row(
            "SELECT cache_key, artifact_directory, manifest_sha256 FROM cache_entries WHERE cache_key=?1",
            [cache_key],
            |row| Ok(CacheRecord { cache_key: row.get(0)?, artifact_directory: PathBuf::from(row.get::<_, String>(1)?), manifest_sha256: row.get(2)? }),
        ).optional().map_err(Error::from)
    }

    pub fn batch_artifacts(&self, job_id: i64) -> Result<Vec<BatchArtifactRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT ordinal, state, artifact_directory, manifest_sha256 FROM job_batches WHERE job_id=?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map([job_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (ordinal, state, directory, manifest_sha256) = row?;
            records.push(BatchArtifactRecord {
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| Error::Contract("batch ordinal overflow".into()))?,
                state: PersistentState::parse(&state)?,
                artifact_directory: directory.map(PathBuf::from),
                manifest_sha256,
            });
        }
        Ok(records)
    }

    pub fn remove_cache_entry(&mut self, cache_key: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM cache_entries WHERE cache_key=?1", [cache_key])?;
        Ok(())
    }
}

fn now_millis() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Clock)?
        .as_millis();
    i64::try_from(millis).map_err(|_| Error::Contract("timestamp overflow".into()))
}

fn source_kind_token(source_kind: SourceKind) -> &'static str {
    match source_kind {
        SourceKind::AddonExport => "addon_export",
        SourceKind::SimcFile => "simc_file",
    }
}

fn parse_source_kind(value: &str) -> Result<SourceKind> {
    match value {
        "addon_export" => Ok(SourceKind::AddonExport),
        "simc_file" => Ok(SourceKind::SimcFile),
        _ => Err(Error::Contract(format!("unknown source kind: {value}"))),
    }
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::Contract("path must be valid UTF-8".into()))
}

fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Contract(
            "digest must be 64 hexadecimal characters".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(database: &mut Database) -> i64 {
        database
            .insert_profile(SourceKind::SimcFile, b"source", b"generated", "{}")
            .unwrap()
    }

    fn new_job(profile_id: i64) -> NewJob {
        NewJob {
            profile_id,
            cpu_preset: CpuPreset::Balanced,
            executable_path: PathBuf::from("/tmp/simc"),
            runtime_revision: "1234567".into(),
            executable_sha256: "a".repeat(64),
            simc_version: "1210-01".into(),
            game_version: "12.1.0.1".into(),
            normalized_schema_version: 1,
            rule_revision: "quick-v1".into(),
            timeout_millis: 60_000,
        }
    }

    fn batches(count: usize) -> Vec<NewBatch> {
        (0..count)
            .map(|index| NewBatch {
                source_kind: SourceKind::SimcFile,
                source_bytes: format!("source-{index}").into_bytes(),
                generated_bytes: format!("generated-{index}").into_bytes(),
                generated_sha256: format!("{:064x}", index + 1),
                cache_key: format!("{:064x}", index + 10),
            })
            .collect()
    }

    #[test]
    fn migration_is_idempotent_and_foreign_keys_are_enabled() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("state.sqlite3");
        let database = Database::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(database);
        let database = Database::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        let enabled: i64 = database
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(enabled, 1);
    }

    #[test]
    fn migration_rejects_an_unversioned_non_empty_database_without_modifying_it() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("unknown.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE legacy_sentinel(value TEXT); INSERT INTO legacy_sentinel VALUES ('preserve-me');")
            .unwrap();
        drop(connection);

        let error = match Database::open(&path) {
            Ok(_) => panic!("unversioned non-empty database was accepted"),
            Err(error) => error,
        };
        assert!(
            matches!(error, Error::Contract(message) if message.contains("unversioned non-empty"))
        );
        let connection = Connection::open(&path).unwrap();
        let value: String = connection
            .query_row("SELECT value FROM legacy_sentinel", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "preserve-me");
        let migration_table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_table, 0);
    }

    #[test]
    fn recent_jobs_are_bounded_and_newest_first() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let first = database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        let second = database
            .enqueue_job(&new_job(profile_id), &batches(2))
            .unwrap();

        let jobs = database.recent_jobs(10).unwrap();
        assert_eq!(
            jobs.iter().map(|job| job.id).collect::<Vec<_>>(),
            [second, first]
        );
        assert_eq!(jobs[0].pending_batches, 2);
        assert!(database.recent_jobs(0).is_err());
        assert!(database.recent_jobs(501).is_err());
    }

    #[test]
    fn terminal_job_deletion_cascades_history_and_rejects_active_work() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let job_id = database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        assert!(database.delete_terminal_jobs(&[job_id]).is_err());

        let attempt = database.claim_next_attempt().unwrap().unwrap();
        database
            .complete_attempt(
                attempt.attempt_id,
                Path::new("/tmp/deleted-artifact"),
                &"f".repeat(64),
                false,
            )
            .unwrap();
        database.delete_terminal_jobs(&[job_id]).unwrap();

        assert!(database.job(job_id).is_err());
        assert!(database.attempts_for_job(job_id).unwrap().is_empty());
        let remaining_profiles: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining_profiles, 0);
    }

    #[test]
    fn deletion_preserves_cache_and_detects_artifacts_referenced_by_other_jobs() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let first = database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        let second = database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        for _ in 0..2 {
            let attempt = database.claim_next_attempt().unwrap().unwrap();
            database
                .complete_attempt(
                    attempt.attempt_id,
                    Path::new("/tmp/shared-artifact"),
                    &"f".repeat(64),
                    false,
                )
                .unwrap();
        }

        assert_eq!(
            database
                .artifact_directories_excluding_jobs(&[first])
                .unwrap(),
            [PathBuf::from("/tmp/shared-artifact")]
        );
        database.delete_terminal_jobs(&[first]).unwrap();
        assert_eq!(
            database.job(second).unwrap().state,
            PersistentState::Succeeded
        );
        assert!(
            database
                .cache_entry(&format!("{:064x}", 10))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn interrupted_attempt_is_diagnosable_and_only_pending_batches_resume() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let job_id = database
            .enqueue_job(&new_job(profile_id), &batches(3))
            .unwrap();
        let first = database.claim_next_attempt().unwrap().unwrap();
        database
            .complete_attempt(
                first.attempt_id,
                Path::new("/tmp/first"),
                &"f".repeat(64),
                false,
            )
            .unwrap();
        let second = database.claim_next_attempt().unwrap().unwrap();
        assert_eq!(second.batch_ordinal, 1);

        assert_eq!(database.recover_interrupted().unwrap(), vec![job_id]);
        assert_eq!(
            database.job(job_id).unwrap().state,
            PersistentState::Interrupted
        );
        let attempts = database.attempts_for_job(job_id).unwrap();
        assert_eq!(attempts[0].state, PersistentState::Succeeded);
        assert_eq!(attempts[1].state, PersistentState::Interrupted);
        assert!(attempts[1].failure.as_deref().unwrap().contains("stopped"));

        database.resume_interrupted(job_id).unwrap();
        let resumed = database.claim_next_attempt().unwrap().unwrap();
        assert_eq!(resumed.batch_ordinal, 1);
        assert_eq!(resumed.sequence, 2);
    }

    #[test]
    fn log_repository_is_bounded() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        let attempt = database.claim_next_attempt().unwrap().unwrap();
        assert!(
            database
                .append_log(
                    attempt.attempt_id,
                    LogStream::Stdout,
                    &vec![b'x'; MAX_LOG_BYTES_PER_STREAM + 5]
                )
                .unwrap()
        );
        assert_eq!(
            database
                .read_log(attempt.attempt_id, LogStream::Stdout)
                .unwrap()
                .len(),
            MAX_LOG_BYTES_PER_STREAM
        );
        let snapshot = database.attempts_for_job(attempt.job_id).unwrap();
        assert!(snapshot[0].stdout_log_truncated);
        assert!(!snapshot[0].stderr_log_truncated);
    }

    #[test]
    fn cancellation_race_preserves_completed_batch_and_retry_queues_only_remaining() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let job_id = database
            .enqueue_job(&new_job(profile_id), &batches(2))
            .unwrap();
        let first = database.claim_next_attempt().unwrap().unwrap();
        database.request_cancel(job_id).unwrap();
        database
            .complete_attempt(
                first.attempt_id,
                Path::new("/tmp/completed-during-cancel"),
                &"f".repeat(64),
                false,
            )
            .unwrap();
        let canceled = database.job(job_id).unwrap();
        assert_eq!(canceled.state, PersistentState::Canceled);
        assert_eq!(canceled.succeeded_batches, 1);
        assert_eq!(canceled.pending_batches, 1);
        database.retry_job(job_id).unwrap();
        let remaining = database.claim_next_attempt().unwrap().unwrap();
        assert_eq!(remaining.batch_ordinal, 1);
    }

    #[test]
    fn terminal_job_can_be_cloned_without_mutating_original_history() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let job_id = database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        let attempt = database.claim_next_attempt().unwrap().unwrap();
        database
            .complete_attempt(
                attempt.attempt_id,
                Path::new("/tmp/original-artifact"),
                &"a".repeat(64),
                false,
            )
            .unwrap();

        let rerun_id = database.clone_terminal_job(job_id).unwrap();
        let original = database.job(job_id).unwrap();
        let rerun = database.job(rerun_id).unwrap();

        assert_eq!(original.state, PersistentState::Succeeded);
        assert_eq!(rerun.state, PersistentState::Queued);
        assert_eq!(rerun.succeeded_batches, 0);
        assert_eq!(rerun.pending_batches, 1);
        assert!(database.attempts_for_job(rerun_id).unwrap().is_empty());
    }

    #[test]
    fn job_snapshots_expose_stable_identity_timing_and_cpu_metadata() {
        let mut database = Database::open_in_memory().unwrap();
        let profile_id = profile(&mut database);
        let job_id = database
            .enqueue_job(&new_job(profile_id), &batches(1))
            .unwrap();
        let snapshot = database.job(job_id).unwrap();

        assert_eq!(snapshot.cpu_preset, CpuPreset::Balanced);
        assert!(snapshot.created_unix_millis > 0);
        assert!(snapshot.updated_unix_millis >= snapshot.created_unix_millis);
        assert!(!snapshot.profile_json.is_empty());
    }

    #[test]
    fn sqlite_full_rolls_back_the_profile_insert() {
        let mut database = Database::open_in_memory().unwrap();
        let page_count: i64 = database
            .connection
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap();
        database
            .connection
            .pragma_update(None, "max_page_count", page_count)
            .unwrap();
        let result = database.insert_profile(
            SourceKind::SimcFile,
            &vec![b'x'; 2 * 1024 * 1024],
            b"generated",
            "{}",
        );
        assert!(result.is_err());
        let count: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
