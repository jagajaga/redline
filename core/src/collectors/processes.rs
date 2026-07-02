//! Live process resource sampling via `sysinfo`. Used to (a) confirm a session's
//! pid is actually alive and (b) read its CPU% and RSS.
//!
//! CPU percentage is only meaningful after two refreshes spaced by at least
//! `sysinfo`'s minimum interval, which the daemon's ~2s poll satisfies; the
//! first sample reads 0%.

use std::collections::HashMap;
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcStat {
    pub cpu_pct: f32,
    pub rss_mb: u64,
}

pub struct ProcessProbe {
    sys: System,
}

impl Default for ProcessProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessProbe {
    pub fn new() -> Self {
        ProcessProbe {
            sys: System::new(),
        }
    }

    /// Refresh process data. Call once per poll tick before querying.
    pub fn refresh(&mut self) {
        self.sys
            .refresh_processes(ProcessesToUpdate::All, true);
    }

    /// Refresh only the given pids — far cheaper than a full process-table
    /// scan when we already know which sessions we care about.
    pub fn refresh_pids(&mut self, pids: &[i32]) {
        let pids: Vec<Pid> = pids
            .iter()
            .filter(|p| **p > 0)
            .map(|p| Pid::from_u32(*p as u32))
            .collect();
        self.sys
            .refresh_processes(ProcessesToUpdate::Some(&pids), true);
    }

    /// Is this pid currently a live process?
    pub fn is_alive(&self, pid: i32) -> bool {
        pid > 0 && self.sys.process(Pid::from_u32(pid as u32)).is_some()
    }

    /// CPU%/RSS for a pid, or `None` if it isn't alive.
    pub fn stat(&self, pid: i32) -> Option<ProcStat> {
        if pid <= 0 {
            return None;
        }
        self.sys.process(Pid::from_u32(pid as u32)).map(|p| ProcStat {
            cpu_pct: p.cpu_usage(),
            rss_mb: p.memory() / (1024 * 1024),
        })
    }

    /// Batch stats for many pids.
    pub fn stats(&self, pids: &[i32]) -> HashMap<i32, ProcStat> {
        pids.iter()
            .filter_map(|&pid| self.stat(pid).map(|s| (pid, s)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_is_alive() {
        let mut probe = ProcessProbe::new();
        probe.refresh();
        let me = std::process::id() as i32;
        assert!(probe.is_alive(me));
        assert!(probe.stat(me).is_some());
        // A pid that cannot exist.
        assert!(!probe.is_alive(-1));
    }
}
