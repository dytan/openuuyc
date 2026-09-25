//! Historical Linux clipboard-format stub from the early cfg split.
//!
//! Live Linux format registration lives in `formats.rs` (`#[cfg(not(windows))]`
//! process-local CF_* registry + MIME helpers). This file is intentionally
//! unused so Windows/Linux module layout stays discoverable without diverging
//! from the PR1 `formats.rs` source of truth.
