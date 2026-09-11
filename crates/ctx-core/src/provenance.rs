//! Provenance attached to every piece of evidence the compiler emits.
//!
//! Without this, a context packet is an unfalsifiable claim. With it, any
//! packet line can be traced back to a file, a hash, and the retrieval method
//! that found it — which is what makes benchmark results auditable.

use crate::handle::Handle;
use serde::{Deserialize, Serialize};

/// How a piece of evidence was located. Recorded so benchmarks can report the
/// retrieval mix and prove that cheap methods ran before expensive ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMethod {
    /// Direct path lookup — cheapest.
    ExactPath,
    /// Symbol name lookup against the symbol index.
    SymbolIndex,
    /// Definition/reference graph traversal.
    ReferenceGraph,
    /// Tree-sitter structural pattern match.
    AstPattern,
    /// Trigram-accelerated literal search.
    Trigram,
    /// BM25 ranked lexical search.
    Bm25,
    /// Import/call dependency graph traversal.
    DependencyGraph,
    /// Embedding similarity — optional, and deliberately last.
    Semantic,
    /// Bounded raw scan fallback.
    RawScan,
}

impl RetrievalMethod {
    /// Rough cost tier, ascending. Used to assert in tests that the router
    /// tried cheap methods before expensive ones.
    pub fn cost_tier(self) -> u8 {
        match self {
            RetrievalMethod::ExactPath => 0,
            RetrievalMethod::SymbolIndex => 1,
            RetrievalMethod::ReferenceGraph => 2,
            RetrievalMethod::AstPattern => 3,
            RetrievalMethod::Trigram => 4,
            RetrievalMethod::Bm25 => 5,
            RetrievalMethod::DependencyGraph => 6,
            RetrievalMethod::Semantic => 7,
            RetrievalMethod::RawScan => 8,
        }
    }
}

/// Where a packet item came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Repository-relative path.
    pub path: String,
    /// Inclusive 1-based line range, when the evidence is a span.
    pub lines: Option<(u32, u32)>,
    /// Symbol name, when the evidence is a symbol.
    pub symbol: Option<String>,
    /// Content hash of the source at retrieval time, so staleness is detectable.
    pub content_hash: String,
    pub method: RetrievalMethod,
    /// Handle to the full raw evidence, when one exists.
    pub raw: Option<Handle>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_tiers_order_cheap_before_expensive() {
        assert!(
            RetrievalMethod::ExactPath.cost_tier() < RetrievalMethod::Semantic.cost_tier(),
            "exact path must be cheaper than semantic search"
        );
        assert!(
            RetrievalMethod::SymbolIndex.cost_tier() < RetrievalMethod::Bm25.cost_tier()
        );
        assert!(RetrievalMethod::Semantic.cost_tier() < RetrievalMethod::RawScan.cost_tier());
    }

    #[test]
    fn provenance_serialises_with_handle() {
        let p = Provenance {
            path: "src/auth/token.rs".into(),
            lines: Some((82, 123)),
            symbol: Some("validate_refresh_token".into()),
            content_hash: "abc921".into(),
            method: RetrievalMethod::SymbolIndex,
            raw: Some(Handle::file_snapshot("abc921")),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("symbol_index"));
        let back: Provenance = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
