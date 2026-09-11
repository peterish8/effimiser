//! ctx-store — the local raw store.
//!
//! Everything the runtime withholds from the model is kept here, byte-exact,
//! behind a stable handle. The store is the reason compression is progressive
//! disclosure rather than data loss.
//!
//! Layout under `.ctx/`:
//! - `ctx.sqlite`   metadata: blobs, runs, snapshots
//! - `blobs/aa/bb…` content-addressed raw bytes (blake3)

pub mod blobs;
pub mod store;

pub use store::{BlobMeta, Store};
