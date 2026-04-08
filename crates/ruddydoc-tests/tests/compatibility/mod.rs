//! Compatibility test suite for RuddyDoc vs Python docling.
//!
//! This test suite verifies that RuddyDoc produces structurally equivalent
//! output to Python docling across all supported formats.
//!
//! Run with: `cargo test --test compatibility`

mod helpers;
mod roundtrip;
mod export_validation;
mod sparql;
mod schema;

// Re-export helpers for use in sub-modules
pub use helpers::*;
