//! Background management of remote/cloud hosts. Fetching a remote is slow
//! (an SSH round-trip), so it runs on its own cadence in a background thread and
//! caches the results; the main refresh loop merges the cached remote snapshots
//! into every local snapshot it pushes.
//!
//! A remote that fails its fetch does NOT silently vanish: its error is kept
//! and surfaced as a `RemoteDown` alert by the daemon.

use ccwatch_core::model::Snapshot;
use ccwatch_core::remote::{fetch_remote, RemoteDef, SystemRunner};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Per-remote fetch timeout (an ssh round-trip + probe run).
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

pub struct RemoteManager {
    defs: Arc<Vec<RemoteDef>>,
    cache: Arc<RwLock<Vec<Snapshot>>>,
    /// `(remote name, error)` for every remote whose *last* fetch failed.
    errors: Arc<RwLock<Vec<(String, String)>>>,
}

impl RemoteManager {
    pub fn new(defs: Vec<RemoteDef>) -> Self {
        RemoteManager {
            defs: Arc::new(defs),
            cache: Arc::new(RwLock::new(Vec::new())),
            errors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Shared cache of the latest successful remote snapshots.
    pub fn cache(&self) -> Arc<RwLock<Vec<Snapshot>>> {
        self.cache.clone()
    }

    /// Shared list of currently-failing remotes.
    pub fn errors(&self) -> Arc<RwLock<Vec<(String, String)>>> {
        self.errors.clone()
    }

    pub fn defs(&self) -> Arc<Vec<RemoteDef>> {
        self.defs.clone()
    }

    /// Fetch every remote once; successes replace the cache, failures are
    /// recorded for alerting.
    fn refresh_once(
        defs: &[RemoteDef],
        cache: &RwLock<Vec<Snapshot>>,
        errors: &RwLock<Vec<(String, String)>>,
    ) {
        let runner = SystemRunner;
        let mut snaps = Vec::new();
        let mut errs = Vec::new();
        for def in defs {
            match fetch_remote(def, &runner, FETCH_TIMEOUT) {
                Ok(s) => snaps.push(s),
                Err(e) => {
                    eprintln!("remote '{}' fetch failed: {e}", def.name);
                    errs.push((def.name.clone(), e.to_string()));
                }
            }
        }
        *cache.write().unwrap() = snaps;
        *errors.write().unwrap() = errs;
    }

    /// Start the background fetch loop (immediate first fetch, then every
    /// `interval`). No thread if there are no remotes.
    pub fn spawn(&self, interval: Duration) {
        if self.defs.is_empty() {
            return;
        }
        let defs = self.defs.clone();
        let cache = self.cache.clone();
        let errors = self.errors.clone();
        std::thread::spawn(move || loop {
            RemoteManager::refresh_once(&defs, &cache, &errors);
            std::thread::sleep(interval);
        });
    }
}
