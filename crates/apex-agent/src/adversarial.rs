//! Adversarial test-mutant loop (AdverTest paper).
//!
//! Iteratively strengthens tests by:
//! 1. Generating a test targeting a branch
//! 2. Generating a code mutant that kills the test
//! 3. Generating a new test that detects the mutant
//! 4. Repeating until convergence or max rounds

/// One round of the adversarial test-mutant loop.
#[derive(Debug, Clone)]
pub struct AdversarialRound {
    pub round_number: u32,
    pub test_code: String,
    pub mutant_code: Option<String>,
    pub mutant_killed: bool,
}

/// Configuration for the adversarial loop.
#[derive(Debug, Clone)]
pub struct AdversarialConfig {
    pub max_rounds: u32,
    pub target_mutation_score: f64,
}

impl Default for AdversarialConfig {
    fn default() -> Self {
        AdversarialConfig {
            max_rounds: 3,
            target_mutation_score: 0.8,
        }
    }
}

/// Adversarial test-mutant loop state.
pub struct AdversarialLoop {
    pub config: AdversarialConfig,
    pub rounds: Vec<AdversarialRound>,
}

impl AdversarialLoop {
    pub fn new(config: AdversarialConfig) -> Self {
        AdversarialLoop {
            config,
            rounds: Vec::new(),
        }
    }

    pub fn record_round(&mut self, round: AdversarialRound) {
        self.rounds.push(round);
    }

    pub fn should_continue(&self) -> bool {
        if self.rounds.len() >= self.config.max_rounds as usize {
            return false;
        }
        if let Some(last) = self.rounds.last() {
            if last.mutant_killed {
                return false;
            }
        }
        if self.mutation_score() >= self.config.target_mutation_score {
            return false;
        }
        true
    }

    pub fn mutation_score(&self) -> f64 {
        let with_mutants: Vec<_> = self
            .rounds
            .iter()
            .filter(|r| r.mutant_code.is_some())
            .collect();
        if with_mutants.is_empty() {
            return 0.0;
        }
        let killed = with_mutants.iter().filter(|r| r.mutant_killed).count();
        killed as f64 / with_mutants.len() as f64
    }

    pub fn best_test(&self) -> Option<&str> {
        self.rounds
            .iter()
            .rev()
            .find(|r| r.mutant_killed)
            .map(|r| r.test_code.as_str())
            .or_else(|| self.rounds.last().map(|r| r.test_code.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adversarial_round_creation() {
        let round = AdversarialRound {
            round_number: 1,
            test_code: "def test_foo(): assert foo(1) == 2".to_string(),
            mutant_code: Some("def foo(x): return x + 2".to_string()),
            mutant_killed: false,
        };
        assert_eq!(round.round_number, 1);
        assert!(!round.mutant_killed);
    }

    #[test]
    fn adversarial_config_defaults() {
        let config = AdversarialConfig::default();
        assert_eq!(config.max_rounds, 3);
        assert!(config.target_mutation_score > 0.0);
    }

    #[test]
    fn adversarial_loop_new() {
        let config = AdversarialConfig {
            max_rounds: 5,
            target_mutation_score: 0.8,
        };
        let loop_ = AdversarialLoop::new(config);
        assert_eq!(loop_.config.max_rounds, 5);
        assert!(loop_.rounds.is_empty());
    }

    #[test]
    fn adversarial_loop_record_round() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        let round = AdversarialRound {
            round_number: 1,
            test_code: "test".to_string(),
            mutant_code: None,
            mutant_killed: false,
        };
        loop_.record_round(round);
        assert_eq!(loop_.rounds.len(), 1);
    }

    #[test]
    fn adversarial_loop_should_continue_under_max() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig {
            max_rounds: 3,
            target_mutation_score: 0.8,
        });
        let round = AdversarialRound {
            round_number: 1,
            test_code: "test".to_string(),
            mutant_code: Some("mutant".to_string()),
            mutant_killed: false,
        };
        loop_.record_round(round);
        assert!(loop_.should_continue());
    }

