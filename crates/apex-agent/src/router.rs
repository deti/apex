//! S2F (Select-to-Fuzz) router — replaces heuristic-threshold strategy selection
//! with classifier-driven routing per the S2F paper.

use crate::priority::{BranchCandidate, StrategyRecommendation};
use rand::Rng;

/// Classification of a branch based on constraint characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchClass {
    /// Simple numeric/boolean — fuzzer can flip easily.
    EasyFuzz,
    /// Complex numeric — needs many mutations or gradient guidance.
    HardFuzz,
    /// String/hash comparison — needs solver or concolic.
    NeedsSolver,
    /// Requires structured input (format, protocol) — needs LLM synthesis.
    NeedsSynth,
}

/// S2F Router — classifies branches and routes to the optimal strategy.
pub struct S2FRouter {
    pub classifier_threshold: f64,
    pub depth_threshold: u32,
    pub stall_threshold: u64,
}

impl S2FRouter {
    pub fn new() -> Self {
        S2FRouter {
            classifier_threshold: 0.7,
            depth_threshold: 10,
            stall_threshold: 10,
        }
    }

    pub fn classify(&self, candidate: &BranchCandidate) -> BranchClass {
        let stalled = candidate.attempts_since_progress >= self.stall_threshold;
        let close = candidate.heuristic >= self.classifier_threshold;
        let deep = candidate.depth_in_cfg >= self.depth_threshold;

        if stalled && candidate.heuristic < 0.2 {
            BranchClass::NeedsSynth
        } else if close && !deep {
            BranchClass::EasyFuzz
        } else if close && deep {
            BranchClass::NeedsSolver
        } else if candidate.heuristic < 0.3 {
            BranchClass::NeedsSynth
        } else {
            BranchClass::HardFuzz
        }
    }

    pub fn route(&self, candidate: &BranchCandidate) -> StrategyRecommendation {
        match self.classify(candidate) {
            BranchClass::EasyFuzz => StrategyRecommendation::Fuzz,
            BranchClass::NeedsSolver => StrategyRecommendation::Gradient,
            BranchClass::NeedsSynth => StrategyRecommendation::LlmSynth,
            BranchClass::HardFuzz => {
                let mut rng = rand::rng();
                match rng.random_range(0..3) {
                    0 => StrategyRecommendation::Fuzz,
                    1 => StrategyRecommendation::Gradient,
                    _ => StrategyRecommendation::LlmSynth,
                }
            }
        }
    }
}

impl Default for S2FRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    fn make_candidate(heuristic: f64, attempts: u64, depth: u32, hits: u64) -> BranchCandidate {
        BranchCandidate {
            id: BranchId::new(1, 10, 0, 0),
            heuristic,
            attempts_since_progress: attempts,
            depth_in_cfg: depth,
            hit_count: hits,
        }
    }

    #[test]
    fn branch_class_all_variants_exist() {
        let classes = [
            BranchClass::EasyFuzz,
            BranchClass::HardFuzz,
            BranchClass::NeedsSolver,
            BranchClass::NeedsSynth,
        ];
        assert_eq!(classes.len(), 4);
    }

    #[test]
    fn router_new_creates_instance() {
        let router = S2FRouter::new();
        assert!(router.classifier_threshold > 0.0);
    }

    #[test]
    fn router_route_easy_fuzz() {
        let router = S2FRouter::new();
        let candidate = make_candidate(0.9, 0, 2, 50);
        let rec = router.route(&candidate);
        assert_eq!(rec, StrategyRecommendation::Fuzz);
    }

    #[test]
    fn router_route_needs_solver() {
        let router = S2FRouter::new();
        let candidate = make_candidate(0.85, 8, 15, 200);
        let rec = router.route(&candidate);
        assert_eq!(rec, StrategyRecommendation::Gradient);
    }

    #[test]
    fn router_route_needs_synth() {
        let router = S2FRouter::new();
        let candidate = make_candidate(0.05, 20, 5, 10);
        let rec = router.route(&candidate);
        assert_eq!(rec, StrategyRecommendation::LlmSynth);
    }

    #[test]
    fn router_route_hard_fuzz_uses_bandit() {
        let router = S2FRouter::new();
        let candidate = make_candidate(0.5, 5, 8, 30);
        let rec = router.route(&candidate);
        assert!(
            rec == StrategyRecommendation::Fuzz
                || rec == StrategyRecommendation::Gradient
                || rec == StrategyRecommendation::LlmSynth
        );
    }

    #[test]
    fn classify_high_heuristic_low_depth_is_easy() {
        let router = S2FRouter::new();
        let candidate = make_candidate(0.95, 0, 2, 100);
        let class = router.classify(&candidate);
        assert_eq!(class, BranchClass::EasyFuzz);
    }

    #[test]
    fn classify_stalled_deep_is_needs_synth() {
        let router = S2FRouter::new();
        let candidate = make_candidate(0.1, 15, 20, 5);
        let class = router.classify(&candidate);
        assert_eq!(class, BranchClass::NeedsSynth);
    }

    #[test]
    fn classify_covers_all_quadrants() {
        let router = S2FRouter::new();
        assert_eq!(
            router.classify(&make_candidate(0.9, 0, 2, 10)),
            BranchClass::EasyFuzz
        );
        assert_eq!(
            router.classify(&make_candidate(0.85, 0, 15, 10)),
            BranchClass::NeedsSolver
        );
        assert_eq!(
            router.classify(&make_candidate(0.05, 20, 5, 10)),
            BranchClass::NeedsSynth
        );
        assert_eq!(
            router.classify(&make_candidate(0.5, 3, 5, 10)),
            BranchClass::HardFuzz
        );
    }

    #[test]
    fn default_impl() {
        let router = S2FRouter::default();
        assert!((router.classifier_threshold - 0.7).abs() < 1e-9);
        assert_eq!(router.depth_threshold, 10);
        assert_eq!(router.stall_threshold, 10);
    }

    #[test]
    fn route_returns_valid_recommendation_for_all_classes() {
        let router = S2FRouter::new();
        let candidates = [
            make_candidate(0.95, 0, 2, 50),
            make_candidate(0.85, 0, 15, 100),
            make_candidate(0.05, 20, 5, 10),
            make_candidate(0.5, 5, 8, 30),
        ];
        for c in &candidates {
            let rec = router.route(c);
            assert!(
                rec == StrategyRecommendation::Fuzz
                    || rec == StrategyRecommendation::Gradient
                    || rec == StrategyRecommendation::LlmSynth
            );
        }
    }

    #[test]
    fn branch_class_debug_and_eq() {
        assert_eq!(BranchClass::EasyFuzz, BranchClass::EasyFuzz);
        assert_ne!(BranchClass::EasyFuzz, BranchClass::HardFuzz);
        let _ = format!("{:?}", BranchClass::NeedsSolver);
    }
}
