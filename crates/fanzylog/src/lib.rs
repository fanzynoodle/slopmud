//! fanzylog
//!
//! Internal facade for our consensus/log engine.
//!
//! This crate intentionally re-exports the underlying implementation so
//! consumers depend only on `fanzylog`. That gives us an easy swap point
//! when we move to the in-house zero-copy implementation.

pub use openraft::*;
