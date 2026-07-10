//! Cruise Control — the Governor's enforcement policy. Pure and deterministic:
//! a snapshot in, a pacing plan out. A single dual-price controller paces the
//! fleet's burn and shares the budget fairly at once (Kelly NUM / dual mirror
//! descent); an AIMD overlay handles the 429 shock. No enforcement here.

/// Billable tok/min that spends `remaining - reserve` evenly until the deadline.
/// `reserve == 0` and `deadline == reset` gives the Governor's coast pace.
pub fn target_rate(remaining: u64, reserve: u64, mins_to_deadline: f64) -> f64 {
    if mins_to_deadline <= 0.0 {
        return 0.0;
    }
    let spendable = remaining.saturating_sub(reserve) as f64;
    spendable / mins_to_deadline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coast_spends_all_remaining_over_the_window() {
        // 120M remaining, no reserve, 200 min to reset → 600k/min.
        assert_eq!(target_rate(120_000_000, 0, 200.0), 600_000.0);
    }

    #[test]
    fn reservation_lowers_the_target() {
        // Keep 20M in reserve → only 100M is spendable → 500k/min.
        assert_eq!(target_rate(120_000_000, 20_000_000, 200.0), 500_000.0);
    }

    #[test]
    fn past_deadline_or_over_reserve_is_zero() {
        assert_eq!(target_rate(120_000_000, 0, 0.0), 0.0);
        assert_eq!(target_rate(10_000_000, 20_000_000, 200.0), 0.0);
    }
}
