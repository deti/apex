use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Default)]
pub struct SemanticSignals {
    pub stack_depth_max: usize,
    pub unique_value_count: usize,
    pub assertion_distance: f64,
}

pub fn extract_signals(observed_values: &[u64], stderr: &str) -> SemanticSignals {
    let unique_value_count = observed_values.iter().collect::<HashSet<_>>().len();
    let assertion_distance = parse_assertion_distance(stderr);
    SemanticSignals {
        stack_depth_max: 0,
        unique_value_count,
        assertion_distance,
    }
}

fn parse_assertion_distance(stderr: &str) -> f64 {
    try_parse_assertion_distance(stderr).unwrap_or(0.0)
}

static ASSERTION_DISTANCE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"expected\s+(\d+)\s+got\s+(\d+)").unwrap());

fn try_parse_assertion_distance(stderr: &str) -> Option<f64> {
    let caps = ASSERTION_DISTANCE_RE.captures(stderr)?;
    let a: f64 = caps[1].parse().ok()?;
    let b: f64 = caps[2].parse().ok()?;
    Some((a - b).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_trace_gives_zero_signals() {
        let sig = extract_signals(&[], "");
        assert_eq!(sig.stack_depth_max, 0);
        assert_eq!(sig.unique_value_count, 0);
    }

    #[test]
    fn value_diversity_counts_unique_u64s() {
        let values: Vec<u64> = vec![1, 2, 2, 3, 3, 3];
        let sig = extract_signals(&values, "");
        assert_eq!(sig.unique_value_count, 3);
    }

    #[test]
    fn assertion_distance_parsed_from_stderr() {
        let stderr = "AssertionError: expected 5 got 7";
        let sig = extract_signals(&[], stderr);
        // Distance = |5 - 7| = 2; non-zero means assertion was close.
        assert!(sig.assertion_distance > 0.0);
    }
}
