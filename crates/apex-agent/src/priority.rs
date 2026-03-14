use apex_core::types::BranchId;

/// A candidate branch for priority-based selection.
#[derive(Debug, Clone)]
pub struct BranchCandidate {
    pub id: BranchId,
    pub heuristic: f64,
    pub attempts_since_progress: u64,
    pub depth_in_cfg: u32,
    pub hit_count: u64,
}

/// Priority score for an uncovered branch — higher = explore first.
///
/// Composite of four signals (from Owi + EvoMaster):
/// - Rarity: prefer branches reached by fewer inputs
/// - Depth penalty: penalize deeply nested paths
/// - Proximity: branches closer to flipping (high heuristic) get priority
/// - Staleness bonus: branches not making progress get a boost to try different strategies
pub fn target_priority(
    heuristic: f64,               // from CoverageOracle: how close we've gotten [0,1]
    attempts_since_progress: u64, // how many iterations without improvement
    depth_in_cfg: u32,            // depth in control-flow graph
    hit_count: u64,               // how many times we've reached nearby code
) -> f64 {
    let rarity = 1.0 / (hit_count as f64 + 1.0);
    let depth_penalty = 1.0 / (depth_in_cfg as f64).ln_1p().max(1.0);
    let staleness_bonus = if attempts_since_progress > 5 {
        0.5
    } else {
        0.0
    };
    let proximity = heuristic;

    rarity * depth_penalty * (1.0 + proximity) + staleness_bonus
}

