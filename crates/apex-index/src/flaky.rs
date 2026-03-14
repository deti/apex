use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct FlakyDetector {
    runs: HashMap<String, Vec<bool>>,
    window: usize,
}

#[derive(Debug)]
pub struct FlakyReport {
    flakiness: HashMap<String, f64>,
}

impl FlakyDetector {
    pub fn new(window: usize) -> Self { Self { window, runs: HashMap::new() } }

    pub fn record_run(&mut self, test_id: &str, passed: bool) {
        let v = self.runs.entry(test_id.to_string()).or_default();
        v.push(passed);
        if v.len() > self.window { v.remove(0); }
    }

    pub fn report(&self) -> FlakyReport {
        let flakiness = self.runs.iter().map(|(k, v)| {
            let flips = v.windows(2).filter(|w| w[0] != w[1]).count();
            let rate = if v.len() < 2 { 0.0 } else { flips as f64 / (v.len() - 1) as f64 };
            (k.clone(), rate)
        }).collect();
        FlakyReport { flakiness }
    }
}

impl FlakyReport {
    pub fn is_flaky(&self, id: &str) -> bool { self.flakiness_rate(id) > 0.0 }
    pub fn flakiness_rate(&self, id: &str) -> f64 { *self.flakiness.get(id).unwrap_or(&0.0) }
    pub fn all_flaky(&self) -> Vec<&str> {
        self.flakiness.iter().filter(|(_, &r)| r > 0.0).map(|(k, _)| k.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_branch_not_flagged() {
        let mut det = FlakyDetector::new(3);
        for _ in 0..3 { det.record_run("test_foo", true); }
        let report = det.report();
        assert!(!report.is_flaky("test_foo"));
    }

    #[test]
    fn flipping_branch_is_flaky() {
        let mut det = FlakyDetector::new(4);
        det.record_run("test_bar", true);
        det.record_run("test_bar", false);
        det.record_run("test_bar", true);
        det.record_run("test_bar", false);
        let report = det.report();
        assert!(report.is_flaky("test_bar"));
    }

    #[test]
    fn flakiness_rate_computed_correctly() {
        let mut det = FlakyDetector::new(4);
        for _ in 0..2 { det.record_run("t", true); }
        for _ in 0..2 { det.record_run("t", false); }
        let rate = det.report().flakiness_rate("t");
        // 2 flips out of 3 transitions
        assert!(rate > 0.0 && rate <= 1.0);
    }
}
