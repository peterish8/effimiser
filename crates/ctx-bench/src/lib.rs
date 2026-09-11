//! ctx-bench — the measurement harness.
//!
//! This crate exists before the optimisations it will judge. Its design goals
//! are narrow and deliberate:
//!
//! - Report the **distribution**, never the best run. `min` is recorded but a
//!   summary that showed only `min` would be a lie, so [`Stats`] always carries
//!   p50/p95 alongside it.
//! - Record **cold and warm separately**, labelled, because hiding cold-start
//!   cost is one of the easier ways to fake a win.
//! - Carry a full **reproducibility record** into every artifact, so a number
//!   with no provenance cannot be emitted by construction.
//! - Detect a **planted regression**. A harness that cannot fail a deliberately
//!   worsened treatment cannot validate a genuine improvement, so that property
//!   is unit-tested in this crate.

pub mod harness;
pub mod stats;

pub use harness::{Environment, Measurement, Thermal};
pub use stats::{compare, Comparison, Stats, Verdict};
