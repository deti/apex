use std::collections::VecDeque;

pub struct CoverageMonitor {
    window: VecDeque<(u64, usize)>,
    window_size: usize,
    stall_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorAction {
    Normal,
    SwitchStrategy,
    AgentCycle,
    Stop,
}

impl CoverageMonitor {
    pub fn new(window_size: usize) -> Self {
        CoverageMonitor {
            window: VecDeque::new(),
            window_size,
            stall_count: 0,
        }
    }

    pub fn record(&mut self, iteration: u64, covered: usize) {
        // Check if coverage grew compared to most recent sample.
        let grew = self.window.back().is_some_and(|&(_, prev)| covered > prev);

        if grew {
            self.stall_count = 0;
        } else if !self.window.is_empty() {
            self.stall_count += 1;
        }

        self.window.push_back((iteration, covered));
        if self.window.len() > self.window_size {
            self.window.pop_front();
        }
    }

    pub fn growth_rate(&self) -> f64 {
        if self.window.len() < 2 {
            return 0.0;
        }
        let oldest = self.window.front().map(|e| e.1).unwrap_or(0);
        let newest = self.window.back().map(|e| e.1).unwrap_or(0);
        (newest as f64 - oldest as f64) / self.window.len() as f64
    }

    pub fn action(&self) -> MonitorAction {
        if self.stall_count == 0 {
            MonitorAction::Normal
        } else if self.stall_count < 2 * self.window_size {
            MonitorAction::SwitchStrategy
        } else if self.stall_count < 4 * self.window_size {
            MonitorAction::AgentCycle
        } else {
            MonitorAction::Stop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_window() {
        let m = CoverageMonitor::new(5);
        assert_eq!(m.growth_rate(), 0.0);
    }

    #[test]
    fn record_single_sample() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        assert_eq!(m.growth_rate(), 0.0);
    }

    #[test]
    fn record_growing_coverage() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        m.record(1, 20);
        m.record(2, 30);
        assert!(m.growth_rate() > 0.0);
        assert_eq!(m.action(), MonitorAction::Normal);
    }

    #[test]
    fn stalled_coverage_escalates() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        for i in 1..=10 {
            m.record(i, 10);
        }
        assert_ne!(m.action(), MonitorAction::Normal);
    }

    #[test]
    fn window_evicts_old_entries() {
        let mut m = CoverageMonitor::new(3);
        for i in 0..5 {
            m.record(i as u64, i * 10);
        }
        assert_eq!(m.window.len(), 3);
    }

    #[test]
    fn action_escalation_levels() {
        let mut m = CoverageMonitor::new(3);
        m.record(0, 10);

        // 3 stalls → SwitchStrategy (stall_count < 2*3=6)
        for i in 1..=3 {
            m.record(i, 10);
        }
        assert_eq!(m.stall_count, 3);
        assert_eq!(m.action(), MonitorAction::SwitchStrategy);

        // 6 stalls → AgentCycle (stall_count >= 2*3=6, < 4*3=12)
        for i in 4..=6 {
            m.record(i, 10);
        }
        assert_eq!(m.stall_count, 6);
        assert_eq!(m.action(), MonitorAction::AgentCycle);

        // 12 stalls → Stop (stall_count >= 4*3=12)
        for i in 7..=12 {
            m.record(i as u64, 10);
        }
        assert_eq!(m.stall_count, 12);
        assert_eq!(m.action(), MonitorAction::Stop);
    }

    #[test]
    fn recovery_resets_escalation() {
        let mut m = CoverageMonitor::new(3);
        m.record(0, 10);
        // Stall a few times
        for i in 1..=5 {
            m.record(i, 10);
        }
        assert_ne!(m.action(), MonitorAction::Normal);

        // Now grow — should reset
        m.record(6, 20);
        assert_eq!(m.action(), MonitorAction::Normal);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// First call on an empty window: neither `grew` (no previous entry) nor
    /// `!self.window.is_empty()` (window is empty before push) → stall_count
    /// stays at 0, action is Normal.
    #[test]
    fn first_record_does_not_increment_stall() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        assert_eq!(m.stall_count, 0);
        assert_eq!(m.action(), MonitorAction::Normal);
    }

