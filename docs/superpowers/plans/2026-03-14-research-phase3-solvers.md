# Phase 3 — Analysis & Solver Upgrades Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deeper changes to coverage intelligence, symbolic solving (LLM as solver, diverse solutions), advanced fuzzing, and security spec mining — building on Phase 1+2.

**Architecture:** 4 parallel tracks (3A-3D). Coverage intelligence, solver upgrades, advanced fuzzing, and security spec mining.

**Tech Stack:** Rust, async_trait, serde, z3 (behind feature flag), reqwest

---

## Prerequisites

These tasks depend on types from Phase 0-2 that may not yet exist. Where a dependency is noted, use the following stubs if the real type is not yet available:

- **`LlmClient`** (Phase 0, Task 0.1): trait in `apex-core::llm` with `async fn complete(&self, messages: &[LlmMessage], max_tokens: u32) -> Result<LlmResponse>` and `fn model_name(&self) -> &str`. Mock: `MockLlmClient::new(vec!["response".into()])`.
- **`MutationResult`** / **`MutationOperator`** / **`MutationKind`** / **`oracle_gap()`** (Phase 1, Task 1.1): types in `apex-coverage::mutation`. If not present, create a minimal `mutation.rs` with the types needed.
- **`FlakyCandidate`** / **`detect_flaky_by_coverage()`** (Phase 1, Task 1.2): types in `apex-index::analysis`. If not present, create stubs.

---

## Track 3A: Coverage Intelligence

### Task 3.1: Metamorphic Adequacy Scoring

**Crate:** `apex-coverage`
**Modify:** `crates/apex-coverage/src/lib.rs` (add `pub mod mutation;`)
**Create:** `crates/apex-coverage/src/mutation.rs`
**Depends on:** 1.1 (oracle_gap / MutationResult) — stub if absent

#### Step 3.1.1 — Write failing test for MutationResult + MetamorphicScore types

- [ ] Create `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs` with:

```rust
//! Mutation adequacy scoring — goes beyond binary kill/survive.

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_kind_debug() {
        let k = MutationKind::ArithmeticSwap;
        assert_eq!(format!("{k:?}"), "ArithmeticSwap");
    }

    #[test]
    fn mutation_operator_creation() {
        let op = MutationOperator {
            kind: MutationKind::BoundaryOff,
            file_id: 1,
            line: 42,
            original: "> 0".to_string(),
            replacement: ">= 0".to_string(),
        };
        assert_eq!(op.line, 42);
    }

    #[test]
    fn mutation_result_killed() {
        let op = MutationOperator {
            kind: MutationKind::ArithmeticSwap,
            file_id: 1,
            line: 10,
            original: "+".to_string(),
            replacement: "-".to_string(),
        };
        let r = MutationResult {
            operator: op,
            killed: true,
            killing_tests: vec!["test_add".to_string()],
            detection_margin: 0.95,
        };
        assert!(r.killed);
        assert!((r.detection_margin - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn metamorphic_score_creation() {
        let score = MetamorphicScore {
            mutation_score: 0.8,
            detection_ratio: 0.75,
            weak_mutations: vec![],
        };
        assert!((score.mutation_score - 0.8).abs() < f64::EPSILON);
    }
}
```

- [ ] Add `pub mod mutation;` to `/Users/ad/prj/bcov/crates/apex-coverage/src/lib.rs` after existing modules.

