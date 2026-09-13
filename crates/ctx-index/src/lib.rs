//! ctx-index — an incremental symbol index.
//!
//! Phase 3's job is to answer "where is X defined" without the agent reading
//! files or scanning the repository. Two properties matter and both are
//! measured rather than asserted:
//!
//! 1. **Incremental.** Editing one file must reparse one file. The check is a
//!    blake3 content hash per file, so an unchanged file costs a hash and
//!    nothing else. `IndexStats::files_parsed` reports the truth for a
//!    benchmark to assert on.
//! 2. **Cheap to query.** Lookup is a single indexed SQLite query, so the
//!    answer does not depend on repository size the way a scan does.
//!
//! What this is not: a type-resolving index. Tree-sitter gives a syntax tree,
//! not name resolution, so `find_definition` answers "which definitions carry
//! this name" and may legitimately return more than one. Saying so here is
//! cheaper than having someone infer a guarantee that was never offered.

pub mod symbols;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub use symbols::{Symbol, SymbolKind};

const SCHEMA: &str = "
-- One row per indexed file. `content_hash` is what makes reindexing
-- incremental: an unchanged hash means the symbols below are still valid.
CREATE TABLE IF NOT EXISTS indexed_files (
    path         TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    byte_len     INTEGER NOT NULL,
    symbol_count INTEGER NOT NULL,
    indexed_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS symbols (
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    path       TEXT NOT NULL REFERENCES indexed_files(path) ON DELETE CASCADE,
    line       INTEGER NOT NULL,
    end_line   INTEGER NOT NULL,
    container  TEXT
);

-- The index that makes lookup independent of repository size. Without it,
-- every query degrades to a table scan and the whole exercise is pointless.
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
";

/// What one indexing pass actually did.
///
/// `files_parsed` versus `files_seen` is the incrementality claim, exposed so a
/// benchmark can assert on it instead of taking it on trust.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexStats {
    /// Files considered.
    pub files_seen: usize,
    /// Files whose content hash changed, so they were reparsed.
    pub files_parsed: usize,
    /// Files skipped because their content hash was unchanged.
    pub files_unchanged: usize,
    /// Files dropped from the index because they no longer exist.
    pub files_removed: usize,
    /// Symbols written during this pass.
    pub symbols_written: usize,
}

/// A definition site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    pub container: Option<String>,
}

/// The symbol index, stored beside the rest of the runtime's state.
pub struct Index {
    conn: Connection,
    root: PathBuf,
}

impl Index {
    /// Open (creating if absent) the index for a repository root.
    pub fn open(root: &Path) -> Result<Self> {
        let dir = root.join(".ctx");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let conn = Connection::open(dir.join("index.sqlite"))
            .with_context(|| format!("opening the index in {}", dir.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn,
            root: root.to_path_buf(),
        })
    }

    /// Open an index at an explicit database path. Used by benchmarks that
    /// index a repository they must not write into.
    pub fn open_at(db_path: &Path, root: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = Connection::open(db_path)
            .with_context(|| format!("opening the index at {}", db_path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn,
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Index every supported source file under the root, skipping files whose
    /// content hash is unchanged since the last pass.
    pub fn index_all(&mut self) -> Result<IndexStats> {
        let files = discover_sources(&self.root);
        self.index_files(&files)
    }

    /// Index an explicit file list. Same incremental rule as [`Self::index_all`].
    pub fn index_files(&mut self, files: &[PathBuf]) -> Result<IndexStats> {
        let mut stats = IndexStats {
            files_seen: files.len(),
            ..Default::default()
        };
        let mut parser = symbols::rust_parser()?;
        let now = now_secs();

        let tx = self.conn.transaction()?;
        for file in files {
            let rel = relative_path(&self.root, file);
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let hash = blake3::hash(&bytes).to_hex().to_string();

            let existing: Option<String> = tx
                .query_row(
                    "SELECT content_hash FROM indexed_files WHERE path = ?1",
                    params![rel],
                    |r| r.get(0),
                )
                .ok();

            if existing.as_deref() == Some(hash.as_str()) {
                stats.files_unchanged += 1;
                continue;
            }

            let text = String::from_utf8_lossy(&bytes);
            let found = symbols::extract_rust(&mut parser, &text)?;
            stats.files_parsed += 1;

            // Replace this file's rows wholesale. Cheaper and less error-prone
            // than diffing symbol sets, and the unit of change is the file.
            //
            // The parent row goes first: `symbols.path` is a foreign key into
            // `indexed_files`, so inserting symbols for a not-yet-recorded file
            // fails the constraint.
            tx.execute("DELETE FROM symbols WHERE path = ?1", params![rel])?;
            tx.execute(
                "INSERT INTO indexed_files (path, content_hash, byte_len, symbol_count, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET
                     content_hash = excluded.content_hash,
                     byte_len     = excluded.byte_len,
                     symbol_count = excluded.symbol_count,
                     indexed_at   = excluded.indexed_at",
                params![rel, hash, bytes.len() as i64, found.len() as i64, now],
            )?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO symbols (name, kind, path, line, end_line, container)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for s in &found {
                    stmt.execute(params![
                        s.name,
                        s.kind.as_str(),
                        rel,
                        s.line as i64,
                        s.end_line as i64,
                        s.container
                    ])?;
                }
            }
            stats.symbols_written += found.len();
        }

        // Drop files that have disappeared, so a deleted file cannot keep
        // answering lookups.
        let present: std::collections::HashSet<String> =
            files.iter().map(|f| relative_path(&self.root, f)).collect();
        let known: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM indexed_files")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        for path in known {
            if !present.contains(&path) {
                tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
                tx.execute("DELETE FROM indexed_files WHERE path = ?1", params![path])?;
                stats.files_removed += 1;
            }
        }

        tx.commit()?;
        Ok(stats)
    }

    /// Definitions carrying `name`.
    ///
    /// Exact match by design. Tree-sitter does not resolve names, so this
    /// answers "which definitions are called this", which is what the lookup
    /// benchmark measures and all the CLI promises.
    pub fn find_definition(&self, name: &str) -> Result<Vec<Definition>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT name, kind, path, line, end_line, container
             FROM symbols WHERE name = ?1
             ORDER BY path, line",
        )?;
        let rows = stmt.query_map(params![name], |r| {
            Ok(Definition {
                name: r.get(0)?,
                kind: SymbolKind::from_str(&r.get::<_, String>(1)?),
                path: r.get(2)?,
                line: r.get::<_, i64>(3)? as usize,
                end_line: r.get::<_, i64>(4)? as usize,
                container: r.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn symbol_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    pub fn file_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM indexed_files", [], |r| {
                r.get::<_, i64>(0)
            })? as u64)
    }
}