    #[test]
    fn adversarial_loop_stops_at_max_rounds() {
        let config = AdversarialConfig {
            max_rounds: 2,
            target_mutation_score: 0.8,
        };
        let mut loop_ = AdversarialLoop::new(config);
        for i in 0..2 {
            loop_.record_round(AdversarialRound {
                round_number: i + 1,
                test_code: format!("test_{i}"),
                mutant_code: Some(format!("mutant_{i}")),
                mutant_killed: false,
            });
        }
        assert!(!loop_.should_continue());
    }

    #[test]
    fn adversarial_loop_stops_when_mutant_killed() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        loop_.record_round(AdversarialRound {
            round_number: 1,
            test_code: "test".to_string(),
            mutant_code: Some("mutant".to_string()),
            mutant_killed: true,
        });
        assert!(!loop_.should_continue());
    }

    #[test]
    fn adversarial_loop_mutation_score() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        loop_.record_round(AdversarialRound {
            round_number: 1,
            test_code: "t1".into(),
            mutant_code: Some("m1".into()),
            mutant_killed: true,
        });
        loop_.record_round(AdversarialRound {
            round_number: 2,
            test_code: "t2".into(),
            mutant_code: Some("m2".into()),
            mutant_killed: false,
        });
        let score = loop_.mutation_score();
        assert!((score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn adversarial_loop_mutation_score_no_mutants() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        loop_.record_round(AdversarialRound {
            round_number: 1,
            test_code: "t1".into(),
            mutant_code: None,
            mutant_killed: false,
        });
        assert_eq!(loop_.mutation_score(), 0.0);
    }

    #[test]
    fn adversarial_loop_best_test() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        loop_.record_round(AdversarialRound {
            round_number: 1,
            test_code: "weak_test".into(),
            mutant_code: Some("m".into()),
            mutant_killed: false,
        });
        loop_.record_round(AdversarialRound {
            round_number: 2,
            test_code: "strong_test".into(),
            mutant_code: Some("m2".into()),
            mutant_killed: true,
        });
        assert_eq!(loop_.best_test(), Some("strong_test"));
    }

    #[test]
    fn adversarial_loop_best_test_none_when_empty() {
        let loop_ = AdversarialLoop::new(AdversarialConfig::default());
        assert_eq!(loop_.best_test(), None);
    }

    #[test]
    fn adversarial_loop_stops_at_target_mutation_score() {
        let config = AdversarialConfig {
            max_rounds: 10,
            target_mutation_score: 0.5,
        };
        let mut loop_ = AdversarialLoop::new(config);
        loop_.record_round(AdversarialRound {
            round_number: 1,
            test_code: "t".into(),
            mutant_code: Some("m".into()),
            mutant_killed: true,
        });
        assert!(!loop_.should_continue());
    }

    #[test]
    fn should_continue_true_when_no_rounds() {
        let loop_ = AdversarialLoop::new(AdversarialConfig::default());
        assert!(loop_.should_continue());
    }

    #[test]
    fn mutation_score_all_killed() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        for i in 1..=3 {
            loop_.record_round(AdversarialRound {
                round_number: i,
                test_code: format!("t{i}"),
                mutant_code: Some(format!("m{i}")),
                mutant_killed: true,
            });
        }
        assert!((loop_.mutation_score() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mutation_score_none_killed() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        for i in 1..=3 {
            loop_.record_round(AdversarialRound {
                round_number: i,
                test_code: format!("t{i}"),
                mutant_code: Some(format!("m{i}")),
                mutant_killed: false,
            });
        }
        assert_eq!(loop_.mutation_score(), 0.0);
    }

    #[test]
    fn best_test_returns_last_when_none_killed() {
        let mut loop_ = AdversarialLoop::new(AdversarialConfig::default());
        loop_.record_round(AdversarialRound {
            round_number: 1,
            test_code: "first".into(),
            mutant_code: None,
            mutant_killed: false,
        });
        loop_.record_round(AdversarialRound {
            round_number: 2,
            test_code: "second".into(),
            mutant_code: None,
            mutant_killed: false,
        });
        assert_eq!(loop_.best_test(), Some("second"));
    }

    #[test]
    fn adversarial_round_debug() {
        let round = AdversarialRound {
            round_number: 1,
            test_code: "t".into(),
            mutant_code: None,
            mutant_killed: false,
        };
        let _ = format!("{:?}", round);
    }

    #[test]
    fn adversarial_config_debug() {
        let config = AdversarialConfig::default();
        let _ = format!("{:?}", config);
    }
}
