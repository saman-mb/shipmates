//! `shipmates status` / `shipmates upgrade` support: release lookup, channel
//! detection, install indexing, and the upgrade orchestration built on them.

pub mod audit;
pub mod classify;
pub mod file;
pub mod index;
pub mod orchestrate;
pub mod release;
pub mod types;
