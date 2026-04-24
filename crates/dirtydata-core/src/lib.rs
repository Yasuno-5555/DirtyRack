//! dirtydata-core — Canonical IR / Patch Engine.
//!
//! The heart of DirtyData. Everything else is projection.
//!
//! # Modules
//!
//! - [`types`] — Shared type vocabulary
//! - [`ir`] — Canonical IR (Node, Edge, Graph)
//! - [`patch`] — Patch Engine (apply, diff, merge, replay)
//! - [`hash`] — BLAKE3 deterministic hashing
//! - [`storage`] — Filesystem persistence
//! - [`validate`] — Commit validation
//! - [`actions`] — User-facing action schema
//! - [`dsl`] — Surface DSL export (Layer 2)

pub mod actions;
pub mod constitution;
pub mod dsl;
pub mod graph_utils;
pub mod hash;
pub mod ir;
pub mod patch;
pub mod storage;
pub mod types;
pub mod validate;

// Re-exports for convenience
pub use ir::{Edge, Graph, Node};
pub use patch::{Operation, Patch, PatchError, PatchSet};
pub use types::*;
pub use validate::{validate_commit, ValidationReport};