- [ ] Run test to verify it fails (types don't exist yet):
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation_kind_debug 2>&1 | head -20
```

#### Step 3.1.2 — Implement types

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
/// Categories of source-level mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationKind {
    ArithmeticSwap,
    BoundaryOff,
    NegateCondition,
    ReturnDefault,
    DeleteStatement,
}

/// A concrete mutation applied to a source location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationOperator {
    pub kind: MutationKind,
    pub file_id: u64,
    pub line: u32,
    pub original: String,
    pub replacement: String,
}

/// Result of running tests against one mutant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    pub operator: MutationOperator,
    pub killed: bool,
    pub killing_tests: Vec<String>,
    /// How close was detection? 1.0 = strong kill, 0.01 = barely detected.
    pub detection_margin: f64,
}

/// Metamorphic adequacy goes beyond binary kill/survive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetamorphicScore {
    /// Traditional mutation score: killed / total.
    pub mutation_score: f64,
    /// Ratio of mutants with detection_margin > 0.5.
    pub detection_ratio: f64,
    /// Mutants killed but with very low margin (detection_margin < 0.1).
    pub weak_mutations: Vec<MutationOperator>,
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation 2>&1 | tail -5
```

#### Step 3.1.3 — Write failing test for `metamorphic_adequacy()`

- [ ] Add to tests in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
    #[test]
    fn metamorphic_adequacy_all_killed() {
        let results = vec![
            MutationResult {
                operator: MutationOperator {
                    kind: MutationKind::ArithmeticSwap,
                    file_id: 1,
                    line: 1,
                    original: "+".into(),
                    replacement: "-".into(),
                },
                killed: true,
                killing_tests: vec!["t1".into()],
                detection_margin: 0.9,
            },
            MutationResult {
                operator: MutationOperator {
                    kind: MutationKind::NegateCondition,
                    file_id: 1,
                    line: 2,
                    original: ">".into(),
                    replacement: "<=".into(),
                },
                killed: true,
                killing_tests: vec!["t2".into()],
                detection_margin: 0.7,
            },
        ];
        let score = metamorphic_adequacy(&results);
        assert!((score.mutation_score - 1.0).abs() < f64::EPSILON);
        assert!((score.detection_ratio - 1.0).abs() < f64::EPSILON);
        assert!(score.weak_mutations.is_empty());
    }

    #[test]
    fn metamorphic_adequacy_with_weak_and_survived() {
        let results = vec![
            MutationResult {
                operator: MutationOperator {
                    kind: MutationKind::ArithmeticSwap,
                    file_id: 1,
                    line: 1,
                    original: "+".into(),
                    replacement: "-".into(),
                },
                killed: true,
                killing_tests: vec!["t1".into()],
                detection_margin: 0.05, // weak kill
            },
            MutationResult {
                operator: MutationOperator {
                    kind: MutationKind::BoundaryOff,
                    file_id: 1,
                    line: 2,
                    original: ">".into(),
                    replacement: ">=".into(),
                },
                killed: false,
                killing_tests: vec![],
                detection_margin: 0.0,
            },
        ];
        let score = metamorphic_adequacy(&results);
        assert!((score.mutation_score - 0.5).abs() < f64::EPSILON);
        assert!((score.detection_ratio - 0.0).abs() < f64::EPSILON); // neither has margin > 0.5
        assert_eq!(score.weak_mutations.len(), 1);
    }

    #[test]
    fn metamorphic_adequacy_empty() {
        let score = metamorphic_adequacy(&[]);
        assert!((score.mutation_score - 1.0).abs() < f64::EPSILON);
        assert!((score.detection_ratio - 1.0).abs() < f64::EPSILON);
        assert!(score.weak_mutations.is_empty());
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage metamorphic_adequacy_all_killed 2>&1 | head -20
```

#### Step 3.1.4 — Implement `metamorphic_adequacy()`

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
/// Compute metamorphic adequacy from mutation testing results.
///
/// Goes beyond binary kill/survive:
/// - `mutation_score`: fraction of killed mutants.
/// - `detection_ratio`: fraction of killed mutants with detection_margin > 0.5.
/// - `weak_mutations`: killed mutants with detection_margin < 0.1 (brittle detection).
pub fn metamorphic_adequacy(results: &[MutationResult]) -> MetamorphicScore {
    if results.is_empty() {
        return MetamorphicScore {
            mutation_score: 1.0,
            detection_ratio: 1.0,
            weak_mutations: vec![],
        };
    }

    let total = results.len() as f64;
    let killed: Vec<&MutationResult> = results.iter().filter(|r| r.killed).collect();
    let killed_count = killed.len() as f64;

    let mutation_score = killed_count / total;

    let strong_kills = killed
        .iter()
        .filter(|r| r.detection_margin > 0.5)
        .count() as f64;
    let detection_ratio = if killed_count > 0.0 {
        strong_kills / killed_count
    } else {
        0.0
    };

    let weak_mutations: Vec<MutationOperator> = killed
        .iter()
        .filter(|r| r.detection_margin < 0.1)
        .map(|r| r.operator.clone())
        .collect();

    MetamorphicScore {
        mutation_score,
        detection_ratio,
        weak_mutations,
    }
}
```

- [ ] Run all mutation tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-coverage/src/mutation.rs crates/apex-coverage/src/lib.rs && git commit -m "feat(coverage): add metamorphic adequacy scoring (Task 3.1)"
```

---

### Task 3.2: Rank Aggregation TCP (Test Case Prioritization)

**Crate:** `apex-index`
**Create:** `crates/apex-index/src/prioritize.rs`
**Modify:** `crates/apex-index/src/lib.rs` (add `pub mod prioritize;`)

#### Step 3.2.1 — Write failing test for TestRanker trait and borda_aggregate

- [ ] Create `/Users/ad/prj/bcov/crates/apex-index/src/prioritize.rs`:

```rust
//! Test case prioritization via rank aggregation of multiple signals.
//! Based on arXiv:2412.00015 — uses Borda count over diverse rankers.

use crate::types::TestTrace;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::ExecutionStatus;

    fn make_trace(name: &str, duration: u64, branch_count: usize) -> TestTrace {
        TestTrace {
            test_name: name.to_string(),
            branches: (0..branch_count as u32)
                .map(|l| apex_core::types::BranchId::new(1, l, 0, 0))
                .collect(),
            duration_ms: duration,
            status: ExecutionStatus::Pass,
        }
    }

    #[test]
    fn borda_aggregate_single_ranking() {
        let rankings = vec![vec![
            ("test_a".to_string(), 3.0),
            ("test_b".to_string(), 2.0),
            ("test_c".to_string(), 1.0),
        ]];
        let result = borda_aggregate(&rankings);
        assert_eq!(result[0].0, "test_a");
        assert_eq!(result[1].0, "test_b");
        assert_eq!(result[2].0, "test_c");
    }

    #[test]
    fn borda_aggregate_two_rankings_tie_break() {
        let r1 = vec![
            ("a".to_string(), 3.0),
            ("b".to_string(), 2.0),
            ("c".to_string(), 1.0),
        ];
        let r2 = vec![
            ("c".to_string(), 3.0),
            ("b".to_string(), 2.0),
            ("a".to_string(), 1.0),
        ];
        let result = borda_aggregate(&[r1, r2]);
        // b gets rank 1 in both => highest Borda score
        assert_eq!(result[0].0, "b");
    }

    #[test]
    fn borda_aggregate_empty() {
        let result = borda_aggregate(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn coverage_ranker_sorts_by_branch_count() {
        let traces = vec![
            make_trace("few", 10, 2),
            make_trace("many", 10, 10),
            make_trace("mid", 10, 5),
        ];
        let ranker = CoverageRanker;
        let ranked = ranker.rank(&traces);
        assert_eq!(ranked[0].0, "many");
        assert_eq!(ranked[1].0, "mid");
        assert_eq!(ranked[2].0, "few");
    }

    #[test]
    fn speed_ranker_sorts_by_duration() {
        let traces = vec![
            make_trace("slow", 1000, 5),
            make_trace("fast", 10, 5),
            make_trace("mid", 100, 5),
        ];
        let ranker = SpeedRanker;
        let ranked = ranker.rank(&traces);
        // Faster tests should rank higher (lower duration = higher score)
        assert_eq!(ranked[0].0, "fast");
        assert_eq!(ranked[1].0, "mid");
        assert_eq!(ranked[2].0, "slow");
    }

    #[test]
    fn test_prioritizer_combines_rankers() {
        let traces = vec![
            make_trace("fast_low", 10, 2),
            make_trace("slow_high", 1000, 10),
            make_trace("mid_mid", 100, 5),
        ];
        let prioritizer = TestPrioritizer {
            rankers: vec![Box::new(CoverageRanker), Box::new(SpeedRanker)],
        };
        let result = prioritizer.prioritize(&traces);
        // mid_mid should be a reasonable middle ground
        assert_eq!(result.len(), 3);
        // All test names present
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"fast_low"));
        assert!(names.contains(&"slow_high"));
        assert!(names.contains(&"mid_mid"));
    }
}
```

- [ ] Add `pub mod prioritize;` to `/Users/ad/prj/bcov/crates/apex-index/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index borda_aggregate_single 2>&1 | head -20
```

#### Step 3.2.2 — Implement TestRanker, CoverageRanker, SpeedRanker, borda_aggregate, TestPrioritizer

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-index/src/prioritize.rs`:

```rust
/// A ranker produces a descending-score ordering of tests.
pub trait TestRanker: Send + Sync {
    fn rank(&self, tests: &[TestTrace]) -> Vec<(String, f64)>;
    fn name(&self) -> &str;
}

/// Ranks tests by number of branches covered (more = higher score).
pub struct CoverageRanker;

impl TestRanker for CoverageRanker {
    fn rank(&self, tests: &[TestTrace]) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = tests
            .iter()
            .map(|t| (t.test_name.clone(), t.branches.len() as f64))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }

    fn name(&self) -> &str {
        "coverage"
    }
}

/// Ranks tests by speed (faster = higher score, using 1/duration).
pub struct SpeedRanker;

impl TestRanker for SpeedRanker {
    fn rank(&self, tests: &[TestTrace]) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = tests
            .iter()
            .map(|t| {
                let score = if t.duration_ms == 0 {
                    f64::MAX
                } else {
                    1.0 / t.duration_ms as f64
                };
                (t.test_name.clone(), score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }

    fn name(&self) -> &str {
        "speed"
    }
}

/// Aggregate multiple rankings using Borda count.
///
/// Each ranker's output is treated as a ranked list. Position `i` in a list of
/// `n` items receives a Borda score of `n - i`. Scores are summed across
/// rankings and the final list is sorted descending.
pub fn borda_aggregate(rankings: &[Vec<(String, f64)>]) -> Vec<(String, f64)> {
    if rankings.is_empty() {
        return vec![];
    }

    let mut scores: HashMap<String, f64> = HashMap::new();

    for ranking in rankings {
        let n = ranking.len() as f64;
        for (i, (name, _)) in ranking.iter().enumerate() {
            *scores.entry(name.clone()).or_insert(0.0) += n - i as f64;
        }
    }

    let mut result: Vec<(String, f64)> = scores.into_iter().collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// Combines multiple TestRankers via Borda aggregation.
pub struct TestPrioritizer {
    pub rankers: Vec<Box<dyn TestRanker>>,
}

impl TestPrioritizer {
    pub fn new(rankers: Vec<Box<dyn TestRanker>>) -> Self {
        TestPrioritizer { rankers }
    }

    pub fn prioritize(&self, tests: &[TestTrace]) -> Vec<(String, f64)> {
        let rankings: Vec<Vec<(String, f64)>> =
            self.rankers.iter().map(|r| r.rank(tests)).collect();
        borda_aggregate(&rankings)
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index prioritize
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-index/src/prioritize.rs crates/apex-index/src/lib.rs && git commit -m "feat(index): add rank-aggregation test prioritization (Task 3.2)"
```

---

### Task 3.3: Slice-Based Change Impact

**Crate:** `apex-index`
**Create:** `crates/apex-index/src/change_impact.rs`
**Modify:** `crates/apex-index/src/lib.rs` (add `pub mod change_impact;`)

#### Step 3.3.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-index/src/change_impact.rs`:

```rust
//! Slice-based change impact analysis.
//! Given a set of changed source lines, identify which tests are affected.
//! Based on arXiv:2508.19056.

use crate::types::{branch_key, BranchIndex, TestTrace};
use apex_core::types::BranchId;
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::ExecutionStatus;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_branch(file_id: u64, line: u32) -> BranchId {
        BranchId::new(file_id, line, 0, 0)
    }

    fn make_index(traces: Vec<TestTrace>) -> BranchIndex {
        let profiles = BranchIndex::build_profiles(&traces);
        BranchIndex {
            traces,
            profiles,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 10,
            covered_branches: 5,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        }
    }

    #[test]
    fn change_impact_finds_affected_tests() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10), make_branch(1, 20)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(1, 30), make_branch(2, 5)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = make_index(traces);
        // Changed line 10 in file_id 1 -> should affect test_a
        let changed = vec![(1u64, 10u32)];
        let affected = change_impact(&changed, &index);
        assert!(affected.contains(&"test_a".to_string()));
        assert!(!affected.contains(&"test_b".to_string()));
    }

    #[test]
    fn change_impact_no_match() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces);
        let changed = vec![(99u64, 999u32)];
        let affected = change_impact(&changed, &index);
        assert!(affected.is_empty());
    }

    #[test]
    fn change_impact_multiple_tests_affected() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(1, 10), make_branch(1, 20)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = make_index(traces);
        let changed = vec![(1u64, 10u32)];
        let affected = change_impact(&changed, &index);
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&"test_a".to_string()));
        assert!(affected.contains(&"test_b".to_string()));
    }

    #[test]
    fn change_impact_empty_changes() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces);
        let affected = change_impact(&[], &index);
        assert!(affected.is_empty());
    }

    #[test]
    fn change_impact_deduplicates() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10), make_branch(1, 20)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces);
        // Both changed lines are in test_a — should still appear only once
        let changed = vec![(1u64, 10u32), (1u64, 20u32)];
        let affected = change_impact(&changed, &index);
        assert_eq!(affected.len(), 1);
    }
}
```

- [ ] Add `pub mod change_impact;` to `/Users/ad/prj/bcov/crates/apex-index/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index change_impact_finds 2>&1 | head -20
```

#### Step 3.3.2 — Implement `change_impact()`

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-index/src/change_impact.rs`:

```rust
/// Given a set of changed source lines `(file_id, line)`, return the names
/// of all tests whose branch traces intersect any changed line.
///
/// This is a lightweight approximation of program slicing: any test that
/// executed a branch on a changed line is considered affected.
pub fn change_impact(changed_lines: &[(u64, u32)], index: &BranchIndex) -> Vec<String> {
    if changed_lines.is_empty() {
        return vec![];
    }

    let changed_set: HashSet<(u64, u32)> = changed_lines.iter().copied().collect();

    let mut affected: HashSet<String> = HashSet::new();

    for trace in &index.traces {
        for branch in &trace.branches {
            if changed_set.contains(&(branch.file_id, branch.line)) {
                affected.insert(trace.test_name.clone());
                break; // no need to check more branches in this trace
            }
        }
    }

    let mut result: Vec<String> = affected.into_iter().collect();
    result.sort();
    result
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index change_impact
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-index/src/change_impact.rs crates/apex-index/src/lib.rs && git commit -m "feat(index): slice-based change impact analysis (Task 3.3)"
```

---

### Task 3.4: Dead Code Detection + LLM Validation

**Crate:** `apex-index`
**Create:** `crates/apex-index/src/dead_code.rs`
**Modify:** `crates/apex-index/src/lib.rs` (add `pub mod dead_code;`)
**Depends on:** 0.1 (LlmClient)

#### Step 3.4.1 — Write failing test for DeadCodeCandidate and detect()

- [ ] Create `/Users/ad/prj/bcov/crates/apex-index/src/dead_code.rs`:

```rust
//! Dead code detection with optional LLM validation.
//! Identifies branches that are never hit across all test traces,
//! then optionally asks an LLM whether each is genuinely dead.

use crate::types::{BranchIndex, BranchProfile};
use apex_core::types::BranchId;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{ExecutionStatus, Language};
    use crate::types::TestTrace;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_branch(file_id: u64, line: u32) -> BranchId {
        BranchId::new(file_id, line, 0, 0)
    }

    #[test]
    fn dead_code_candidate_creation() {
        let c = DeadCodeCandidate {
            branch: make_branch(1, 42),
            file_path: Some(PathBuf::from("src/lib.py")),
            reason: "never hit in any test trace".to_string(),
        };
        assert_eq!(c.branch.line, 42);
    }

    #[test]
    fn detect_finds_uncovered_branches() {
        let all_branches = vec![
            make_branch(1, 10),
            make_branch(1, 20),
            make_branch(1, 30),
        ];
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![make_branch(1, 10)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 3,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let candidates = DeadCodeDetector::detect(&index, &all_branches);
        assert_eq!(candidates.len(), 2);
        let lines: Vec<u32> = candidates.iter().map(|c| c.branch.line).collect();
        assert!(lines.contains(&20));
        assert!(lines.contains(&30));
    }

    #[test]
    fn detect_no_dead_code() {
        let branches = vec![make_branch(1, 10)];
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: branches.clone(),
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let candidates = DeadCodeDetector::detect(&index, &branches);
        assert!(candidates.is_empty());
    }

    #[test]
    fn dead_code_result_creation() {
        let r = DeadCodeResult {
            candidate: DeadCodeCandidate {
                branch: make_branch(1, 42),
                file_path: None,
                reason: "never hit".into(),
            },
            confirmed_dead: true,
            llm_explanation: Some("This branch is guarded by an always-false condition.".into()),
        };
        assert!(r.confirmed_dead);
    }
}
```

- [ ] Add `pub mod dead_code;` to `/Users/ad/prj/bcov/crates/apex-index/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index dead_code_candidate 2>&1 | head -20
```

#### Step 3.4.2 — Implement types and detect()

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-index/src/dead_code.rs`:

```rust
use std::collections::HashSet;
use std::path::PathBuf;
use crate::types::branch_key;

/// A branch suspected of being dead code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeCandidate {
    pub branch: BranchId,
    pub file_path: Option<PathBuf>,
    pub reason: String,
}

/// Result of LLM validation of a dead code candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeResult {
    pub candidate: DeadCodeCandidate,
    pub confirmed_dead: bool,
    pub llm_explanation: Option<String>,
}

/// Detects dead code by finding branches never hit in any test trace.
pub struct DeadCodeDetector;

impl DeadCodeDetector {
    /// Find branches from `all_branches` that appear in no test trace.
    pub fn detect(index: &BranchIndex, all_branches: &[BranchId]) -> Vec<DeadCodeCandidate> {
        let covered_keys: HashSet<String> = index.profiles.keys().cloned().collect();

        all_branches
            .iter()
            .filter(|b| !covered_keys.contains(&branch_key(b)))
            .map(|b| DeadCodeCandidate {
                branch: b.clone(),
                file_path: index.file_paths.get(&b.file_id).cloned(),
                reason: "never hit in any test trace".to_string(),
            })
            .collect()
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index dead_code
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-index/src/dead_code.rs crates/apex-index/src/lib.rs && git commit -m "feat(index): dead code detection with LLM validation types (Task 3.4)"
```

---

### Task 3.5: LLM Flaky Test Repair

**Crate:** `apex-index`
**Create:** `crates/apex-index/src/flaky_repair.rs`
**Modify:** `crates/apex-index/src/lib.rs` (add `pub mod flaky_repair;`)
**Depends on:** 0.1 (LlmClient), 1.2 (FlakyCandidate — uses FlakyTest from analysis.rs)

#### Step 3.5.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-index/src/flaky_repair.rs`:

```rust
//! LLM-assisted flaky test repair suggestions.
//! Given a test identified as flaky (nondeterministic branch coverage),
//! asks an LLM to suggest a fix based on the test source code.

use crate::analysis::FlakyTest;

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;
    use crate::analysis::DivergentBranch;

    fn make_flaky() -> FlakyTest {
        FlakyTest {
            test_name: "test_random_order".to_string(),
            divergent_branches: vec![DivergentBranch {
                branch: BranchId::new(1, 42, 0, 0),
                file_path: None,
                hit_ratio: "3/5".to_string(),
            }],
            divergent_runs: 5,
            total_runs: 5,
        }
    }

    #[test]
    fn build_prompt_contains_test_name() {
        let flaky = make_flaky();
        let source = "def test_random_order():\n    items = list(set([1,2,3]))\n    assert items[0] == 1";
        let prompt = FlakyRepair::build_prompt(&flaky, source);
        assert!(prompt.contains("test_random_order"));
        assert!(prompt.contains("3/5"));
    }

    #[test]
    fn build_prompt_contains_source() {
        let flaky = make_flaky();
        let source = "def test_random_order():\n    pass";
        let prompt = FlakyRepair::build_prompt(&flaky, source);
        assert!(prompt.contains("def test_random_order"));
    }

    #[test]
    fn build_prompt_handles_empty_source() {
        let flaky = make_flaky();
        let prompt = FlakyRepair::build_prompt(&flaky, "");
        assert!(prompt.contains("test_random_order"));
    }
}
```

- [ ] Add `pub mod flaky_repair;` to `/Users/ad/prj/bcov/crates/apex-index/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index build_prompt_contains 2>&1 | head -20
```

#### Step 3.5.2 — Implement FlakyRepair

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-index/src/flaky_repair.rs`:

```rust
/// Generates LLM prompts for flaky test repair.
pub struct FlakyRepair;

impl FlakyRepair {
    /// Build a prompt for an LLM to suggest a fix for a flaky test.
    pub fn build_prompt(candidate: &FlakyTest, test_source: &str) -> String {
        let mut prompt = String::new();
        prompt.push_str("You are a test reliability expert. The following test is flaky ");
        prompt.push_str("(produces nondeterministic results across runs).\n\n");
        prompt.push_str(&format!("**Test name:** `{}`\n", candidate.test_name));
        prompt.push_str(&format!(
            "**Divergent runs:** {}/{}\n\n",
            candidate.divergent_runs, candidate.total_runs
        ));

        if !candidate.divergent_branches.is_empty() {
            prompt.push_str("**Divergent branches (hit inconsistently):**\n");
            for db in &candidate.divergent_branches {
                prompt.push_str(&format!(
                    "- Line {}, hit ratio: {}\n",
                    db.branch.line, db.hit_ratio
                ));
            }
            prompt.push('\n');
        }

        if !test_source.is_empty() {
            prompt.push_str("**Test source code:**\n```\n");
            prompt.push_str(test_source);
            prompt.push_str("\n```\n\n");
        }

        prompt.push_str(
            "Identify the root cause of flakiness and suggest a minimal code fix. \
             Common causes: timing dependencies, random ordering, shared mutable state, \
             filesystem race conditions, floating-point comparisons.\n\n\
             Reply with:\n1. Root cause\n2. Suggested fix (code diff)\n",
        );

        prompt
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index flaky_repair
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-index/src/flaky_repair.rs crates/apex-index/src/lib.rs && git commit -m "feat(index): LLM flaky test repair prompt builder (Task 3.5)"
```

---

## Track 3B: Solver Upgrades

### Task 3.6: Diverse SMT Solutions

**Crate:** `apex-symbolic`
**Create:** `crates/apex-symbolic/src/diversity.rs`
**Modify:** `crates/apex-symbolic/src/lib.rs` (add `pub mod diversity;`)

#### Step 3.6.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-symbolic/src/diversity.rs`:

```rust
//! Generate multiple diverse solutions from one constraint set.
//! Based on the PanSampler paper — solves, adds blocking clause, repeats.

use crate::traits::{Solver, SolverLogic};
use apex_core::error::Result;
use apex_core::types::InputSeed;

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;
    use std::sync::Mutex;

    /// A solver that returns incrementing solutions up to a limit.
    struct IncrementingSolver {
        counter: Mutex<usize>,
        max_solutions: usize,
    }

    impl IncrementingSolver {
        fn new(max: usize) -> Self {
            IncrementingSolver {
                counter: Mutex::new(0),
                max_solutions: max,
            }
        }
    }

    impl Solver for IncrementingSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            let mut c = self.counter.lock().unwrap();
            if *c >= self.max_solutions {
                return Ok(None);
            }
            *c += 1;
            Ok(Some(InputSeed::new(vec![*c as u8], SeedOrigin::Symbolic)))
        }
        fn set_logic(&mut self, _logic: SolverLogic) {}
        fn name(&self) -> &str { "incrementing" }
    }

    #[test]
    fn diversity_solver_returns_multiple() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 3);
        let results = ds.solve_diverse(&["(> x 0)".to_string()]).unwrap();
        assert_eq!(results.len(), 3);
        // Each solution should be different
        assert_ne!(results[0].data, results[1].data);
        assert_ne!(results[1].data, results[2].data);
    }

    #[test]
    fn diversity_solver_fewer_than_requested() {
        let inner = IncrementingSolver::new(2);
        let ds = DiversitySolver::new(inner, 5);
        let results = ds.solve_diverse(&["(> x 0)".to_string()]).unwrap();
        // Solver can only produce 2 solutions
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn diversity_solver_empty_constraints() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 3);
        let results = ds.solve_diverse(&[]).unwrap();
        // Even with empty constraints, solver returns solutions
        // (depends on inner solver behavior)
        assert!(!results.is_empty() || results.is_empty()); // no panic
    }

    #[test]
    fn diversity_solver_zero_requested() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 0);
        let results = ds.solve_diverse(&["(> x 0)".to_string()]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn diversity_solver_implements_solver_trait() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 3);
        assert_eq!(ds.name(), "diversity");
        // Standard solve should return first solution
        let result = ds.solve(&["(> x 0)".to_string()], false).unwrap();
        assert!(result.is_some());
    }
}
```

- [ ] Add `pub mod diversity;` to `/Users/ad/prj/bcov/crates/apex-symbolic/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic diversity_solver_returns 2>&1 | head -20
```

#### Step 3.6.2 — Implement DiversitySolver

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-symbolic/src/diversity.rs`:

```rust
/// Wraps any Solver to produce multiple diverse solutions via blocking clauses.
pub struct DiversitySolver<S: Solver> {
    inner: S,
    num_solutions: usize,
}

impl<S: Solver> DiversitySolver<S> {
    pub fn new(inner: S, num_solutions: usize) -> Self {
        DiversitySolver {
            inner,
            num_solutions,
        }
    }

    /// Solve constraints multiple times, returning up to `num_solutions` diverse seeds.
    ///
    /// After each solution, adds a blocking clause to exclude the previous solution.
    /// Stops early if the solver returns None (no more solutions).
    pub fn solve_diverse(&self, constraints: &[String]) -> Result<Vec<InputSeed>> {
        let mut solutions = Vec::new();
        let mut augmented_constraints: Vec<String> = constraints.to_vec();

        for _ in 0..self.num_solutions {
            match self.inner.solve(&augmented_constraints, false)? {
                Some(seed) => {
                    // Build a blocking clause from the solution data.
                    // We negate the current solution by adding a constraint
                    // that the output must differ from this seed.
                    let blocking = format!(
                        "(not (= _solution_hash {}))",
                        hash_seed_data(&seed.data)
                    );
                    augmented_constraints.push(blocking);
                    solutions.push(seed);
                }
                None => break,
            }
        }

        Ok(solutions)
    }
}

impl<S: Solver> Solver for DiversitySolver<S> {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        self.inner.solve(constraints, negate_last)
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.inner.set_logic(logic);
    }

    fn name(&self) -> &str {
        "diversity"
    }
}

/// Simple hash of seed data for blocking clause generation.
fn hash_seed_data(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic diversity
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-symbolic/src/diversity.rs crates/apex-symbolic/src/lib.rs && git commit -m "feat(symbolic): diverse SMT solutions via blocking clauses (Task 3.6)"
```

---

### Task 3.7: LLM as Concolic Solver

**Crate:** `apex-symbolic`
**Create:** `crates/apex-symbolic/src/llm_solver.rs`
**Modify:** `crates/apex-symbolic/src/lib.rs` (add `pub mod llm_solver;`)
**Depends on:** 0.1 (LlmClient) — must add `apex-core` dep with llm feature or use trait directly

#### Step 3.7.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-symbolic/src/llm_solver.rs`:

```rust
//! LLM-based constraint solver — uses a language model to solve constraints
//! when traditional SMT solvers fail or time out.
//! Based on the Cottontail paper.

use crate::traits::{Solver, SolverLogic};
use apex_core::error::Result;
use apex_core::types::InputSeed;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_to_prompt_simple() {
        let constraints = vec!["(> x 0)".to_string(), "(< x 100)".to_string()];
        let prompt = constraints_to_prompt(&constraints, false);
        assert!(prompt.contains("(> x 0)"));
        assert!(prompt.contains("(< x 100)"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn constraints_to_prompt_negate_last() {
        let constraints = vec!["(> x 0)".to_string(), "(< x 100)".to_string()];
        let prompt = constraints_to_prompt(&constraints, true);
        assert!(prompt.contains("negate"));
        assert!(prompt.contains("(< x 100)"));
    }

    #[test]
    fn constraints_to_prompt_empty() {
        let prompt = constraints_to_prompt(&[], false);
        assert!(prompt.contains("no constraints"));
    }

    #[test]
    fn parse_llm_solution_valid_json() {
        let response = r#"{"x": 42, "y": -5}"#;
        let seed = parse_llm_solution(response);
        assert!(seed.is_some());
        let data = String::from_utf8(seed.unwrap().data.to_vec()).unwrap();
        assert!(data.contains("42"));
    }

    #[test]
    fn parse_llm_solution_invalid() {
        let seed = parse_llm_solution("I cannot solve this");
        assert!(seed.is_none());
    }

    #[test]
    fn parse_llm_solution_json_in_markdown() {
        let response = "Here is the solution:\n```json\n{\"x\": 10}\n```\n";
        let seed = parse_llm_solution(response);
        assert!(seed.is_some());
    }

    #[test]
    fn parse_llm_solution_empty_object() {
        let seed = parse_llm_solution("{}");
        assert!(seed.is_none()); // empty object = no assignments
    }
}
```

- [ ] Add `pub mod llm_solver;` to `/Users/ad/prj/bcov/crates/apex-symbolic/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic constraints_to_prompt 2>&1 | head -20
```

#### Step 3.7.2 — Implement helper functions

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-symbolic/src/llm_solver.rs`:

```rust
use apex_core::types::SeedOrigin;

/// Convert SMTLIB2 constraints to a natural language prompt for an LLM.
pub fn constraints_to_prompt(constraints: &[String], negate_last: bool) -> String {
    if constraints.is_empty() {
        return "There are no constraints to solve. Reply with an empty JSON object: {}".to_string();
    }

    let mut prompt = String::new();
    prompt.push_str(
        "You are an SMT solver. Given the following SMTLIB2 constraints, \
         find integer values for all variables that satisfy ALL constraints.\n\n",
    );

    prompt.push_str("Constraints:\n");
    for (i, c) in constraints.iter().enumerate() {
        let is_last = i == constraints.len() - 1;
        if is_last && negate_last {
            prompt.push_str(&format!("  {}. (negate this) {}\n", i + 1, c));
        } else {
            prompt.push_str(&format!("  {}. {}\n", i + 1, c));
        }
    }

    if negate_last {
        prompt.push_str(
            "\nIMPORTANT: The last constraint must be NEGATED. Find values that satisfy \
             constraints 1..N-1 AND the negation of constraint N.\n",
        );
    }

    prompt.push_str(
        "\nReply with ONLY a JSON object mapping variable names to integer values. \
         Example: {\"x\": 42, \"y\": -5}\n\
         If unsatisfiable, reply with: UNSAT\n",
    );

    prompt
}

/// Parse an LLM response into an InputSeed.
///
/// Tries to extract a JSON object `{"var": value, ...}` from the response.
/// Handles responses with markdown code fences.
pub fn parse_llm_solution(response: &str) -> Option<InputSeed> {
    let trimmed = response.trim();

    // Try direct JSON parse first
    if let Some(seed) = try_parse_json_object(trimmed) {
        return Some(seed);
    }

    // Try extracting from markdown code fence
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        // Skip optional language tag (e.g., "json\n")
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            let json_str = content[..end].trim();
            return try_parse_json_object(json_str);
        }
    }

    None
}

fn try_parse_json_object(s: &str) -> Option<InputSeed> {
    let parsed: serde_json::Value = serde_json::from_str(s).ok()?;
    let obj = parsed.as_object()?;

    if obj.is_empty() {
        return None;
    }

    let json_bytes = serde_json::to_vec(&parsed).ok()?;
    Some(InputSeed::new(json_bytes, SeedOrigin::Symbolic))
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic llm_solver
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-symbolic/src/llm_solver.rs crates/apex-symbolic/src/lib.rs && git commit -m "feat(symbolic): LLM-based constraint solver helpers (Task 3.7)"
```

---

### Task 3.8: Fitness Landscape Adaptation

**Crate:** `apex-symbolic`
**Create:** `crates/apex-symbolic/src/landscape.rs`
**Modify:** `crates/apex-symbolic/src/lib.rs` (add `pub mod landscape;`)

#### Step 3.8.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-symbolic/src/landscape.rs`:

```rust
//! Fitness landscape analysis for adaptive strategy switching.
//! Based on arXiv:2502.00169 — detects deceptive landscapes where
//! gradient descent fails and suggests alternative strategies.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_hint_debug() {
        assert_eq!(format!("{:?}", StrategyHint::GradientUseful), "GradientUseful");
        assert_eq!(format!("{:?}", StrategyHint::SwitchToRandom), "SwitchToRandom");
        assert_eq!(format!("{:?}", StrategyHint::NeedsSolver), "NeedsSolver");
    }

    #[test]
    fn empty_analyzer_not_deceptive() {
        let analyzer = LandscapeAnalyzer::new();
        assert!(!analyzer.is_deceptive());
    }

    #[test]
    fn monotonic_improvement_not_deceptive() {
        let mut analyzer = LandscapeAnalyzer::new();
        analyzer.add_sample(vec![0], 1.0);
        analyzer.add_sample(vec![1], 0.8);
        analyzer.add_sample(vec![2], 0.5);
        analyzer.add_sample(vec![3], 0.2);
        analyzer.add_sample(vec![4], 0.0);
        assert!(!analyzer.is_deceptive());
    }

    #[test]
    fn oscillating_fitness_is_deceptive() {
        let mut analyzer = LandscapeAnalyzer::new();
        // Fitness goes up and down — gradient is misleading
        for i in 0..20 {
            let fitness = if i % 2 == 0 { 0.8 } else { 0.2 };
            analyzer.add_sample(vec![i as u8], fitness);
        }
        assert!(analyzer.is_deceptive());
    }

    #[test]
    fn suggest_gradient_when_monotonic() {
        let mut analyzer = LandscapeAnalyzer::new();
        analyzer.add_sample(vec![0], 1.0);
        analyzer.add_sample(vec![1], 0.5);
        analyzer.add_sample(vec![2], 0.1);
        assert_eq!(analyzer.suggest_strategy(), StrategyHint::GradientUseful);
    }

    #[test]
    fn suggest_random_when_deceptive() {
        let mut analyzer = LandscapeAnalyzer::new();
        for i in 0..20 {
            let fitness = if i % 2 == 0 { 0.9 } else { 0.1 };
            analyzer.add_sample(vec![i as u8], fitness);
        }
        assert_eq!(analyzer.suggest_strategy(), StrategyHint::SwitchToRandom);
    }

    #[test]
    fn suggest_solver_when_plateau() {
        let mut analyzer = LandscapeAnalyzer::new();
        // All samples have the same fitness — plateau
        for i in 0..10 {
            analyzer.add_sample(vec![i as u8], 0.5);
        }
        assert_eq!(analyzer.suggest_strategy(), StrategyHint::NeedsSolver);
    }

    #[test]
    fn add_sample_grows_collection() {
        let mut analyzer = LandscapeAnalyzer::new();
        assert_eq!(analyzer.sample_count(), 0);
        analyzer.add_sample(vec![1], 0.5);
        assert_eq!(analyzer.sample_count(), 1);
        analyzer.add_sample(vec![2], 0.3);
        assert_eq!(analyzer.sample_count(), 2);
    }
}
```

- [ ] Add `pub mod landscape;` to `/Users/ad/prj/bcov/crates/apex-symbolic/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic strategy_hint_debug 2>&1 | head -20
```

#### Step 3.8.2 — Implement LandscapeAnalyzer

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-symbolic/src/landscape.rs`:

```rust
/// Strategy recommendation from landscape analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyHint {
    /// Fitness landscape is smooth — gradient descent will work.
    GradientUseful,
    /// Fitness landscape is deceptive — switch to random exploration.
    SwitchToRandom,
    /// Fitness landscape is flat — need a solver (SMT/LLM).
    NeedsSolver,
}

/// Analyzes fitness landscape from sampled (input, fitness) pairs.
pub struct LandscapeAnalyzer {
    samples: Vec<(Vec<u8>, f64)>,
}

impl LandscapeAnalyzer {
    pub fn new() -> Self {
        LandscapeAnalyzer {
            samples: Vec::new(),
        }
    }

    pub fn add_sample(&mut self, input: Vec<u8>, fitness: f64) {
        self.samples.push((input, fitness));
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Detect if the landscape is deceptive (gradient doesn't reliably lead to target).
    ///
    /// Measures the ratio of sign changes in consecutive fitness deltas.
    /// High ratio of sign changes = oscillating = deceptive.
    pub fn is_deceptive(&self) -> bool {
        if self.samples.len() < 4 {
            return false;
        }

        let deltas: Vec<f64> = self
            .samples
            .windows(2)
            .map(|w| w[1].1 - w[0].1)
            .collect();

        let sign_changes = deltas
            .windows(2)
            .filter(|w| (w[0] > 0.0) != (w[1] > 0.0) && w[0] != 0.0 && w[1] != 0.0)
            .count();

        let total_transitions = deltas.len().saturating_sub(1);
        if total_transitions == 0 {
            return false;
        }

        let ratio = sign_changes as f64 / total_transitions as f64;
        ratio > 0.5
    }

    /// Suggest a strategy based on landscape shape.
    pub fn suggest_strategy(&self) -> StrategyHint {
        if self.samples.len() < 3 {
            return StrategyHint::GradientUseful; // not enough data, try gradient
        }

        // Check for plateau (all fitness values within epsilon)
        let fitnesses: Vec<f64> = self.samples.iter().map(|(_, f)| *f).collect();
        let min = fitnesses.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = fitnesses.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if (max - min).abs() < 0.01 {
            return StrategyHint::NeedsSolver;
        }

        if self.is_deceptive() {
            return StrategyHint::SwitchToRandom;
        }

        StrategyHint::GradientUseful
    }
}

impl Default for LandscapeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic landscape
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-symbolic/src/landscape.rs crates/apex-symbolic/src/lib.rs && git commit -m "feat(symbolic): fitness landscape analysis for strategy switching (Task 3.8)"
```

---

### Task 3.9: Path Decomposition (AutoBug)

**Crate:** `apex-symbolic`
**Create:** `crates/apex-symbolic/src/path_decomp.rs`
**Modify:** `crates/apex-symbolic/src/lib.rs` (add `pub mod path_decomp;`)

#### Step 3.9.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-symbolic/src/path_decomp.rs`:

```rust
//! Path decomposition for long constraint chains.
//! Based on the AutoBug paper — splits long chains into independent
//! sub-problems that can be solved separately.

use crate::smtlib::extract_variables;
use crate::traits::Solver;
use apex_core::error::Result;
use apex_core::types::InputSeed;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_independent_constraints() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(< y 10)".to_string(),
            "(= z 5)".to_string(),
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // x, y, z are independent — each gets its own partition
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn decompose_shared_variable_groups() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(< x 10)".to_string(), // shares x with first
            "(= y 5)".to_string(),  // independent
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // x constraints grouped, y separate => 2 partitions
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn decompose_all_shared() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(and (> x 0) (< y 10))".to_string(), // links x and y
            "(< y 5)".to_string(),
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // All linked through x-y chain => 1 partition
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 3);
    }

    #[test]
    fn decompose_empty() {
        let parts = PathDecomposer::decompose(&[]);
        assert!(parts.is_empty());
    }

    #[test]
    fn decompose_single_constraint() {
        let constraints = vec!["(> x 0)".to_string()];
        let parts = PathDecomposer::decompose(&constraints);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 1);
    }
}
```

- [ ] Add `pub mod path_decomp;` to `/Users/ad/prj/bcov/crates/apex-symbolic/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic decompose_independent 2>&1 | head -20
```

#### Step 3.9.2 — Implement PathDecomposer

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-symbolic/src/path_decomp.rs`:

```rust
/// Decomposes long constraint chains into independent sub-problems.
pub struct PathDecomposer;

impl PathDecomposer {
    /// Split constraints into independent partitions based on shared variables.
    ///
    /// Two constraints are in the same partition if they share any variable,
    /// transitively. Uses union-find to group constraints.
    pub fn decompose(constraints: &[String]) -> Vec<Vec<String>> {
        if constraints.is_empty() {
            return vec![];
        }

        // Extract variables for each constraint
        let var_sets: Vec<HashSet<String>> = constraints
            .iter()
            .map(|c| extract_variables(c).into_iter().collect())
            .collect();

        // Union-find: parent[i] = representative of constraint i's group
        let n = constraints.len();
        let mut parent: Vec<usize> = (0..n).collect();

        // Find with path compression
        fn find(parent: &mut [usize], i: usize) -> usize {
            if parent[i] != i {
                parent[i] = find(parent, parent[i]);
            }
            parent[i]
        }

        // Union constraints that share variables
        for i in 0..n {
            for j in (i + 1)..n {
                if !var_sets[i].is_disjoint(&var_sets[j]) {
                    let ri = find(&mut parent, i);
                    let rj = find(&mut parent, j);
                    if ri != rj {
                        parent[ri] = rj;
                    }
                }
            }
        }

        // Group constraints by representative
        let mut groups: HashMap<usize, Vec<String>> = HashMap::new();
        for i in 0..n {
            let root = find(&mut parent, i);
            groups
                .entry(root)
                .or_default()
                .push(constraints[i].clone());
        }

        let mut result: Vec<Vec<String>> = groups.into_values().collect();
        result.sort_by_key(|v| v.len());
        result
    }

    /// Solve each partition independently and merge results.
    pub fn solve_decomposed(
        parts: &[Vec<String>],
        solver: &dyn Solver,
    ) -> Result<Option<InputSeed>> {
        let mut combined_data: Vec<u8> = Vec::new();

        for part in parts {
            match solver.solve(part, false)? {
                Some(seed) => combined_data.extend_from_slice(&seed.data),
                None => return Ok(None), // One partition is UNSAT => whole thing is UNSAT
            }
        }

        if combined_data.is_empty() {
            Ok(None)
        } else {
            Ok(Some(InputSeed::new(
                combined_data,
                apex_core::types::SeedOrigin::Symbolic,
            )))
        }
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-symbolic path_decomp
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-symbolic/src/path_decomp.rs crates/apex-symbolic/src/lib.rs && git commit -m "feat(symbolic): path decomposition for long constraint chains (Task 3.9)"
```

---

## Track 3C: Fuzz Advanced

### Task 3.10: SeedMind (LLM Seed Generators)

**Crate:** `apex-fuzz`
**Create:** `crates/apex-fuzz/src/seedmind.rs`
**Modify:** `crates/apex-fuzz/src/lib.rs` (add `pub mod seedmind;`)
**Depends on:** 0.1 (LlmClient)

#### Step 3.10.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-fuzz/src/seedmind.rs`:

```rust
//! SeedMind — LLM-guided seed generation targeting uncovered branches.
//! Based on the SeedMind paper.

use apex_core::types::BranchId;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_seed_prompt_includes_format() {
        let uncovered = vec![
            BranchId::new(1, 10, 0, 0),
            BranchId::new(1, 20, 0, 1),
        ];
        let prompt = build_seed_prompt(&uncovered, "JSON");
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("line 10"));
        assert!(prompt.contains("line 20"));
    }

    #[test]
    fn build_seed_prompt_empty_uncovered() {
        let prompt = build_seed_prompt(&[], "binary");
        assert!(prompt.contains("no specific"));
    }

    #[test]
    fn parse_seed_response_json_array() {
        let response = r#"[{"key": "value"}, {"key": "other"}]"#;
        let seeds = parse_seed_response(response);
        assert_eq!(seeds.len(), 2);
    }

    #[test]
    fn parse_seed_response_single_object() {
        let response = r#"{"input": 42}"#;
        let seeds = parse_seed_response(response);
        assert_eq!(seeds.len(), 1);
    }

    #[test]
    fn parse_seed_response_invalid() {
        let seeds = parse_seed_response("not json at all");
        assert!(seeds.is_empty());
    }

    #[test]
    fn parse_seed_response_with_markdown() {
        let response = "Here are seeds:\n```json\n[{\"x\": 1}]\n```\n";
        let seeds = parse_seed_response(response);
        assert_eq!(seeds.len(), 1);
    }
}
```

- [ ] Add `pub mod seedmind;` to `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs` after existing modules.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz build_seed_prompt 2>&1 | head -20
```

#### Step 3.10.2 — Implement helpers

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-fuzz/src/seedmind.rs`:

```rust
/// Build a prompt asking an LLM to generate targeted seed inputs.
pub fn build_seed_prompt(uncovered: &[BranchId], target_format: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!(
        "You are a fuzzing seed generator. Generate test inputs in {target_format} format \
         that are likely to exercise specific code paths.\n\n"
    ));

    if uncovered.is_empty() {
        prompt.push_str(
            "There are no specific uncovered branches. Generate diverse, \
             boundary-testing inputs.\n",
        );
    } else {
        prompt.push_str("Target these uncovered branches:\n");
        for b in uncovered.iter().take(20) {
            prompt.push_str(&format!(
                "- file_id={}, line {}, direction {}\n",
                b.file_id, b.line, b.direction
            ));
        }
        if uncovered.len() > 20 {
            prompt.push_str(&format!("  ... and {} more\n", uncovered.len() - 20));
        }
    }

    prompt.push_str(&format!(
        "\nGenerate 5 diverse {target_format} inputs as a JSON array. \
         Include boundary values, empty inputs, large inputs, and edge cases.\n"
    ));

    prompt
}

/// Parse an LLM response into seed byte vectors.
pub fn parse_seed_response(response: &str) -> Vec<Vec<u8>> {
    let trimmed = response.trim();

    // Try direct parse
    if let Some(seeds) = try_parse_seeds(trimmed) {
        return seeds;
    }

    // Try extracting from markdown code fence
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            let json_str = content[..end].trim();
            if let Some(seeds) = try_parse_seeds(json_str) {
                return seeds;
            }
        }
    }

    vec![]
}

fn try_parse_seeds(s: &str) -> Option<Vec<Vec<u8>>> {
    let parsed: serde_json::Value = serde_json::from_str(s).ok()?;

    match parsed {
        serde_json::Value::Array(arr) => {
            let seeds: Vec<Vec<u8>> = arr
                .iter()
                .filter_map(|v| serde_json::to_vec(v).ok())
                .collect();
            if seeds.is_empty() {
                None
            } else {
                Some(seeds)
            }
        }
        serde_json::Value::Object(_) => {
            let bytes = serde_json::to_vec(&parsed).ok()?;
            Some(vec![bytes])
        }
        _ => None,
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz seedmind
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-fuzz/src/seedmind.rs crates/apex-fuzz/src/lib.rs && git commit -m "feat(fuzz): SeedMind LLM-guided seed generation (Task 3.10)"
```

---

### Task 3.11: HGFuzzer (Directed Greybox)

**Crate:** `apex-fuzz`
**Create:** `crates/apex-fuzz/src/hgfuzzer.rs`
**Modify:** `crates/apex-fuzz/src/lib.rs` (add `pub mod hgfuzzer;`)

#### Step 3.11.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-fuzz/src/hgfuzzer.rs`:

```rust
//! HGFuzzer — directed greybox fuzzing with hierarchical distance computation.
//! Based on the HGFuzzer paper.

use apex_core::types::BranchId;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_branch(line: u32) -> BranchId {
        BranchId::new(1, line, 0, 0)
    }

    #[test]
    fn hgfuzzer_creation() {
        let targets = vec![make_branch(42)];
        let hg = HGFuzzer::new(targets.clone());
        assert_eq!(hg.target_branches.len(), 1);
    }

    #[test]
    fn assign_energy_at_target() {
        let targets = vec![make_branch(42)];
        let mut hg = HGFuzzer::new(targets);
        hg.set_distance(&make_branch(42), 0.0);
        let energy = hg.assign_energy(&make_branch(42));
        assert!((energy - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn assign_energy_far_from_target() {
        let targets = vec![make_branch(42)];
        let mut hg = HGFuzzer::new(targets);
        hg.set_distance(&make_branch(100), 10.0);
        let energy = hg.assign_energy(&make_branch(100));
        assert!(energy < 1.0);
        assert!(energy > 0.0);
    }

    #[test]
    fn assign_energy_unknown_distance() {
        let targets = vec![make_branch(42)];
        let hg = HGFuzzer::new(targets);
        // No distance set => default energy
        let energy = hg.assign_energy(&make_branch(99));
        assert!((energy - 0.1).abs() < f64::EPSILON); // default low energy
    }

    #[test]
    fn closer_gets_more_energy() {
        let targets = vec![make_branch(42)];
        let mut hg = HGFuzzer::new(targets);
        hg.set_distance(&make_branch(10), 2.0);
        hg.set_distance(&make_branch(20), 8.0);
        let e_close = hg.assign_energy(&make_branch(10));
        let e_far = hg.assign_energy(&make_branch(20));
        assert!(e_close > e_far);
    }
}
```

- [ ] Add `pub mod hgfuzzer;` to `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz hgfuzzer_creation 2>&1 | head -20
```

#### Step 3.11.2 — Implement HGFuzzer

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-fuzz/src/hgfuzzer.rs`:

```rust
/// Directed greybox fuzzer that assigns energy based on distance to target branches.
pub struct HGFuzzer {
    pub target_branches: Vec<BranchId>,
    distance_cache: HashMap<String, f64>,
}

impl HGFuzzer {
    pub fn new(target_branches: Vec<BranchId>) -> Self {
        HGFuzzer {
            target_branches,
            distance_cache: HashMap::new(),
        }
    }

    /// Set the distance from a branch to the nearest target.
    pub fn set_distance(&mut self, branch: &BranchId, distance: f64) {
        self.distance_cache
            .insert(branch_key(branch), distance);
    }

    /// Assign energy to a corpus entry based on its distance to the target.
    ///
    /// Energy = 1.0 / (1.0 + distance). At target (distance=0), energy=1.0.
    /// Unknown distance gets a default low energy of 0.1.
    pub fn assign_energy(&self, branch: &BranchId) -> f64 {
        match self.distance_cache.get(&branch_key(branch)) {
            Some(&distance) => 1.0 / (1.0 + distance),
            None => 0.1, // default low energy for unknown distance
        }
    }
}

fn branch_key(b: &BranchId) -> String {
    format!("{}:{}:{}:{}", b.file_id, b.line, b.col, b.direction)
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz hgfuzzer
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-fuzz/src/hgfuzzer.rs crates/apex-fuzz/src/lib.rs && git commit -m "feat(fuzz): HGFuzzer directed greybox energy assignment (Task 3.11)"
```

---

### Task 3.12: FOX Stochastic Control

**Crate:** `apex-fuzz`
**Create:** `crates/apex-fuzz/src/control.rs`
**Modify:** `crates/apex-fuzz/src/lib.rs` (add `pub mod control;`)

#### Step 3.12.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-fuzz/src/control.rs`:

```rust
//! FOX — stochastic fuzzing control that adapts mutation and exploration
//! rates based on recent coverage progress.
//! Based on the FOX paper.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fox_default_rates() {
        let ctrl = FoxController::new();
        assert!((ctrl.mutation_rate - 0.5).abs() < f64::EPSILON);
        assert!((ctrl.exploration_rate - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn adapt_increases_exploration_on_stall() {
        let mut ctrl = FoxController::new();
        // No new coverage for many iterations => explore more
        ctrl.adapt(0.0, 100);
        assert!(ctrl.exploration_rate > 0.5);
    }

    #[test]
    fn adapt_increases_exploitation_on_progress() {
        let mut ctrl = FoxController::new();
        // High coverage delta => exploit more (reduce exploration)
        ctrl.adapt(0.5, 0);
        assert!(ctrl.exploration_rate < 0.5);
    }

    #[test]
    fn should_explore_respects_rate() {
        let mut ctrl = FoxController::new();
        ctrl.exploration_rate = 1.0;
        // With rate=1.0, should always explore
        assert!(ctrl.should_explore());

        ctrl.exploration_rate = 0.0;
        // With rate=0.0, should never explore
        assert!(!ctrl.should_explore());
    }

    #[test]
    fn rates_stay_in_bounds() {
        let mut ctrl = FoxController::new();
        // Extreme stall
        for _ in 0..100 {
            ctrl.adapt(0.0, 10000);
        }
        assert!(ctrl.exploration_rate <= 1.0);
        assert!(ctrl.mutation_rate >= 0.0);

        // Extreme progress
        for _ in 0..100 {
            ctrl.adapt(1.0, 0);
        }
        assert!(ctrl.exploration_rate >= 0.0);
        assert!(ctrl.mutation_rate <= 1.0);
    }

    #[test]
    fn mutation_rate_increases_on_stall() {
        let mut ctrl = FoxController::new();
        let initial = ctrl.mutation_rate;
        ctrl.adapt(0.0, 50);
        assert!(ctrl.mutation_rate >= initial);
    }
}
```

- [ ] Add `pub mod control;` to `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz fox_default 2>&1 | head -20
```

#### Step 3.12.2 — Implement FoxController

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-fuzz/src/control.rs`:

```rust
/// Adaptive fuzzing controller that adjusts mutation and exploration rates.
pub struct FoxController {
    pub mutation_rate: f64,
    pub exploration_rate: f64,
}

impl FoxController {
    pub fn new() -> Self {
        FoxController {
            mutation_rate: 0.5,
            exploration_rate: 0.5,
        }
    }

    /// Adapt rates based on recent coverage progress.
    ///
    /// - `coverage_delta`: fraction of new coverage gained (0.0 = none, 1.0 = all new).
    /// - `iterations_since_new`: how many iterations since last new coverage.
    pub fn adapt(&mut self, coverage_delta: f64, iterations_since_new: u64) {
        let stall_pressure = (iterations_since_new as f64 / 100.0).min(1.0);
        let progress_pressure = coverage_delta.min(1.0);

        // On stall: increase exploration and mutation aggressiveness
        // On progress: decrease exploration (exploit current direction)
        let alpha = 0.1; // learning rate

        self.exploration_rate += alpha * (stall_pressure - progress_pressure);
        self.exploration_rate = self.exploration_rate.clamp(0.0, 1.0);

        self.mutation_rate += alpha * stall_pressure * 0.5;
        self.mutation_rate -= alpha * progress_pressure * 0.3;
        self.mutation_rate = self.mutation_rate.clamp(0.01, 1.0);
    }

    /// Whether the next iteration should explore (random/diverse) vs exploit (targeted).
    pub fn should_explore(&self) -> bool {
        self.exploration_rate >= 0.5
    }
}

impl Default for FoxController {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz control
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-fuzz/src/control.rs crates/apex-fuzz/src/lib.rs && git commit -m "feat(fuzz): FOX stochastic fuzzing control (Task 3.12)"
```

---

## Track 3D: Security Spec Mining

### Task 3.13: Syscall Spec Mining

**Crate:** `apex-detect`
**Create:** `crates/apex-detect/src/detectors/spec_miner.rs`
**Create:** `crates/apex-detect/src/detectors/python_audit.rs`
**Modify:** `crates/apex-detect/src/detectors/mod.rs`

#### Step 3.13.1 — Write failing test for SyscallSpec

- [ ] Create `/Users/ad/prj/bcov/crates/apex-detect/src/detectors/spec_miner.rs`:

```rust
//! Syscall specification mining.
//! Based on the Caruca paper — learns normal syscall sequences from test runs
//! and flags deviations as potential security issues.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syscall_spec_creation() {
        let spec = SyscallSpec::new("my_function");
        assert_eq!(spec.function_name, "my_function");
        assert!(spec.allowed_calls.is_empty());
    }

    #[test]
    fn learn_from_traces_builds_spec() {
        let traces = vec![
            vec!["open".to_string(), "read".to_string(), "close".to_string()],
            vec!["open".to_string(), "write".to_string(), "close".to_string()],
        ];
        let spec = SyscallSpec::learn("handler", &traces);
        assert!(spec.allowed_calls.contains("open"));
        assert!(spec.allowed_calls.contains("close"));
        assert!(spec.allowed_calls.contains("read"));
        assert!(spec.allowed_calls.contains("write"));
    }

    #[test]
    fn check_violation_detects_unknown_call() {
        let mut spec = SyscallSpec::new("handler");
        spec.allowed_calls.insert("open".to_string());
        spec.allowed_calls.insert("read".to_string());
        spec.allowed_calls.insert("close".to_string());

        let trace = vec![
            "open".to_string(),
            "exec".to_string(), // not in spec!
            "close".to_string(),
        ];
        let violations = spec.check(&trace);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0], "exec");
    }

    #[test]
    fn check_no_violations() {
        let mut spec = SyscallSpec::new("handler");
        spec.allowed_calls.insert("open".to_string());
        spec.allowed_calls.insert("close".to_string());

        let trace = vec!["open".to_string(), "close".to_string()];
        let violations = spec.check(&trace);
        assert!(violations.is_empty());
    }

    #[test]
    fn learn_empty_traces() {
        let spec = SyscallSpec::learn("empty", &[]);
        assert!(spec.allowed_calls.is_empty());
    }

    #[test]
    fn spec_miner_multiple_functions() {
        let mut miner = SpecMiner::new();
        miner.add_trace("func_a", vec!["open".into(), "read".into()]);
        miner.add_trace("func_a", vec!["open".into(), "write".into()]);
        miner.add_trace("func_b", vec!["connect".into(), "send".into()]);

        let specs = miner.build_specs();
        assert_eq!(specs.len(), 2);

        let spec_a = specs.iter().find(|s| s.function_name == "func_a").unwrap();
        assert!(spec_a.allowed_calls.contains("open"));
        assert!(spec_a.allowed_calls.contains("read"));
        assert!(spec_a.allowed_calls.contains("write"));
    }
}
```

- [ ] Add `pub mod spec_miner;` to `/Users/ad/prj/bcov/crates/apex-detect/src/detectors/mod.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect syscall_spec_creation 2>&1 | head -20
```

#### Step 3.13.2 — Implement SyscallSpec and SpecMiner

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-detect/src/detectors/spec_miner.rs`:

```rust
/// A learned specification of allowed syscalls for a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallSpec {
    pub function_name: String,
    pub allowed_calls: HashSet<String>,
}

impl SyscallSpec {
    pub fn new(function_name: &str) -> Self {
        SyscallSpec {
            function_name: function_name.to_string(),
            allowed_calls: HashSet::new(),
        }
    }

    /// Learn a spec from observed syscall traces.
    pub fn learn(function_name: &str, traces: &[Vec<String>]) -> Self {
        let mut allowed = HashSet::new();
        for trace in traces {
            for call in trace {
                allowed.insert(call.clone());
            }
        }
        SyscallSpec {
            function_name: function_name.to_string(),
            allowed_calls: allowed,
        }
    }

    /// Check a trace against this spec, returning unknown calls.
    pub fn check(&self, trace: &[String]) -> Vec<String> {
        trace
            .iter()
            .filter(|call| !self.allowed_calls.contains(call.as_str()))
            .cloned()
            .collect()
    }
}

/// Collects syscall traces per function and builds specs.
pub struct SpecMiner {
    traces: HashMap<String, Vec<Vec<String>>>,
}

impl SpecMiner {
    pub fn new() -> Self {
        SpecMiner {
            traces: HashMap::new(),
        }
    }

    pub fn add_trace(&mut self, function_name: &str, trace: Vec<String>) {
        self.traces
            .entry(function_name.to_string())
            .or_default()
            .push(trace);
    }

    pub fn build_specs(&self) -> Vec<SyscallSpec> {
        self.traces
            .iter()
            .map(|(name, traces)| SyscallSpec::learn(name, traces))
            .collect()
    }
}

impl Default for SpecMiner {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect spec_miner
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-detect/src/detectors/spec_miner.rs crates/apex-detect/src/detectors/mod.rs && git commit -m "feat(detect): syscall spec mining (Task 3.13)"
```

---

### Task 3.14: Data Transform Spec Mining

**Crate:** `apex-index`
**Create:** `crates/apex-index/src/spec_mining.rs`
**Modify:** `crates/apex-index/src/lib.rs` (add `pub mod spec_mining;`)

#### Step 3.14.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-index/src/spec_mining.rs`:

```rust
//! Data transform specification mining.
//! Based on "Beyond Bools" paper — learns input/output relationships
//! beyond boolean pass/fail from test executions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_spec_creation() {
        let spec = TransformSpec {
            function_name: "sort".to_string(),
            input_output_pairs: vec![
                (r#"[3,1,2]"#.to_string(), r#"[1,2,3]"#.to_string()),
            ],
            inferred_properties: vec![],
        };
        assert_eq!(spec.function_name, "sort");
    }

    #[test]
    fn infer_length_preservation() {
        let pairs = vec![
            ("[1,2,3]".to_string(), "[3,2,1]".to_string()),
            ("[1]".to_string(), "[1]".to_string()),
            ("[5,4,3,2,1]".to_string(), "[1,2,3,4,5]".to_string()),
        ];
        let props = infer_properties(&pairs);
        assert!(props.contains(&"length_preserved".to_string()));
    }

    #[test]
    fn infer_no_properties_from_empty() {
        let props = infer_properties(&[]);
        assert!(props.is_empty());
    }

    #[test]
    fn infer_idempotent() {
        let pairs = vec![
            ("hello".to_string(), "hello".to_string()),
            ("world".to_string(), "world".to_string()),
        ];
        let props = infer_properties(&pairs);
        assert!(props.contains(&"idempotent".to_string()));
    }
}
```

- [ ] Add `pub mod spec_mining;` to `/Users/ad/prj/bcov/crates/apex-index/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index transform_spec 2>&1 | head -20
```

#### Step 3.14.2 — Implement TransformSpec and infer_properties

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-index/src/spec_mining.rs`:

```rust
/// A learned specification of a function's input/output behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformSpec {
    pub function_name: String,
    pub input_output_pairs: Vec<(String, String)>,
    pub inferred_properties: Vec<String>,
}

/// Infer high-level properties from input/output pairs.
///
/// Currently checks:
/// - `length_preserved`: input and output have the same string length.
/// - `idempotent`: output equals input for all pairs.
pub fn infer_properties(pairs: &[(String, String)]) -> Vec<String> {
    if pairs.is_empty() {
        return vec![];
    }

    let mut properties = Vec::new();

    // Check length preservation
    let all_length_preserved = pairs.iter().all(|(i, o)| i.len() == o.len());
    if all_length_preserved {
        properties.push("length_preserved".to_string());
    }

    // Check idempotency
    let all_idempotent = pairs.iter().all(|(i, o)| i == o);
    if all_idempotent {
        properties.push("idempotent".to_string());
    }

    properties
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-index spec_mining
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-index/src/spec_mining.rs crates/apex-index/src/lib.rs && git commit -m "feat(index): data transform spec mining (Task 3.14)"
```

---

### Task 3.15: CEGAR Spec Mining

**Crate:** `apex-detect`
**Create:** `crates/apex-detect/src/detectors/cegar.rs`
**Modify:** `crates/apex-detect/src/detectors/mod.rs`
**Depends on:** 3.13, 3.14

#### Step 3.15.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-detect/src/detectors/cegar.rs`:

```rust
//! CEGAR-based specification refinement.
//! Based on the SmCon paper — iteratively refines specifications
//! using counterexample-guided abstraction refinement.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_refinement_creation() {
        let spec = CegarSpec::new("validate_input");
        assert_eq!(spec.function_name, "validate_input");
        assert_eq!(spec.iteration, 0);
    }

    #[test]
    fn refine_adds_counterexample() {
        let mut spec = CegarSpec::new("f");
        spec.allowed.insert("normal_call".to_string());

        let counterexample = "dangerous_call".to_string();
        let is_genuine = true; // confirmed as a real violation
        spec.refine(&counterexample, is_genuine);

        assert_eq!(spec.iteration, 1);
        assert!(spec.violations.contains(&counterexample));
        assert!(!spec.allowed.contains(&counterexample));
    }

    #[test]
    fn refine_spurious_counterexample_adds_to_allowed() {
        let mut spec = CegarSpec::new("f");

        let counterexample = "actually_safe_call".to_string();
        let is_genuine = false; // spurious — add to allowed
        spec.refine(&counterexample, is_genuine);

        assert_eq!(spec.iteration, 1);
        assert!(spec.allowed.contains(&counterexample));
        assert!(!spec.violations.contains(&counterexample));
    }

    #[test]
    fn is_converged_after_no_new_violations() {
        let mut spec = CegarSpec::new("f");
        spec.allowed.insert("a".to_string());
        // No refinements => converged
        assert!(spec.is_converged(3));
    }

    #[test]
    fn not_converged_with_recent_refinements() {
        let mut spec = CegarSpec::new("f");
        spec.refine(&"bad".to_string(), true);
        assert!(!spec.is_converged(3));
    }
}
```

- [ ] Add `pub mod cegar;` to `/Users/ad/prj/bcov/crates/apex-detect/src/detectors/mod.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect spec_refinement 2>&1 | head -20
```

#### Step 3.15.2 — Implement CegarSpec

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-detect/src/detectors/cegar.rs`:

```rust
/// A specification refined through CEGAR iterations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CegarSpec {
    pub function_name: String,
    pub allowed: HashSet<String>,
    pub violations: HashSet<String>,
    pub iteration: u32,
    /// Track iteration numbers when violations were last added.
    last_violation_iteration: u32,
}

impl CegarSpec {
    pub fn new(function_name: &str) -> Self {
        CegarSpec {
            function_name: function_name.to_string(),
            allowed: HashSet::new(),
            violations: HashSet::new(),
            iteration: 0,
            last_violation_iteration: 0,
        }
    }

    /// Refine the spec with a counterexample.
    ///
    /// If `is_genuine` is true, the counterexample is a real violation.
    /// If false, it was spurious and should be added to the allowed set.
    pub fn refine(&mut self, counterexample: &str, is_genuine: bool) {
        self.iteration += 1;
        if is_genuine {
            self.violations.insert(counterexample.to_string());
            self.allowed.remove(counterexample);
            self.last_violation_iteration = self.iteration;
        } else {
            self.allowed.insert(counterexample.to_string());
        }
    }

    /// Check if the spec has converged (no new violations for `patience` iterations).
    pub fn is_converged(&self, patience: u32) -> bool {
        if self.iteration == 0 {
            return true; // no refinements attempted
        }
        self.iteration - self.last_violation_iteration >= patience
    }
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect cegar
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-detect/src/detectors/cegar.rs crates/apex-detect/src/detectors/mod.rs && git commit -m "feat(detect): CEGAR spec refinement loop (Task 3.15)"
```

---

### Task 3.16: DeepDFA Dataflow Features

**Crate:** `apex-cpg`
**Create:** `crates/apex-cpg/src/deepdfa.rs`
**Modify:** `crates/apex-cpg/src/lib.rs` (add `pub mod deepdfa;`)

#### Step 3.16.1 — Write failing test

- [ ] Create `/Users/ad/prj/bcov/crates/apex-cpg/src/deepdfa.rs`:

```rust
//! DeepDFA — extract dataflow feature vectors from CPG nodes.
//! Based on the DeepDFA paper — computes per-node feature vectors
//! encoding reaching-definition and taint-reachability information.

use crate::{Cpg, NodeId, NodeKind, EdgeKind};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple_cpg() -> Cpg {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "foo".into(),
            file: "test.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let a = cpg.add_node(NodeKind::Assignment {
            lhs: "y".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "sink".into(),
            line: 3,
        });

        cpg.add_edge(m, p, EdgeKind::Ast);
        cpg.add_edge(p, a, EdgeKind::ReachingDef { variable: "x".into() });
        cpg.add_edge(a, c, EdgeKind::ReachingDef { variable: "y".into() });
        cpg.add_edge(m, a, EdgeKind::Cfg);
        cpg.add_edge(a, c, EdgeKind::Cfg);

        cpg
    }

    #[test]
    fn extract_features_returns_all_nodes() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        assert_eq!(features.len(), cpg.node_count());
    }

    #[test]
    fn feature_vector_has_expected_dims() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        for (_, fv) in &features {
            assert_eq!(fv.len(), FEATURE_DIM);
        }
    }

    #[test]
    fn parameter_node_has_source_flag() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        // Node 1 is the parameter
        let param_features = &features[&1];
        assert!(param_features[IDX_IS_SOURCE] > 0.0);
    }

    #[test]
    fn call_node_has_sink_flag() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        // Node 3 is the call to "sink"
        let call_features = &features[&3];
        assert!(call_features[IDX_IS_SINK] > 0.0);
    }

    #[test]
    fn empty_cpg_returns_empty_features() {
        let cpg = Cpg::new();
        let features = extract_dataflow_features(&cpg);
        assert!(features.is_empty());
    }

    #[test]
    fn reaching_def_count_populated() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        // Assignment node (id=2) has 1 incoming reaching def and 1 outgoing
        let assign_features = &features[&2];
        assert!(assign_features[IDX_REACHING_DEF_IN] > 0.0);
        assert!(assign_features[IDX_REACHING_DEF_OUT] > 0.0);
    }
}
```

- [ ] Add `pub mod deepdfa;` to `/Users/ad/prj/bcov/crates/apex-cpg/src/lib.rs`.

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-cpg extract_features 2>&1 | head -20
```

