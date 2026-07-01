//! Collectors turn on-disk / OS state into typed data. Each is independently
//! fallible: a failure yields empty/stale data, never a panic.

pub mod hooks;
pub mod processes;
pub mod sessions;
pub mod tasks;
pub mod transcripts;
