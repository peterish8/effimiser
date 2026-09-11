//! The metadata store: SQLite for indexed facts, blobs for raw bytes.
//!
//! Two invariants this module exists to uphold:
//!
//! 1. Anything withheld from the model is retrievable byte-exactly later.
//! 2. A file the agent has already seen is recorded, so an unchanged re-read
//!    can be answered with a marker instead of the whole file again.

use anyhow::{Context, Result};
use ctx_core::handle::{Handle, HandleKind};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

use crate::blobs;

/// Schema version. Bumping this without a migration is a bug; `open` refuses
/// to run against a newer schema than it understands.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Raw bytes we captured, addressed by blake3 hash.
CREATE TABLE IF NOT EXISTS blobs (
    hash       TEXT PRIMARY KEY,
    byte_len   INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

-- One row per captured command execution.
CREATE TABLE IF NOT EXISTS runs (
    id           TEXT PRIMARY KEY,
    command      TEXT NOT NULL,
    cwd          TEXT NOT NULL,
    exit_code    INTEGER,
    stdout_hash  TEXT REFERENCES blobs(hash),
    stderr_hash  TEXT REFERENCES blobs(hash),
    duration_ms  INTEGER NOT NULL,
    created_at   INTEGER NOT NULL
);

-- What content of which path the agent has already been shown.
-- Keyed by (path, content_hash) so history is retained across edits.
CREATE TABLE IF NOT EXISTS snapshots (
    path         TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    byte_len     INTEGER NOT NULL,
    seen_at      INTEGER NOT NULL,
    PRIMARY KEY (path, content_hash)
);

CREATE INDEX IF NOT EXISTS idx_snapshots_path_seen
    ON snapshots(path, seen_at DESC);
";

/// Recorded facts about a stored blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMeta {
    pub hash: String,
    pub byte_len: u64,
}

/// A captured command run, as the model would be told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub handle: Handle,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout_len: u64,
    pub stderr_len: u64,
    pub duration_ms: u64,
}

/// The local store, rooted at a `.ctx/` directory inside the repository.
pub struct Store {
    conn: Connection,
    blob_root: PathBuf,
}

impl Store {
    /// Open (creating if absent) a store under `root/.ctx`.
    pub fn open(root: &Path) -> Result<Self> {
        let ctx_dir = root.join(".ctx");
        let blob_root = ctx_dir.join("blobs");
        std::fs::create_dir_all(&blob_root)
            .with_context(|| format!("creating store at {}", ctx_dir.display()))?;

        let conn = Connection::open(ctx_dir.join("ctx.sqlite"))
            .with_context(|| format!("opening database in {}", ctx_dir.display()))?;

        // WAL keeps readers from blocking the indexer; NORMAL sync is the
        // right trade for a cache we can always rebuild from the repo.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;

        let store = Self { conn, blob_root };
        store.check_schema_version()?;
        Ok(store)
    }

