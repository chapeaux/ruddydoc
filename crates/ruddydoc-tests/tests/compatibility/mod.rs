//! Compatibility test suite for RuddyDoc vs Python docling.
//!
//! This test suite verifies that RuddyDoc produces structurally equivalent
//! output to Python docling across all supported formats.
//!
//! Run with: `cargo test --test compatibility`

mod export_validation;
mod helpers;
mod roundtrip;
mod schema;
mod sparql;

// Re-export helpers for use in sub-modules
pub use helpers::*;
