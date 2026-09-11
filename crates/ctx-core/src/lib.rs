//! ctx-core — shared types, honest token accounting, and provenance primitives
//! for the Context Runtime.
//!
//! Design rule enforced here: a "token" number that reaches a user or a
//! benchmark artifact must come from a real tokenizer, never from a
//! bytes/4 heuristic. See [`tokens`].

pub mod handle;
pub mod provenance;
pub mod tokens;

pub use handle::Handle;
pub use provenance::{Provenance, RetrievalMethod};
pub use tokens::{TokenCount, TokenCounter, Tokenizer};
