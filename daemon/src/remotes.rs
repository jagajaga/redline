//! Background management of remote/cloud hosts. Fetching a remote is slow
//! (an SSH round-trip), so it runs on its own cadence in a background thread and
//! caches the results; the main refresh loop merges the cached remote snapshots
//! into every local snapshot it pushes.

use ccwatch_core::model::Snapshot;
use ccwatch_core::remote::{fetch_remote, RemoteDef, SystemRunner};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Per-remote fetch timeout.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

pub struct RemoteManager {
    defs: Arc<Vec<RemoteDef>>,
    cache: Arc<RwLock<Vec<Snapshot>>>,
}

impl RemoteManager {
    pub fn new(defs: Vec<RemoteDef>) -> Self {
        RemoteManager {
            defs: Arc::new(defs),
            cache: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Shared cache of the latest successful remote snapshots.
    pub fn cache(&self) -> Arc<RwLock<Vec<Snapshot>>> {
        self.cache.clone()
    }

    pub fn defs(&self) -> Arc<Vec<RemoteDef>> {
        self.defs.clone()
    }

    /// Fetch every remote once, replacing the cache with the successes. A
    /// remote that errors this cycle simply drops out until it recovers.
    fn refresh_once(defs: &[RemoteDef], cache: &RwLock<Vec<Snapshot>>) {
        let runner = SystemRunner;
        let mut snaps = Vec::new();
        for def in defs {
            match fetch_remote(def, &runner, FETCH_TIMEOUT) {
                Ok(s) => snaps.push(s),
                Err(e) => eprintln!("remote '{}' fetch failed: {e}", def.name),
            }
        }
        *cache.write().unwrap() = snaps;
    }

    /// Start the background fetch loop (immediate first fetch, then every
    /// `interval`). No-op thread if there are no remotes.
    pub fn spawn(&self, interval: Duration) {
        if self.defs.is_empty() {
            return;
        }
        let defs = self.defs.clone();
        let cache = self.cache.clone();
        std::thread::spawn(move || loop {
            RemoteManager::refresh_once(&defs, &cache);
            std::thread::sleep(interval);
        });
    }
}
