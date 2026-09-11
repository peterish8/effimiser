//! Stable handles to raw evidence.
//!
//! The runtime's central bargain: a tool may produce megabytes, the model sees
//! a small representation, and the full bytes stay retrievable forever through
//! a handle. If a handle cannot be minted, the compression must not happen.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The kind of raw evidence a handle points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleKind {
    /// Captured stdout/stderr of a command run.
    Output,
    /// Byte-exact snapshot of a file at a content hash.
    FileSnapshot,
    /// Raw response from an upstream MCP tool.
    Tool,
    /// Git plumbing output (diff, log, show).
    Git,
}

impl HandleKind {
    pub fn scheme(self) -> &'static str {
        match self {
            HandleKind::Output => "output",
            HandleKind::FileSnapshot => "file-snapshot",
            HandleKind::Tool => "tool",
            HandleKind::Git => "git",
        }
    }

    fn from_scheme(s: &str) -> Option<Self> {
        Some(match s {
            "output" => HandleKind::Output,
            "file-snapshot" => HandleKind::FileSnapshot,
            "tool" => HandleKind::Tool,
            "git" => HandleKind::Git,
            _ => return None,
        })
    }
}

/// An opaque, stable pointer to raw stored evidence, e.g.
/// `output://run/3f9a1c2b`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle {
    kind: HandleKind,
    /// Path portion after the scheme. Opaque to callers.
    path: String,
}

impl Handle {
    pub fn new(kind: HandleKind, path: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
        }
    }

    /// Mint a handle for a command run, keyed by a content hash prefix.
    pub fn output_run(id: &str) -> Self {
        Self::new(HandleKind::Output, format!("run/{id}"))
    }

    /// Mint a handle for a file snapshot at a specific content hash.
    pub fn file_snapshot(hash: &str) -> Self {
        Self::new(HandleKind::FileSnapshot, hash.to_string())
    }

    pub fn kind(&self) -> HandleKind {
        self.kind
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.kind.scheme(), self.path)
    }
}

/// Parse failure for a handle string.
#[derive(Debug, thiserror::Error)]
pub enum HandleParseError {
    #[error("handle is missing the `scheme://` separator: {0:?}")]
    MissingSeparator(String),
    #[error("unknown handle scheme: {0:?}")]
    UnknownScheme(String),
    #[error("handle has an empty path")]
    EmptyPath,
}

impl FromStr for Handle {
    type Err = HandleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (scheme, path) = s
            .split_once("://")
            .ok_or_else(|| HandleParseError::MissingSeparator(s.to_string()))?;
        let kind = HandleKind::from_scheme(scheme)
            .ok_or_else(|| HandleParseError::UnknownScheme(scheme.to_string()))?;
        if path.is_empty() {
            return Err(HandleParseError::EmptyPath);
        }
        Ok(Self::new(kind, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_display_and_parse() {
        let h = Handle::output_run("3f9a1c2b");
        assert_eq!(h.to_string(), "output://run/3f9a1c2b");
        assert_eq!(h.to_string().parse::<Handle>().unwrap(), h);
    }

    #[test]
    fn file_snapshot_round_trips() {
        let h = Handle::file_snapshot("abc921");
        assert_eq!(h.to_string(), "file-snapshot://abc921");
        assert_eq!(h.to_string().parse::<Handle>().unwrap(), h);
    }

    #[test]
    fn rejects_malformed() {
        assert!("output:/run/x".parse::<Handle>().is_err());
        assert!("nope://x".parse::<Handle>().is_err());
        assert!("output://".parse::<Handle>().is_err());
    }
}