    /// Coverage grows on the second call → stall_count reset (even if it was >0).
    #[test]
    fn growth_resets_stall_count_from_nonzero() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        m.record(1, 10); // stall → stall_count = 1
        assert_eq!(m.stall_count, 1);
        m.record(2, 20); // grew → stall_count reset
        assert_eq!(m.stall_count, 0);
        assert_eq!(m.action(), MonitorAction::Normal);
    }

    /// `growth_rate()` with exactly 2 entries returns correct non-zero value.
    #[test]
    fn growth_rate_with_exactly_two_entries() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        m.record(1, 30);
        // rate = (30 - 10) / 2 = 10.0
        let rate = m.growth_rate();
        assert!((rate - 10.0).abs() < 1e-9, "rate = {rate}");
    }

    /// `growth_rate()` with a full window that plateaus → rate = 0.
    #[test]
    fn growth_rate_flat_coverage() {
        let mut m = CoverageMonitor::new(4);
        for i in 0..4 {
            m.record(i as u64, 50);
        }
        assert_eq!(m.growth_rate(), 0.0);
    }

    /// `growth_rate()` when newest < oldest (coverage could theoretically decrease).
    #[test]
    fn growth_rate_can_be_negative() {
        let mut m = CoverageMonitor::new(3);
        m.record(0, 100);
        m.record(1, 50);
        m.record(2, 20);
        // oldest = 100 (evicted once more entries come), but after 3 records
        // with window_size=3 front=100, back=20 → rate = (20-100)/3 = -26.67
        let rate = m.growth_rate();
        assert!(rate < 0.0, "expected negative rate, got {rate}");
    }

    /// Exact boundary: stall_count == 2 * window_size → AgentCycle (not SwitchStrategy).
    #[test]
    fn action_boundary_switch_to_agent_cycle() {
        let mut m = CoverageMonitor::new(4); // 2*4=8, 4*4=16
        m.record(0, 10);
        // Push stall_count to exactly 8
        for i in 1..=8 {
            m.record(i, 10);
        }
        assert_eq!(m.stall_count, 8);
        // stall_count < 2*window_size is false (8 < 8 is false) → AgentCycle
        assert_eq!(m.action(), MonitorAction::AgentCycle);
    }

    /// Exact boundary: stall_count == 4 * window_size → Stop.
    #[test]
    fn action_boundary_agent_cycle_to_stop() {
        let mut m = CoverageMonitor::new(4); // 4*4=16
        m.record(0, 10);
        for i in 1..=16 {
            m.record(i, 10);
        }
        assert_eq!(m.stall_count, 16);
        assert_eq!(m.action(), MonitorAction::Stop);
    }

    /// Exactly 1 stall → SwitchStrategy (stall_count < 2 * window_size when window is large).
    #[test]
    fn one_stall_gives_switch_strategy() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        m.record(1, 10); // stall_count = 1
        // 1 < 2*5=10 → SwitchStrategy
        assert_eq!(m.action(), MonitorAction::SwitchStrategy);
    }

    /// Window eviction keeps exactly window_size entries.
    #[test]
    fn window_eviction_is_exact() {
        let mut m = CoverageMonitor::new(2);
        m.record(0, 1);
        m.record(1, 2);
        m.record(2, 3);
        // Only 2 entries should remain.
        assert_eq!(m.window.len(), 2);
        // Front should be the entry at iteration 1.
        assert_eq!(m.window.front().unwrap().0, 1);
    }

    /// MonitorAction variants satisfy PartialEq (sanity check for all arms).
    #[test]
    fn monitor_action_all_variants_eq() {
        assert_eq!(MonitorAction::Normal, MonitorAction::Normal);
        assert_eq!(MonitorAction::SwitchStrategy, MonitorAction::SwitchStrategy);
        assert_eq!(MonitorAction::AgentCycle, MonitorAction::AgentCycle);
        assert_eq!(MonitorAction::Stop, MonitorAction::Stop);
        assert_ne!(MonitorAction::Normal, MonitorAction::Stop);
    }

    /// `record()` with same coverage value multiple times always increments stall.
    #[test]
    fn repeated_same_coverage_increments_stall_each_time() {
        let mut m = CoverageMonitor::new(10);
        m.record(0, 5);
        for i in 1..=4 {
            m.record(i, 5);
        }
        assert_eq!(m.stall_count, 4);
    }
}
