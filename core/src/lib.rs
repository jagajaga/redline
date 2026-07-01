//! `ccwatch-core` — the engine behind the Claude Code observability dashboard.
//!
//! It reads local Claude Code state (`~/.claude/…`), accounts for token usage
//! incrementally, detects resource/token leaks, and exposes a [`model::Snapshot`]
//! plus [`ipc`] message types and [`actions`] the daemon executes. No terminal
//! or long-running loop lives here — that belongs to the daemon and clients.

pub mod actions;
pub mod collectors;
pub mod config;
pub mod engine;
pub mod governor;
pub mod ipc;
pub mod leaks;
pub mod model;
pub mod paths;
pub mod remote;

pub use config::Config;
pub use engine::Engine;
pub use model::*;
pub use paths::Paths;