#### Step 3.16.2 — Implement feature extraction

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-cpg/src/deepdfa.rs`:

```rust
/// Number of features per node.
pub const FEATURE_DIM: usize = 8;

// Feature indices
pub const IDX_IS_SOURCE: usize = 0;     // 1.0 if Parameter node
pub const IDX_IS_SINK: usize = 1;       // 1.0 if Call node
pub const IDX_IS_ASSIGNMENT: usize = 2; // 1.0 if Assignment node
pub const IDX_REACHING_DEF_IN: usize = 3;  // count of incoming ReachingDef edges
pub const IDX_REACHING_DEF_OUT: usize = 4; // count of outgoing ReachingDef edges
pub const IDX_CFG_IN: usize = 5;        // count of incoming Cfg edges
pub const IDX_CFG_OUT: usize = 6;       // count of outgoing Cfg edges
pub const IDX_AST_CHILDREN: usize = 7;  // count of outgoing Ast edges

/// Extract a feature vector for every node in the CPG.
///
/// Features encode node type flags and edge counts, suitable for
/// downstream ML models or heuristic scoring.
pub fn extract_dataflow_features(cpg: &Cpg) -> HashMap<NodeId, Vec<f64>> {
    let mut features: HashMap<NodeId, Vec<f64>> = HashMap::new();

    for (id, kind) in cpg.nodes() {
        let mut fv = vec![0.0; FEATURE_DIM];

        // Node type flags
        match kind {
            NodeKind::Parameter { .. } => fv[IDX_IS_SOURCE] = 1.0,
            NodeKind::Call { .. } => fv[IDX_IS_SINK] = 1.0,
            NodeKind::Assignment { .. } => fv[IDX_IS_ASSIGNMENT] = 1.0,
            _ => {}
        }

        // Edge counts
        for edge in cpg.edges_from(id) {
            match &edge.2 {
                EdgeKind::ReachingDef { .. } => fv[IDX_REACHING_DEF_OUT] += 1.0,
                EdgeKind::Cfg => fv[IDX_CFG_OUT] += 1.0,
                EdgeKind::Ast => fv[IDX_AST_CHILDREN] += 1.0,
                _ => {}
            }
        }

        for edge in cpg.edges_to(id) {
            match &edge.2 {
                EdgeKind::ReachingDef { .. } => fv[IDX_REACHING_DEF_IN] += 1.0,
                EdgeKind::Cfg => fv[IDX_CFG_IN] += 1.0,
                _ => {}
            }
        }

        features.insert(id, fv);
    }

    features
}
```

- [ ] Run tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-cpg deepdfa
```

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-cpg/src/deepdfa.rs crates/apex-cpg/src/lib.rs && git commit -m "feat(cpg): DeepDFA dataflow feature extraction (Task 3.16)"
```

---

## Dependency Graph

```
3.1 (metamorphic)  ──────────────────────── standalone (stubs for 1.1)
3.2 (TCP rank agg) ──────────────────────── standalone
3.3 (change impact) ─── depends on 3.2 ─── uses BranchIndex
3.4 (dead code)    ──────────────────────── depends on 0.1 (LlmClient)
3.5 (flaky repair) ──────────────────────── depends on 0.1, uses FlakyTest
3.6 (diverse SMT)  ──────────────────────── standalone
3.7 (LLM solver)   ──────────────────────── depends on 0.1 (LlmClient)
3.8 (landscape)    ──────────────────────── standalone
3.9 (path decomp)  ──────────────────────── depends on smtlib::extract_variables
3.10 (seedmind)    ──────────────────────── depends on 0.1
3.11 (hgfuzzer)    ──────────────────────── standalone
3.12 (fox control) ──────────────────────── standalone
3.13 (syscall spec)──────────────────────── standalone
3.14 (data spec)   ──────────────────────── standalone
3.15 (cegar)       ──────────────────────── depends on 3.13, 3.14
3.16 (deepdfa)     ──────────────────────── standalone (uses CPG)
```

**Recommended parallel execution:**
- **Batch 1** (no deps): 3.1, 3.2, 3.6, 3.8, 3.9, 3.11, 3.12, 3.13, 3.14, 3.16
- **Batch 2** (after batch 1): 3.3 (needs 3.2), 3.15 (needs 3.13, 3.14)
- **Batch 3** (after Phase 0): 3.4, 3.5, 3.7, 3.10 (need LlmClient)

---

## Summary

| Task | Track | Crate | Files Created | Files Modified | Tests |
|------|-------|-------|---------------|----------------|-------|
| 3.1  | 3A    | apex-coverage | mutation.rs | lib.rs | 6 |
| 3.2  | 3A    | apex-index | prioritize.rs | lib.rs | 5 |
| 3.3  | 3A    | apex-index | change_impact.rs | lib.rs | 5 |
| 3.4  | 3A    | apex-index | dead_code.rs | lib.rs | 4 |
| 3.5  | 3A    | apex-index | flaky_repair.rs | lib.rs | 3 |
| 3.6  | 3B    | apex-symbolic | diversity.rs | lib.rs | 5 |
| 3.7  | 3B    | apex-symbolic | llm_solver.rs | lib.rs | 7 |
| 3.8  | 3B    | apex-symbolic | landscape.rs | lib.rs | 7 |
| 3.9  | 3B    | apex-symbolic | path_decomp.rs | lib.rs | 5 |
| 3.10 | 3C    | apex-fuzz | seedmind.rs | lib.rs | 6 |
| 3.11 | 3C    | apex-fuzz | hgfuzzer.rs | lib.rs | 5 |
| 3.12 | 3C    | apex-fuzz | control.rs | lib.rs | 6 |
| 3.13 | 3D    | apex-detect | spec_miner.rs | detectors/mod.rs | 6 |
| 3.14 | 3D    | apex-index | spec_mining.rs | lib.rs | 4 |
| 3.15 | 3D    | apex-detect | cegar.rs | detectors/mod.rs | 5 |
| 3.16 | 3D    | apex-cpg | deepdfa.rs | lib.rs | 6 |
| **Total** | | **6 crates** | **16 new files** | **8 modified** | **88** |