    fn check_schema_version(&self) -> Result<()> {
        let found: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match found {
            None => {
                self.conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )?;
                Ok(())
            }
            Some(v) => {
                let v: i64 = v.parse().context("schema_version is not an integer")?;
                anyhow::ensure!(
                    v <= SCHEMA_VERSION,
                    "store schema v{v} is newer than this binary understands \
                     (v{SCHEMA_VERSION}); upgrade ctx"
                );
                Ok(())
            }
        }
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Store raw bytes and register them. Returns the content hash.
    pub fn put_blob(&self, bytes: &[u8]) -> Result<String> {
        let hash = blobs::put(&self.blob_root, bytes)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO blobs (hash, byte_len, created_at) VALUES (?1, ?2, ?3)",
            params![hash, bytes.len() as i64, Self::now()],
        )?;
        Ok(hash)
    }

    /// Read raw bytes back by hash.
    pub fn get_blob(&self, hash: &str) -> Result<Vec<u8>> {
        blobs::get(&self.blob_root, hash)
    }

    pub fn blob_meta(&self, hash: &str) -> Result<Option<BlobMeta>> {
        Ok(self
            .conn
            .query_row(
                "SELECT hash, byte_len FROM blobs WHERE hash = ?1",
                params![hash],
                |r| {
                    Ok(BlobMeta {
                        hash: r.get(0)?,
                        byte_len: r.get::<_, i64>(1)? as u64,
                    })
                },
            )
            .optional()?)
    }

    /// Capture a command execution. stdout and stderr are stored in full; the
    /// returned handle is what a compressed summary will point at.
    pub fn put_run(
        &self,
        command: &str,
        cwd: &str,
        exit_code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
        duration_ms: u64,
    ) -> Result<Handle> {
        let stdout_hash = self.put_blob(stdout)?;
        let stderr_hash = self.put_blob(stderr)?;

        // Run id derives from the command, its output hashes and a counter, so
        // it is stable, collision-resistant, and distinct for repeat runs of
        // the same command with identical output.
        let seq: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
            .unwrap_or(0);
        let mut h = blake3::Hasher::new();
        h.update(command.as_bytes());
        h.update(stdout_hash.as_bytes());
        h.update(stderr_hash.as_bytes());
        h.update(&seq.to_le_bytes());
        let id = h.finalize().to_hex()[..12].to_string();

        self.conn.execute(
            "INSERT OR REPLACE INTO runs
             (id, command, cwd, exit_code, stdout_hash, stderr_hash, duration_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                command,
                cwd,
                exit_code,
                stdout_hash,
                stderr_hash,
                duration_ms as i64,
                Self::now()
            ],
        )?;
        Ok(Handle::output_run(&id))
    }

    /// Look up a captured run by its handle.
    pub fn get_run(&self, handle: &Handle) -> Result<Option<RunRecord>> {
        let id = run_id(handle)?;
        let row = self
            .conn
            .query_row(
                "SELECT r.command, r.exit_code, r.duration_ms,
                        COALESCE(so.byte_len, 0), COALESCE(se.byte_len, 0)
                 FROM runs r
                 LEFT JOIN blobs so ON so.hash = r.stdout_hash
                 LEFT JOIN blobs se ON se.hash = r.stderr_hash
                 WHERE r.id = ?1",
                params![id],
                |r| {
                    Ok(RunRecord {
                        handle: handle.clone(),
                        command: r.get(0)?,
                        exit_code: r.get(1)?,
                        duration_ms: r.get::<_, i64>(2)? as u64,
                        stdout_len: r.get::<_, i64>(3)? as u64,
                        stderr_len: r.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Full captured output for a run: `(stdout, stderr)`, byte-exact.
    pub fn run_output(&self, handle: &Handle) -> Result<(Vec<u8>, Vec<u8>)> {
        let id = run_id(handle)?;
        let (so, se): (Option<String>, Option<String>) = self
            .conn
            .query_row(
                "SELECT stdout_hash, stderr_hash FROM runs WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .with_context(|| format!("no such run: {handle}"))?;

        let stdout = match so {
            Some(h) => self.get_blob(&h)?,
            None => Vec::new(),
        };
        let stderr = match se {
            Some(h) => self.get_blob(&h)?,
            None => Vec::new(),
        };
        Ok((stdout, stderr))
    }

    /// Record that the agent has been shown this exact content of `path`.
    pub fn record_snapshot(&self, path: &str, bytes: &[u8]) -> Result<String> {
        let hash = self.put_blob(bytes)?;
        self.conn.execute(
            "INSERT INTO snapshots (path, content_hash, byte_len, seen_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path, content_hash) DO UPDATE SET seen_at = excluded.seen_at",
            params![path, hash, bytes.len() as i64, Self::now()],
        )?;
        Ok(hash)
    }

    /// The content hash most recently shown for `path`, if any. This is what
    /// makes an "unchanged" answer possible instead of a full re-read.
    pub fn last_seen_hash(&self, path: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT content_hash FROM snapshots
                 WHERE path = ?1 ORDER BY seen_at DESC, rowid DESC LIMIT 1",
                params![path],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Count of distinct contents recorded for a path.
    pub fn snapshot_count(&self, path: &str) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM snapshots WHERE path = ?1",
            params![path],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    /// Total bytes of raw evidence held locally. Reported by `ctx stats` as
    /// the counterpart to "bytes withheld from the model".
    pub fn total_raw_bytes(&self) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(byte_len), 0) FROM blobs",
            [],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }
}

/// Extract the run id from an `output://run/<id>` handle.
fn run_id(handle: &Handle) -> Result<String> {
    anyhow::ensure!(
        handle.kind() == HandleKind::Output,
        "expected an output:// handle, got {handle}"
    );
    handle
        .path()
        .strip_prefix("run/")
        .map(|s| s.to_string())
        .with_context(|| format!("malformed run handle: {handle}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        (dir, s)
    }

    #[test]
    fn captured_run_is_recoverable_byte_exactly() {
        let (_d, s) = store();
        let stdout = b"Tests: 428\nPassed: 421\nFailed: 7\n".repeat(500);
        let stderr = b"warning: unused import\n".to_vec();
        let h = s
            .put_run("npm test", "/repo", Some(1), &stdout, &stderr, 8421)
            .unwrap();

        let (got_out, got_err) = s.run_output(&h).unwrap();
        assert_eq!(got_out, stdout, "stdout must survive byte-exactly");
        assert_eq!(got_err, stderr, "stderr must survive byte-exactly");

        let rec = s.get_run(&h).unwrap().expect("run should be recorded");
        assert_eq!(rec.command, "npm test");
        assert_eq!(rec.exit_code, Some(1));
        assert_eq!(rec.stdout_len, stdout.len() as u64);
        assert_eq!(rec.duration_ms, 8421);
    }

    #[test]
    fn handle_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let h = {
            let s = Store::open(dir.path()).unwrap();
            s.put_run("cargo build", "/repo", Some(0), b"ok", b"", 10)
                .unwrap()
        };
        // Simulate a new session: fresh connection, same root.
        let s2 = Store::open(dir.path()).unwrap();
        assert_eq!(s2.run_output(&h).unwrap().0, b"ok");
    }

    #[test]
    fn unchanged_file_is_detectable_without_rereading() {
        let (_d, s) = store();
        let content = b"fn validate() {}\n";
        let first = s.record_snapshot("src/auth.rs", content).unwrap();
        let again = s.last_seen_hash("src/auth.rs").unwrap().unwrap();
        assert_eq!(first, again, "same content must yield the same hash");
        assert_eq!(
            s.snapshot_count("src/auth.rs").unwrap(),
            1,
            "re-seeing identical content must not create a second snapshot row"
        );
    }

    #[test]
    fn changed_file_yields_a_new_hash_and_keeps_the_old() {
        let (_d, s) = store();
        let old = s.record_snapshot("src/auth.rs", b"v1").unwrap();
        let new = s.record_snapshot("src/auth.rs", b"v2").unwrap();
        assert_ne!(old, new);
        assert_eq!(s.last_seen_hash("src/auth.rs").unwrap().unwrap(), new);
        assert_eq!(s.snapshot_count("src/auth.rs").unwrap(), 2);
        // The previous content is still recoverable for delta computation.
        assert_eq!(s.get_blob(&old).unwrap(), b"v1");
    }

    #[test]
    fn unseen_path_has_no_hash() {
        let (_d, s) = store();
        assert!(s.last_seen_hash("never/seen.rs").unwrap().is_none());
    }

    #[test]
    fn identical_output_from_two_runs_stores_one_copy() {
        let (_d, s) = store();
        let out = b"same output".repeat(100);
        s.put_run("a", "/r", Some(0), &out, b"", 1).unwrap();
        s.put_run("b", "/r", Some(0), &out, b"", 1).unwrap();
        let n: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM blobs", [], |r| r.get(0))
            .unwrap();
        // stdout (shared) + empty stderr (shared) = 2 distinct blobs.
        assert_eq!(n, 2, "content addressing must deduplicate identical output");
    }

    #[test]
    fn repeat_run_with_identical_output_gets_a_distinct_handle() {
        let (_d, s) = store();
        let h1 = s.put_run("npm test", "/r", Some(0), b"same", b"", 1).unwrap();
        let h2 = s.put_run("npm test", "/r", Some(0), b"same", b"", 1).unwrap();
        assert_ne!(
            h1, h2,
            "two separate executions must be addressable separately"
        );
        assert!(s.get_run(&h1).unwrap().is_some());
        assert!(s.get_run(&h2).unwrap().is_some());
    }

    #[test]
    fn empty_output_is_recoverable_as_empty_not_missing() {
        let (_d, s) = store();
        let h = s.put_run("true", "/r", Some(0), b"", b"", 1).unwrap();
        let (o, e) = s.run_output(&h).unwrap();
        assert!(o.is_empty() && e.is_empty());
        assert!(s.get_run(&h).unwrap().is_some());
    }

    #[test]
    fn unknown_run_handle_errors_rather_than_returning_empty() {
        let (_d, s) = store();
        let bogus = Handle::output_run("deadbeef");
        assert!(s.get_run(&bogus).unwrap().is_none());
        assert!(s.run_output(&bogus).is_err());
    }

    #[test]
    fn rejects_a_non_output_handle() {
        let (_d, s) = store();
        let h = Handle::file_snapshot("abc");
        assert!(s.run_output(&h).is_err());
    }

    #[test]
    fn binary_output_survives() {
        let (_d, s) = store();
        let bin: Vec<u8> = (0u8..=255).cycle().take(9000).collect();
        let h = s.put_run("dump", "/r", Some(0), &bin, b"", 1).unwrap();
        assert_eq!(s.run_output(&h).unwrap().0, bin);
    }

    #[test]
    fn total_raw_bytes_accounts_for_what_we_withheld() {
        let (_d, s) = store();
        s.put_run("a", "/r", Some(0), &b"x".repeat(1000), b"", 1)
            .unwrap();
        assert_eq!(s.total_raw_bytes().unwrap(), 1000);
    }
}