/// Source files we can index, honouring `.gitignore`.
pub fn discover_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build()
        .flatten()
    {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    out
}

fn relative_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn finds_definitions_of_each_kind() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "src/lib.rs",
            "pub fn alpha() {}\n\
             pub struct Beta { x: u32 }\n\
             enum Gamma { A, B }\n\
             pub trait Delta { fn m(&self); }\n\
             type Epsilon = u32;\n\
             const ZETA: u32 = 1;\n\
             static ETA: u32 = 2;\n\
             mod theta {}\n",
        );
        let mut idx = Index::open(tmp.path()).unwrap();
        idx.index_all().unwrap();

        for (name, kind) in [
            ("alpha", SymbolKind::Function),
            ("Beta", SymbolKind::Struct),
            ("Gamma", SymbolKind::Enum),
            ("Delta", SymbolKind::Trait),
            ("Epsilon", SymbolKind::TypeAlias),
            ("ZETA", SymbolKind::Const),
            ("ETA", SymbolKind::Static),
            ("theta", SymbolKind::Module),
        ] {
            let defs = idx.find_definition(name).unwrap();
            assert_eq!(defs.len(), 1, "expected exactly one {name}, got {defs:?}");
            assert_eq!(defs[0].kind, kind, "wrong kind for {name}");
            assert_eq!(defs[0].path, "src/lib.rs");
        }
    }

    #[test]
    fn unchanged_files_are_not_reparsed() {
        // The incrementality claim, asserted rather than assumed.
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..5 {
            write(
                tmp.path(),
                &format!("src/m{i}.rs"),
                &format!("pub fn f{i}() {{}}\n"),
            );
        }
        let mut idx = Index::open(tmp.path()).unwrap();
        let first = idx.index_all().unwrap();
        assert_eq!(first.files_parsed, 5);
        assert_eq!(first.files_unchanged, 0);

        let second = idx.index_all().unwrap();
        assert_eq!(
            second.files_parsed, 0,
            "nothing changed, so nothing should have been reparsed"
        );
        assert_eq!(second.files_unchanged, 5);
    }

    #[test]
    fn editing_one_file_reparses_exactly_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            write(
                tmp.path(),
                &format!("src/m{i}.rs"),
                &format!("pub fn f{i}() {{}}\n"),
            );
        }
        let mut idx = Index::open(tmp.path()).unwrap();
        idx.index_all().unwrap();

        write(tmp.path(), "src/m3.rs", "pub fn f3_renamed() {}\n");
        let stats = idx.index_all().unwrap();
        assert_eq!(stats.files_parsed, 1, "target 5: exactly one file reparsed");
        assert_eq!(stats.files_unchanged, 9);

        assert!(idx.find_definition("f3").unwrap().is_empty(), "stale symbol survived");
        assert_eq!(idx.find_definition("f3_renamed").unwrap().len(), 1);
    }

    #[test]
    fn deleted_files_stop_answering_lookups() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "src/a.rs", "pub fn gone() {}\n");
        write(tmp.path(), "src/b.rs", "pub fn stays() {}\n");
        let mut idx = Index::open(tmp.path()).unwrap();
        idx.index_all().unwrap();
        assert_eq!(idx.find_definition("gone").unwrap().len(), 1);

        std::fs::remove_file(tmp.path().join("src/a.rs")).unwrap();
        let stats = idx.index_all().unwrap();
        assert_eq!(stats.files_removed, 1);
        assert!(
            idx.find_definition("gone").unwrap().is_empty(),
            "a deleted file must not keep answering lookups"
        );
        assert_eq!(idx.find_definition("stays").unwrap().len(), 1);
    }

    #[test]
    fn same_name_in_two_files_returns_both() {
        // Honest about what it does: no name resolution, so both are returned.
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "src/a.rs", "pub fn shared() {}\n");
        write(tmp.path(), "src/b.rs", "pub fn shared() {}\n");
        let mut idx = Index::open(tmp.path()).unwrap();
        idx.index_all().unwrap();
        assert_eq!(idx.find_definition("shared").unwrap().len(), 2);
    }

    #[test]
    fn index_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "src/a.rs", "pub fn persisted() {}\n");
        {
            let mut idx = Index::open(tmp.path()).unwrap();
            idx.index_all().unwrap();
        }
        let idx = Index::open(tmp.path()).unwrap();
        assert_eq!(idx.find_definition("persisted").unwrap().len(), 1);
    }
}
