//! TUI daemon client — a thin shim over [`ccwatch_core::client`]. The TUI wants
//! heartbeats and disconnects (to show liveness and retry), so it re-exports the
//! full [`FromDaemon`] stream.

pub use ccwatch_core::client::{ensure_daemon, send_action, subscribe, FromDaemon};