/// Select the top-K branches by priority.
pub fn select_top_targets(branches: &[BranchCandidate], k: usize) -> Vec<BranchId> {
    let mut scored: Vec<_> = branches
        .iter()
        .map(|b| {
            (
                b.id.clone(),
                target_priority(
                    b.heuristic,
                    b.attempts_since_progress,
                    b.depth_in_cfg,
                    b.hit_count,
                ),
            )
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(k).map(|(id, _)| id).collect()
}

/// Recommend which strategy to use based on branch characteristics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyRecommendation {
    /// High heuristic (>0.8): use gradient solver (close to flipping)
    Gradient,
    /// Medium heuristic: use fuzzer (mutation might reach it)
    Fuzz,
    /// Low heuristic or stalled: use LLM synthesis (need structured approach)
    LlmSynth,
}

pub fn recommend_strategy(heuristic: f64, attempts_since_progress: u64) -> StrategyRecommendation {
    if attempts_since_progress > 10 {
        StrategyRecommendation::LlmSynth // Stalled — rotate to LLM
    } else if heuristic > 0.8 {
        StrategyRecommendation::Gradient // Close — nudge with gradient
    } else if heuristic > 0.3 {
        StrategyRecommendation::Fuzz // Medium — mutation might work
    } else {
        StrategyRecommendation::LlmSynth // Far — need structured approach
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    fn make_branch(line: u32) -> BranchId {
        BranchId::new(1, line, 0, 0)
    }

    #[test]
    fn priority_prefers_rare_branches() {
        let rare = target_priority(0.5, 0, 3, 1);
        let common = target_priority(0.5, 0, 3, 100);
        assert!(rare > common);
    }

    #[test]
    fn priority_penalizes_deep_paths() {
        let shallow = target_priority(0.5, 0, 2, 10);
        let deep = target_priority(0.5, 0, 20, 10);
        assert!(shallow > deep);
    }

    #[test]
    fn priority_rewards_proximity() {
        let close = target_priority(0.9, 0, 3, 10);
        let far = target_priority(0.1, 0, 3, 10);
        assert!(close > far);
    }

    #[test]
    fn priority_staleness_bonus_kicks_in() {
        let stale = target_priority(0.5, 10, 3, 10);
        let fresh = target_priority(0.5, 2, 3, 10);
        assert!(stale > fresh);
    }

    #[test]
    fn select_top_targets_returns_k() {
        let branches = vec![
            BranchCandidate {
                id: make_branch(1),
                heuristic: 0.1,
                attempts_since_progress: 0,
                depth_in_cfg: 5,
                hit_count: 50,
            },
            BranchCandidate {
                id: make_branch(2),
                heuristic: 0.9,
                attempts_since_progress: 0,
                depth_in_cfg: 2,
                hit_count: 1,
            },
            BranchCandidate {
                id: make_branch(3),
                heuristic: 0.5,
                attempts_since_progress: 0,
                depth_in_cfg: 3,
                hit_count: 10,
            },
            BranchCandidate {
                id: make_branch(4),
                heuristic: 0.7,
                attempts_since_progress: 0,
                depth_in_cfg: 4,
                hit_count: 5,
            },
            BranchCandidate {
                id: make_branch(5),
                heuristic: 0.3,
                attempts_since_progress: 0,
                depth_in_cfg: 6,
                hit_count: 20,
            },
        ];
        let top = select_top_targets(&branches, 3);
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn select_top_targets_fewer_than_k() {
        let branches = vec![
            BranchCandidate {
                id: make_branch(1),
                heuristic: 0.5,
                attempts_since_progress: 0,
                depth_in_cfg: 3,
                hit_count: 10,
            },
            BranchCandidate {
                id: make_branch(2),
                heuristic: 0.9,
                attempts_since_progress: 0,
                depth_in_cfg: 2,
                hit_count: 1,
            },
        ];
        let top = select_top_targets(&branches, 5);
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn select_top_targets_orders_by_priority() {
        // Branch 2 has high heuristic + low hit_count → highest priority
        let branches = vec![
            BranchCandidate {
                id: make_branch(1),
                heuristic: 0.1,
                attempts_since_progress: 0,
                depth_in_cfg: 3,
                hit_count: 100,
            },
            BranchCandidate {
                id: make_branch(2),
                heuristic: 0.9,
                attempts_since_progress: 0,
                depth_in_cfg: 2,
                hit_count: 1,
            },
            BranchCandidate {
                id: make_branch(3),
                heuristic: 0.5,
                attempts_since_progress: 0,
                depth_in_cfg: 3,
                hit_count: 10,
            },
        ];
        let top = select_top_targets(&branches, 1);
        assert_eq!(top[0], make_branch(2));
    }

    #[test]
    fn recommend_strategy_high_heuristic() {
        assert_eq!(recommend_strategy(0.9, 0), StrategyRecommendation::Gradient);
    }

    #[test]
    fn recommend_strategy_medium_heuristic() {
        assert_eq!(recommend_strategy(0.5, 0), StrategyRecommendation::Fuzz);
    }

    #[test]
    fn recommend_strategy_low_heuristic() {
        assert_eq!(recommend_strategy(0.1, 0), StrategyRecommendation::LlmSynth);
    }

    #[test]
    fn recommend_strategy_stalled_overrides_heuristic() {
        assert_eq!(
            recommend_strategy(0.9, 15),
            StrategyRecommendation::LlmSynth
        );
    }

    #[test]
    fn recommend_strategy_boundary_heuristic_fuzz() {
        // Exactly 0.3 is not > 0.3, so falls to LlmSynth
        assert_eq!(recommend_strategy(0.3, 0), StrategyRecommendation::LlmSynth);
        // Just above 0.3 → Fuzz
        assert_eq!(recommend_strategy(0.31, 0), StrategyRecommendation::Fuzz);
    }

    #[test]
    fn recommend_strategy_boundary_heuristic_gradient() {
        // Exactly 0.8 is not > 0.8, so falls to Fuzz
        assert_eq!(recommend_strategy(0.8, 0), StrategyRecommendation::Fuzz);
        // Just above 0.8 → Gradient
        assert_eq!(
            recommend_strategy(0.81, 0),
            StrategyRecommendation::Gradient
        );
    }

    #[test]
    fn recommend_strategy_stall_boundary() {
        // Exactly 10 attempts is not > 10, so heuristic still applies
        assert_eq!(
            recommend_strategy(0.9, 10),
            StrategyRecommendation::Gradient
        );
        // 11 attempts → stalled → LlmSynth regardless of heuristic
        assert_eq!(
            recommend_strategy(0.9, 11),
            StrategyRecommendation::LlmSynth
        );
    }
}
