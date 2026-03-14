# Phase 1 — Quick Wins: Coverage, Fuzzing, Synthesis, Agent, Security

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 15 research techniques across 5 parallel tracks. Every task is a new file — additive, no conflicts. Each is TDD: write the failing test first, then implement.

**Prerequisite:** Phase 0 complete — `LlmClient` trait, `ExecutionResult.input`, `MutationOperator`/`MutationKind`, `TaintSpecStore` all exist and pass tests.

**Architecture:** All new code lives in new files. Add a `pub mod <name>;` line to the crate's `lib.rs` and a `pub use` re-export. No existing interfaces are modified.

---

## Track 1A — Coverage & Index (apex-coverage, apex-index)

### Task 1.1: Oracle Gap Metric (`oracle_gap.rs`)

**Why:** "Mind the Gap" (ICSE 2025) shows that mutation score predicts real fault detection better than raw coverage %. `OracleGapScore` surfaces mutants that survived (not killed) as the priority list for test generation.

**Files:**
- New: `crates/apex-coverage/src/oracle_gap.rs`
- Modify: `crates/apex-coverage/src/lib.rs` — add `pub mod oracle_gap; pub use oracle_gap::OracleGapScore;`

- [ ] **Step 1: Write failing test**

Add to `crates/apex-coverage/src/oracle_gap.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutation::{MutationKind, MutationOperator, MutationResult};

    fn op(kind: MutationKind, line: u32) -> MutationOperator {
        MutationOperator { kind, file: "lib.py".into(), line, original: "x".into(), replacement: "y".into() }
    }

    #[test]
    fn score_zero_when_all_killed() {
        let results = vec![
            MutationResult { operator: op(MutationKind::BoundaryShift, 1), killed: true, killing_tests: vec!["t1".into()] },
        ];
        let score = OracleGapScore::from_results(&results);
        assert_eq!(score.gap_percent(), 0.0);
        assert!(score.survivors().is_empty());
    }

    #[test]
    fn score_fifty_percent_when_half_survive() {
        let results = vec![
            MutationResult { operator: op(MutationKind::BoundaryShift, 1), killed: true,  killing_tests: vec![] },
            MutationResult { operator: op(MutationKind::ConditionalNegation, 2), killed: false, killing_tests: vec![] },
        ];
        let score = OracleGapScore::from_results(&results);
        assert!((score.gap_percent() - 50.0).abs() < 0.01);
        assert_eq!(score.survivors().len(), 1);
    }

    #[test]
    fn survivors_sorted_by_line() {
        let results = vec![
            MutationResult { operator: op(MutationKind::ArithmeticReplace, 10), killed: false, killing_tests: vec![] },
            MutationResult { operator: op(MutationKind::ReturnValueChange, 3),  killed: false, killing_tests: vec![] },
        ];
        let score = OracleGapScore::from_results(&results);
        let lines: Vec<u32> = score.survivors().iter().map(|s| s.operator.line).collect();
        assert_eq!(lines, vec![3, 10]);
    }
}
```

```bash
cargo test -p apex-coverage oracle_gap 2>&1 | tail -5
# Expected: FAILED (module does not exist)
```

- [ ] **Step 2: Implement `OracleGapScore`**

```rust
use crate::mutation::MutationResult;

/// Gap metric: fraction of mutants that survived the test suite.
#[derive(Debug, Clone)]
pub struct OracleGapScore {
    total: usize,
    survivors: Vec<MutationResult>,
}

impl OracleGapScore {
    pub fn from_results(results: &[MutationResult]) -> Self {
        let mut survivors: Vec<MutationResult> = results.iter()
            .filter(|r| !r.killed)
            .cloned()
            .collect();
        survivors.sort_by_key(|s| s.operator.line);
        Self { total: results.len(), survivors }
    }

    /// Percentage of mutants NOT killed (0.0 = perfect, 100.0 = no test suite).
    pub fn gap_percent(&self) -> f64 {
        if self.total == 0 { return 0.0; }
        (self.survivors.len() as f64 / self.total as f64) * 100.0
    }

    pub fn survivors(&self) -> &[MutationResult] { &self.survivors }
    pub fn total(&self) -> usize { self.total }
}
```

- [ ] **Step 3: Verify tests pass**

```bash
cargo test -p apex-coverage oracle_gap 2>&1 | tail -5
# Expected: test result: ok. 3 passed
```

- [ ] **Step 4: Commit**

```bash
git add crates/apex-coverage/src/oracle_gap.rs crates/apex-coverage/src/lib.rs
git commit -m "feat(coverage): add OracleGapScore metric from Mind-the-Gap paper"
```

---

### Task 1.2: Flaky Detection (`flaky.rs`)

**Why:** FlakyKat (ICSE 2024) identifies non-deterministic tests that produce unreliable coverage signals, which pollute the gap report. `FlakyDetector` flags branches whose hit/miss status flips across repeated runs.

**Files:**
- New: `crates/apex-index/src/flaky.rs`
- Modify: `crates/apex-index/src/lib.rs` — add `pub mod flaky; pub use flaky::{FlakyDetector, FlakyReport};`

- [ ] **Step 1: Write failing tests**

```rust
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
```

- [ ] **Step 2: Implement `FlakyDetector`**

```rust
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
```

- [ ] **Step 3: Verify tests pass, commit**

```bash
cargo test -p apex-index flaky 2>&1 | tail -5
git add crates/apex-index/src/flaky.rs crates/apex-index/src/lib.rs
git commit -m "feat(index): add FlakyDetector for coverage instability detection (FlakyKat)"
```

---

### Task 1.3: Semantic Feedback Signals (`semantic.rs`)

**Why:** arXiv:2511.03995 shows that three lightweight signals — stack depth, unique value diversity, and assertion distance — correlate with hard-to-cover branches and outperform edge coverage alone as a secondary feedback channel for guiding synthesis.

**Files:**
- New: `crates/apex-coverage/src/semantic.rs`
- Modify: `crates/apex-coverage/src/lib.rs` — add `pub mod semantic; pub use semantic::{SemanticSignals, extract_signals};`

- [ ] **Step 1: Write failing tests**

```rust
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
```

- [ ] **Step 2: Implement `SemanticSignals`**

```rust
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct SemanticSignals {
    pub stack_depth_max: usize,
    pub unique_value_count: usize,
    pub assertion_distance: f64,
}

pub fn extract_signals(observed_values: &[u64], stderr: &str) -> SemanticSignals {
    let unique_value_count = observed_values.iter().collect::<HashSet<_>>().len();
    let assertion_distance = parse_assertion_distance(stderr);
    SemanticSignals { stack_depth_max: 0, unique_value_count, assertion_distance }
}

fn parse_assertion_distance(stderr: &str) -> f64 {
    // Heuristic: look for "expected N got M" patterns and compute |N - M|.
    let re = regex::Regex::new(r"expected\s+(\d+)\s+got\s+(\d+)").ok()?;
    let caps = re.captures(stderr)?;
    let a: f64 = caps[1].parse().ok()?;
    let b: f64 = caps[2].parse().ok()?;
    Some((a - b).abs())
}.unwrap_or(0.0)
```

> Note: Add `regex = "1"` to `crates/apex-coverage/Cargo.toml` under `[dependencies]` if not present. Check first with `grep -r 'regex' crates/apex-coverage/Cargo.toml`.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-coverage semantic 2>&1 | tail -5
git add crates/apex-coverage/src/semantic.rs crates/apex-coverage/src/lib.rs
git commit -m "feat(coverage): add SemanticSignals (stack depth, value diversity, assertion distance)"
```

---

## Track 1B — Fuzzing (apex-fuzz)

### Task 1.4: Thompson Sampling Seed Scheduler (`thompson.rs`)

**Why:** T-Scheduler (ISSTA 2024) replaces round-robin corpus selection with Thompson sampling — a Bayesian bandit that prioritizes seeds whose past mutations found new coverage, accelerating branch discovery by 2-3x.

**Files:**
- New: `crates/apex-fuzz/src/thompson.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` — add `pub mod thompson; pub use thompson::ThompsonScheduler;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scheduler_has_no_arms() {
        let ts = ThompsonScheduler::new();
        assert_eq!(ts.arm_count(), 0);
    }

    #[test]
    fn add_seed_creates_arm() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"hello".to_vec());
        assert_eq!(ts.arm_count(), 1);
    }

    #[test]
    fn reward_increases_priority() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        ts.add_seed(b"b".to_vec());
        ts.reward(0, 5); // 5 new branches from seed 0
        // After many samples, seed 0 should dominate
        let mut rng = rand::thread_rng();
        let picks: Vec<usize> = (0..50).map(|_| ts.select(&mut rng)).collect();
        let count_0 = picks.iter().filter(|&&x| x == 0).count();
        assert!(count_0 > 10, "rewarded seed should be picked more often");
    }

    #[test]
    fn select_uniform_with_no_rewards() {
        let mut ts = ThompsonScheduler::new();
        for _ in 0..4 { ts.add_seed(b"x".to_vec()); }
        let mut rng = rand::thread_rng();
        let picks: Vec<usize> = (0..100).map(|_| ts.select(&mut rng)).collect();
        // All 4 arms should be selected at least once
        for i in 0..4 { assert!(picks.contains(&i)); }
    }
}
```

- [ ] **Step 2: Implement `ThompsonScheduler`**

```rust
use rand::RngCore;
use rand_distr::{Beta, Distribution};

/// Bayesian bandit: each seed arm has Beta(α, β) reward distribution.
pub struct ThompsonScheduler {
    arms: Vec<(Vec<u8>, f64, f64)>, // (data, alpha, beta)
}

impl ThompsonScheduler {
    pub fn new() -> Self { Self { arms: Vec::new() } }

    pub fn add_seed(&mut self, data: Vec<u8>) {
        self.arms.push((data, 1.0, 1.0)); // uniform Beta(1,1) prior
    }

    pub fn arm_count(&self) -> usize { self.arms.len() }

    /// Reward arm `idx` with `new_branches` discovered.
    pub fn reward(&mut self, idx: usize, new_branches: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.1 += new_branches as f64; // alpha += successes
        }
    }

    /// Record that seed `idx` produced no new coverage.
    pub fn penalize(&mut self, idx: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.2 += 1.0; // beta += 1
        }
    }

    /// Sample each arm from its Beta distribution; return arm with highest sample.
    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        self.arms.iter().enumerate().map(|(i, (_, a, b))| {
            let sample = Beta::new(*a, *b).ok()
                .map(|d| d.sample(rng))
                .unwrap_or(0.0);
            (i, sample)
        })
        .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0)
    }

    pub fn seed_data(&self, idx: usize) -> Option<&[u8]> {
        self.arms.get(idx).map(|(d, _, _)| d.as_slice())
    }
}
```

> Requires `rand_distr` in `crates/apex-fuzz/Cargo.toml`. Check: `grep rand_distr crates/apex-fuzz/Cargo.toml`. Add `rand_distr = "0.4"` if absent.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-fuzz thompson 2>&1 | tail -5
git add crates/apex-fuzz/src/thompson.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add ThompsonScheduler Bayesian bandit seed selector (T-Scheduler)"
```

---

### Task 1.5: DE Mutation Scheduler (`de_scheduler.rs`)

**Why:** DEzzer (JSS 2025) applies Differential Evolution to mutator weight tuning — crossover and mutation on the weight vector converges to a near-optimal operator mix 40% faster than MOpt.

**Files:**
- New: `crates/apex-fuzz/src/de_scheduler.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` — add `pub mod de_scheduler; pub use de_scheduler::DeScheduler;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_weights_uniform() {
        let de = DeScheduler::new(4);
        let w = de.weights();
        assert_eq!(w.len(), 4);
        // All weights equal initially
        assert!(w.iter().all(|&x| (x - w[0]).abs() < 1e-9));
    }

    #[test]
    fn update_increases_rewarded_weight() {
        let mut de = DeScheduler::new(3);
        let before = de.weights()[0];
        de.update_reward(0, 10.0);
        assert!(de.weights()[0] > before);
    }

    #[test]
    fn weights_sum_to_one_after_update() {
        let mut de = DeScheduler::new(4);
        de.update_reward(1, 5.0);
        let sum: f64 = de.weights().iter().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn select_returns_valid_index() {
        let de = DeScheduler::new(5);
        let mut rng = rand::thread_rng();
        let idx = de.select(&mut rng);
        assert!(idx < 5);
    }
}
```

- [ ] **Step 2: Implement `DeScheduler`**

```rust
use rand::RngCore;

/// Differential Evolution mutator weight scheduler.
pub struct DeScheduler {
    weights: Vec<f64>,
    rewards: Vec<f64>,
}

impl DeScheduler {
    pub fn new(n_operators: usize) -> Self {
        let w = 1.0 / n_operators as f64;
        Self {
            weights: vec![w; n_operators],
            rewards: vec![0.0; n_operators],
        }
    }

    pub fn weights(&self) -> &[f64] { &self.weights }

    /// Record `reward` (e.g. new branches found) for operator `idx`.
    pub fn update_reward(&mut self, idx: usize, reward: f64) {
        if idx < self.rewards.len() {
            self.rewards[idx] += reward;
            self.recompute_weights();
        }
    }

    fn recompute_weights(&mut self) {
        let total: f64 = self.rewards.iter().sum();
        if total > 0.0 {
            for (w, r) in self.weights.iter_mut().zip(&self.rewards) {
                *w = r / total;
            }
        }
    }

    /// Weighted random selection of an operator index.
    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        let mut r = (rng.next_u64() as f64) / (u64::MAX as f64);
        for (i, &w) in self.weights.iter().enumerate() {
            r -= w;
            if r <= 0.0 { return i; }
        }
        self.weights.len() - 1
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-fuzz de_scheduler 2>&1 | tail -5
git add crates/apex-fuzz/src/de_scheduler.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add DeScheduler differential evolution mutator weight tuning (DEzzer)"
```

---

### Task 1.6: Semantic Feedback in Fuzzer (`semantic_feedback.rs`)

**Why:** arXiv:2511.03995 shows that routing semantic signals (assertion distance, value diversity) back into the fuzzer as a secondary fitness function doubles branch discovery on programs with complex data invariants.

**Files:**
- New: `crates/apex-fuzz/src/semantic_feedback.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` — add `pub mod semantic_feedback; pub use semantic_feedback::{SemanticFeedback, SemFeedbackScore};`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{ExecutionResult, ExecutionStatus, SeedId};

    fn make_result(new_branches: usize, stderr: &str) -> ExecutionResult {
        ExecutionResult {
            seed_id: SeedId::new(),
            status: ExecutionStatus::Pass,
            new_branches: (0..new_branches)
                .map(|i| apex_core::types::BranchId::new(1, i as u32, 0, 0))
                .collect(),
            trace: None, duration_ms: 1,
            stdout: String::new(), stderr: stderr.into(), input: None,
        }
    }

    #[test]
    fn zero_score_on_no_coverage_no_stderr() {
        let fb = SemanticFeedback::default();
        let score = fb.score(&make_result(0, ""));
        assert_eq!(score.total(), 0.0);
    }

    #[test]
    fn new_branches_contribute_to_score() {
        let fb = SemanticFeedback::default();
        let score = fb.score(&make_result(3, ""));
        assert!(score.total() > 0.0);
    }

    #[test]
    fn assertion_distance_contributes_when_nonzero() {
        let fb = SemanticFeedback::default();
        let s1 = fb.score(&make_result(0, ""));
        let s2 = fb.score(&make_result(0, "AssertionError: expected 5 got 100"));
        assert!(s2.total() > s1.total());
    }
}
```

- [ ] **Step 2: Implement `SemanticFeedback`**

```rust
use apex_core::types::ExecutionResult;
use apex_coverage::semantic::extract_signals;

#[derive(Debug, Default)]
pub struct SemanticFeedback {
    pub branch_weight: f64,
    pub semantic_weight: f64,
}

impl SemanticFeedback {
    pub fn new(branch_weight: f64, semantic_weight: f64) -> Self {
        Self { branch_weight, semantic_weight }
    }
}

impl Default for SemanticFeedback {
    fn default() -> Self { Self::new(1.0, 0.5) }
}

#[derive(Debug, Default)]
pub struct SemFeedbackScore {
    pub branch_score: f64,
    pub semantic_score: f64,
}

impl SemFeedbackScore {
    pub fn total(&self) -> f64 { self.branch_score + self.semantic_score }
}

impl SemanticFeedback {
    pub fn score(&self, result: &ExecutionResult) -> SemFeedbackScore {
        let branch_score = result.new_branches.len() as f64 * self.branch_weight;
        let sig = extract_signals(&[], &result.stderr);
        let semantic_score = sig.assertion_distance * self.semantic_weight;
        SemFeedbackScore { branch_score, semantic_score }
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-fuzz semantic_feedback 2>&1 | tail -5
git add crates/apex-fuzz/src/semantic_feedback.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add SemanticFeedback secondary fitness from assertion distance signals"
```

---

## Track 1C — Synthesis (apex-synth)

### Task 1.7: Core Abstractions — `PromptStrategy` trait, `GapHistory`, `GapClassifier`

**Why:** All Phase 2 synthesis techniques (TELPA, TestART, SymPrompt, PALM) plug into a shared `PromptStrategy` trait. `GapHistory` tracks which branches were tried and failed, `GapClassifier` categorizes gaps to route them to the right strategy.

**Files:**
- New: `crates/apex-synth/src/strategy.rs`
- New: `crates/apex-synth/src/classify.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add both mods and re-exports

- [ ] **Step 1: Write failing tests for strategy.rs**

```rust
// crates/apex-synth/src/strategy.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_history_records_attempts() {
        let mut h = GapHistory::new();
        h.record_attempt("branch:1:10:0", false);
        h.record_attempt("branch:1:10:0", true);
        assert_eq!(h.attempt_count("branch:1:10:0"), 2);
        assert!(h.last_succeeded("branch:1:10:0"));
    }

    #[test]
    fn gap_history_unknown_key_returns_defaults() {
        let h = GapHistory::new();
        assert_eq!(h.attempt_count("x"), 0);
        assert!(!h.last_succeeded("x"));
    }
}
```

- [ ] **Step 2: Write failing tests for classify.rs**

```rust
// crates/apex-synth/src/classify.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_gap_classified_from_source() {
        let kind = GapClassifier::classify_source("try:\n    x = int(s)\nexcept ValueError:");
        assert_eq!(kind, GapKind::ExceptionHandler);
    }

    #[test]
    fn boundary_gap_classified_from_source() {
        let kind = GapClassifier::classify_source("if x > 100:");
        assert_eq!(kind, GapKind::BoundaryCondition);
    }

    #[test]
    fn unknown_gap_classified_as_general() {
        let kind = GapClassifier::classify_source("pass");
        assert_eq!(kind, GapKind::General);
    }
}
```

- [ ] **Step 3: Implement both files**

```rust
// strategy.rs
use std::collections::HashMap;
use async_trait::async_trait;
use apex_core::types::{BranchId, TestCandidate};
use apex_core::error::Result;

/// A pluggable prompt construction strategy for LLM test synthesis.
#[async_trait]
pub trait PromptStrategy: Send + Sync {
    fn name(&self) -> &str;
    async fn build_prompt(&self, gap: &BranchId, history: &GapHistory, source: &str) -> Result<String>;
}

#[derive(Debug, Default)]
pub struct GapHistory {
    attempts: HashMap<String, Vec<bool>>,
}

impl GapHistory {
    pub fn new() -> Self { Default::default() }
    pub fn record_attempt(&mut self, key: &str, succeeded: bool) {
        self.attempts.entry(key.to_string()).or_default().push(succeeded);
    }
    pub fn attempt_count(&self, key: &str) -> usize {
        self.attempts.get(key).map_or(0, |v| v.len())
    }
    pub fn last_succeeded(&self, key: &str) -> bool {
        self.attempts.get(key).and_then(|v| v.last()).copied().unwrap_or(false)
    }
}
```

```rust
// classify.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapKind { ExceptionHandler, BoundaryCondition, NullCheck, General }

pub struct GapClassifier;

impl GapClassifier {
    pub fn classify_source(snippet: &str) -> GapKind {
        if snippet.contains("except") || snippet.contains("catch") { return GapKind::ExceptionHandler; }
        if snippet.contains("None") || snippet.contains("null") || snippet.contains("nil") { return GapKind::NullCheck; }
        if snippet.contains('>') || snippet.contains('<') || snippet.contains(">=") || snippet.contains("<=") { return GapKind::BoundaryCondition; }
        GapKind::General
    }
}
```

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p apex-synth strategy classify 2>&1 | tail -5
git add crates/apex-synth/src/strategy.rs crates/apex-synth/src/classify.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add PromptStrategy trait, GapHistory, GapClassifier abstractions"
```

---

### Task 1.8: Code Elimination from Prompts (`eliminate.rs`)

**Why:** Xu 2026 demonstrates that stripping dead code paths and irrelevant imports from the source context fed to the LLM reduces prompt token cost by ~30% and improves first-attempt test correctness by eliminating misleading context.

**Files:**
- New: `crates/apex-synth/src/eliminate.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod eliminate; pub use eliminate::eliminate_irrelevant;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
import os
import sys
import logging  # unused

def foo(x):
    if x > 0:
        return x
    return -x

def bar():  # unrelated
    print("hello")
"#;

    #[test]
    fn eliminates_imports_not_referenced_in_target() {
        let result = eliminate_irrelevant(SOURCE, "foo");
        assert!(!result.contains("logging"), "unused import should be removed");
        assert!(result.contains("import os") || result.contains("def foo"));
    }

    #[test]
    fn keeps_target_function() {
        let result = eliminate_irrelevant(SOURCE, "foo");
        assert!(result.contains("def foo"));
        assert!(result.contains("return x"));
    }

    #[test]
    fn result_is_shorter_than_original() {
        let result = eliminate_irrelevant(SOURCE, "foo");
        assert!(result.len() < SOURCE.len());
    }
}
```

- [ ] **Step 2: Implement `eliminate_irrelevant`**

```rust
/// Strip imports and top-level functions not referenced by or within `target_fn`.
pub fn eliminate_irrelevant(source: &str, target_fn: &str) -> String {
    let mut out = Vec::new();
    let mut in_target = false;
    let mut indent_base = 0usize;

    // Collect identifiers used in target function body.
    let target_body = extract_function_body(source, target_fn);

    for line in source.lines() {
        let trimmed = line.trim_start();
        // Keep target function and its body.
        if trimmed.starts_with(&format!("def {target_fn}")) {
            in_target = true;
            indent_base = line.len() - trimmed.len();
            out.push(line);
            continue;
        }
        if in_target {
            let cur_indent = line.len() - line.trim_start().len();
            if line.trim().is_empty() || cur_indent > indent_base {
                out.push(line); continue;
            }
            in_target = false;
        }
        // Keep imports only if the imported name appears in target body.
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            let name = trimmed.split_whitespace().nth(1).unwrap_or("");
            if target_body.contains(name) { out.push(line); }
            continue;
        }
        // Skip other top-level defs not referenced by target.
        if trimmed.starts_with("def ") || trimmed.starts_with("class ") { continue; }
    }
    out.join("\n")
}

fn extract_function_body(source: &str, fn_name: &str) -> String {
    let mut body = String::new();
    let mut in_fn = false;
    let mut base = 0usize;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("def {fn_name}")) {
            in_fn = true;
            base = line.len() - trimmed.len();
            continue;
        }
        if in_fn {
            let cur = line.len() - line.trim_start().len();
            if !line.trim().is_empty() && cur <= base { break; }
            body.push_str(line); body.push('\n');
        }
    }
    body
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-synth eliminate 2>&1 | tail -5
git add crates/apex-synth/src/eliminate.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add eliminate_irrelevant for dead-code stripping in prompts (Xu 2026)"
```

---

### Task 1.9: CoverUp Strategy (`coverup.rs`)

**Why:** CoverUp (ISSTA 2024) is the baseline synthesis strategy — iterative LLM prompting with coverage feedback. Extracting it into a `PromptStrategy` impl lets it participate in the Phase 2 pipeline alongside TELPA, TestART, etc.

**Files:**
- New: `crates/apex-synth/src/coverup.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod coverup; pub use coverup::CoverUpStrategy;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::GapHistory;
    use apex_core::types::BranchId;

    #[test]
    fn strategy_name() {
        assert_eq!(CoverUpStrategy::new().name(), "coverup");
    }

    #[tokio::test]
    async fn prompt_contains_branch_info() {
        let s = CoverUpStrategy::new();
        let branch = BranchId::new(1, 42, 0, 0);
        let history = GapHistory::new();
        let prompt = s.build_prompt(&branch, &history, "def foo(x):\n    if x > 0:\n        return x").await.unwrap();
        assert!(prompt.contains("42")); // line number
        assert!(prompt.contains("def foo"));
    }

    #[tokio::test]
    async fn prompt_includes_retry_hint_on_prior_failure() {
        let s = CoverUpStrategy::new();
        let branch = BranchId::new(1, 5, 0, 0);
        let mut history = GapHistory::new();
        history.record_attempt("1:5:0:0", false);
        let prompt = s.build_prompt(&branch, &history, "pass").await.unwrap();
        assert!(prompt.to_lowercase().contains("previous") || prompt.to_lowercase().contains("attempt") || prompt.to_lowercase().contains("retry"));
    }
}
```

- [ ] **Step 2: Implement `CoverUpStrategy`**

```rust
use async_trait::async_trait;
use apex_core::types::BranchId;
use apex_core::error::Result;
use crate::strategy::{GapHistory, PromptStrategy};

pub struct CoverUpStrategy;

impl CoverUpStrategy {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl PromptStrategy for CoverUpStrategy {
    fn name(&self) -> &str { "coverup" }

    async fn build_prompt(&self, gap: &BranchId, history: &GapHistory, source: &str) -> Result<String> {
        let key = format!("{}:{}:{}:{}", gap.file_id, gap.line, gap.col, gap.direction);
        let attempts = history.attempt_count(&key);
        let retry_hint = if attempts > 0 {
            format!("\nNote: {} previous attempt(s) failed. Try a different approach.", attempts)
        } else {
            String::new()
        };
        Ok(format!(
            "Write a test that covers line {} (direction {}) of the following source code.{}\n\n```\n{}\n```",
            gap.line, gap.direction, retry_hint, source
        ))
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-synth coverup 2>&1 | tail -5
git add crates/apex-synth/src/coverup.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): extract CoverUpStrategy as PromptStrategy impl (Phase 2 baseline)"
```

---

## Track 1D — Agent (apex-agent)

### Task 1.10: Branch Classifier + S2F Categories (`classifier.rs`)

**Why:** S2F (FSE 2024) categorizes uncovered branches into 5 difficulty classes (trivial, data-flow, exception, concurrency, infeasible). Routing each class to its optimal strategy reduces wasted LLM calls by 35%.

**Files:**
- New: `crates/apex-agent/src/classifier.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod classifier; pub use classifier::{BranchClassifier, BranchDifficulty};`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_source_classified_hard() {
        let diff = BranchClassifier::classify_source("except ValueError:\n    pass");
        assert_eq!(diff, BranchDifficulty::ExceptionHandler);
    }

    #[test]
    fn thread_source_classified_concurrency() {
        let diff = BranchClassifier::classify_source("threading.Lock()");
        assert_eq!(diff, BranchDifficulty::Concurrency);
    }

    #[test]
    fn simple_condition_classified_trivial() {
        let diff = BranchClassifier::classify_source("if x == 1:");
        assert_eq!(diff, BranchDifficulty::Trivial);
    }

    #[test]
    fn data_flow_via_multiple_assignments() {
        let diff = BranchClassifier::classify_source("if result[0] > threshold:");
        assert_eq!(diff, BranchDifficulty::DataFlow);
    }

    #[test]
    fn all_variants_distinct() {
        use std::collections::HashSet;
        let variants = [BranchDifficulty::Trivial, BranchDifficulty::DataFlow,
                        BranchDifficulty::ExceptionHandler, BranchDifficulty::Concurrency,
                        BranchDifficulty::Infeasible];
        let set: HashSet<_> = variants.iter().collect();
        assert_eq!(set.len(), 5);
    }
}
```

- [ ] **Step 2: Implement `BranchClassifier`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BranchDifficulty {
    Trivial,
    DataFlow,
    ExceptionHandler,
    Concurrency,
    Infeasible,
}

pub struct BranchClassifier;

impl BranchClassifier {
    pub fn classify_source(snippet: &str) -> BranchDifficulty {
        if snippet.contains("thread") || snippet.contains("Lock") || snippet.contains("async") {
            return BranchDifficulty::Concurrency;
        }
        if snippet.contains("except") || snippet.contains("raise") || snippet.contains("catch") {
            return BranchDifficulty::ExceptionHandler;
        }
        if snippet.contains('[') || snippet.contains('.') && snippet.contains('>') {
            return BranchDifficulty::DataFlow;
        }
        BranchDifficulty::Trivial
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-agent classifier 2>&1 | tail -5
git add crates/apex-agent/src/classifier.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add BranchClassifier with S2F difficulty categories"
```

---

### Task 1.11: Thompson Strategy Bandit (`bandit.rs`)

**Why:** Extending T-Scheduler from seeds to synthesis strategies: the bandit learns which `PromptStrategy` variant (coverup, telpa, palm, etc.) performs best on each `BranchDifficulty` class, converging to optimal routing without fixed rules.

**Files:**
- New: `crates/apex-agent/src/bandit.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod bandit; pub use bandit::StrategyBandit;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bandit_registers_strategies() {
        let bandit = StrategyBandit::new(vec!["coverup".into(), "telpa".into()]);
        assert_eq!(bandit.strategy_count(), 2);
    }

    #[test]
    fn reward_shifts_selection_probability() {
        let mut bandit = StrategyBandit::new(vec!["a".into(), "b".into()]);
        bandit.reward("b", 10.0);
        let mut rng = rand::thread_rng();
        let picks: Vec<&str> = (0..30).map(|_| bandit.select(&mut rng)).collect();
        let b_count = picks.iter().filter(|&&s| s == "b").count();
        assert!(b_count > 5, "rewarded strategy should be picked more: {b_count}");
    }

    #[test]
    fn unknown_strategy_reward_is_noop() {
        let mut bandit = StrategyBandit::new(vec!["a".into()]);
        bandit.reward("nonexistent", 100.0); // should not panic
    }
}
```

- [ ] **Step 2: Implement `StrategyBandit`**

```rust
use rand::RngCore;
use rand_distr::{Beta, Distribution};
use std::collections::HashMap;

pub struct StrategyBandit {
    arms: Vec<String>,
    alpha: HashMap<String, f64>,
    beta_val: HashMap<String, f64>,
}

impl StrategyBandit {
    pub fn new(strategies: Vec<String>) -> Self {
        let mut alpha = HashMap::new();
        let mut beta_val = HashMap::new();
        for s in &strategies { alpha.insert(s.clone(), 1.0); beta_val.insert(s.clone(), 1.0); }
        Self { arms: strategies, alpha, beta_val }
    }

    pub fn strategy_count(&self) -> usize { self.arms.len() }

    pub fn reward(&mut self, strategy: &str, value: f64) {
        if let Some(a) = self.alpha.get_mut(strategy) { *a += value; }
    }

    pub fn penalize(&mut self, strategy: &str) {
        if let Some(b) = self.beta_val.get_mut(strategy) { *b += 1.0; }
    }

    pub fn select<'a>(&'a self, rng: &mut dyn RngCore) -> &'a str {
        self.arms.iter().max_by(|a, b| {
            let sa = self.sample_arm(a, rng);
            let sb = self.sample_arm(b, rng);
            sa.partial_cmp(&sb).unwrap()
        }).map(|s| s.as_str()).unwrap_or("")
    }

    fn sample_arm(&self, name: &str, rng: &mut dyn RngCore) -> f64 {
        let a = self.alpha.get(name).copied().unwrap_or(1.0);
        let b = self.beta_val.get(name).copied().unwrap_or(1.0);
        Beta::new(a, b).ok().map(|d| d.sample(rng)).unwrap_or(0.0)
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-agent bandit 2>&1 | tail -5
git add crates/apex-agent/src/bandit.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add StrategyBandit Thompson sampling over synthesis strategies"
```

---

### Task 1.12: Mutation Guide (`mutation_guide.rs`)

**Why:** Meta ACH (ICSE 2025) uses mutation score as a direct signal for guiding the agent — branches surrounded by surviving mutants need tests that assert on specific values, not just code paths. `MutationGuide` bridges `OracleGapScore` into the agent's decision loop.

**Files:**
- New: `crates/apex-agent/src/mutation_guide.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod mutation_guide; pub use mutation_guide::MutationGuide;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_coverage::mutation::{MutationKind, MutationOperator, MutationResult};

    fn survived(kind: MutationKind, line: u32) -> MutationResult {
        MutationResult {
            operator: MutationOperator { kind, file: "f.py".into(), line,
                original: "x".into(), replacement: "y".into() },
            killed: false, killing_tests: vec![],
        }
    }

    #[test]
    fn no_survivors_means_no_guidance() {
        let guide = MutationGuide::new(vec![]);
        assert!(guide.priority_lines().is_empty());
    }

    #[test]
    fn survivors_produce_guidance_hints() {
        let guide = MutationGuide::new(vec![
            survived(MutationKind::BoundaryShift, 10),
            survived(MutationKind::ReturnValueChange, 20),
        ]);
        assert!(guide.priority_lines().contains(&10));
        assert!(guide.priority_lines().contains(&20));
    }

    #[test]
    fn hint_for_boundary_suggests_value_assertion() {
        let guide = MutationGuide::new(vec![survived(MutationKind::BoundaryShift, 5)]);
        let hint = guide.hint_for_line(5).unwrap();
        assert!(hint.to_lowercase().contains("boundary") || hint.to_lowercase().contains("value"));
    }
}
```

- [ ] **Step 2: Implement `MutationGuide`**

```rust
use std::collections::{HashMap, HashSet};
use apex_coverage::mutation::{MutationKind, MutationResult};

pub struct MutationGuide {
    survivors: Vec<MutationResult>,
    hints: HashMap<u32, String>,
}

impl MutationGuide {
    pub fn new(survivors: Vec<MutationResult>) -> Self {
        let mut hints = HashMap::new();
        for s in &survivors {
            let hint = match s.operator.kind {
                MutationKind::BoundaryShift => "Add assertion on boundary value (e.g., off-by-one).".into(),
                MutationKind::ReturnValueChange => "Assert on the exact return value.".into(),
                MutationKind::ConditionalNegation => "Test both true and false branch outcomes.".into(),
                MutationKind::ArithmeticReplace => "Assert on the computed numeric result.".into(),
                _ => "Add assertion on observable side effect.".into(),
            };
            hints.entry(s.operator.line).or_insert(hint);
        }
        Self { survivors, hints }
    }

    pub fn priority_lines(&self) -> HashSet<u32> {
        self.survivors.iter().map(|s| s.operator.line).collect()
    }

    pub fn hint_for_line(&self, line: u32) -> Option<&str> {
        self.hints.get(&line).map(|s| s.as_str())
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-agent mutation_guide 2>&1 | tail -5
git add crates/apex-agent/src/mutation_guide.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add MutationGuide bridging oracle gap into agent hints (Meta ACH)"
```

---

## Track 1E — Security (apex-cpg, apex-detect)

### Task 1.13: Taint Flows with Store (`taint_flows_store.rs`)

**Why:** IRIS (S&P 2024) infers taint specs via LLM then runs taint analysis with them. The existing `find_taint_flows()` in `apex-cpg/src/taint.rs` uses hardcoded arrays; `find_taint_flows_with_store()` accepts a `TaintSpecStore` so IRIS-inferred specs can be injected at runtime.

**Files:**
- New: `crates/apex-cpg/src/taint_flows_store.rs`
- Modify: `crates/apex-cpg/src/lib.rs` — add `pub mod taint_flows_store; pub use taint_flows_store::find_taint_flows_with_store;`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::taint_store::TaintSpecStore;
    use crate::graph::{CpgNode, NodeKind};

    fn make_node(id: u32, name: &str) -> CpgNode {
        CpgNode { id, kind: NodeKind::Call, name: name.into(), file: "f.py".into(), line: id, col: 0 }
    }

    #[test]
    fn empty_store_finds_no_flows() {
        let store = TaintSpecStore::new();
        let nodes = vec![make_node(1, "user_input"), make_node(2, "exec")];
        let flows = find_taint_flows_with_store(&nodes, &[], &store);
        assert!(flows.is_empty());
    }

    #[test]
    fn source_to_sink_flow_detected() {
        let mut store = TaintSpecStore::new();
        store.add_source("user_input".into());
        store.add_sink("exec".into());
        let nodes = vec![make_node(1, "user_input"), make_node(2, "exec")];
        let edges = vec![(1u32, 2u32)]; // data flow edge
        let flows = find_taint_flows_with_store(&nodes, &edges, &store);
        assert!(!flows.is_empty());
    }

    #[test]
    fn sanitizer_breaks_flow() {
        let mut store = TaintSpecStore::new();
        store.add_source("user_input".into());
        store.add_sink("exec".into());
        store.add_sanitizer("sanitize".into());
        let nodes = vec![make_node(1, "user_input"), make_node(2, "sanitize"), make_node(3, "exec")];
        let edges = vec![(1, 2), (2, 3)];
        let flows = find_taint_flows_with_store(&nodes, &edges, &store);
        assert!(flows.is_empty(), "sanitizer should block the flow");
    }
}
```

> Note: Check what `CpgNode`/`NodeKind` look like in `crates/apex-cpg/src/graph.rs` before writing the test. Adjust field names to match.

- [ ] **Step 2: Check actual graph types**

```bash
grep -n 'pub struct CpgNode\|pub enum NodeKind\|pub id\|pub name\|pub kind' crates/apex-cpg/src/graph.rs | head -20
```

Adapt the test struct construction to match real field names.

- [ ] **Step 3: Implement `find_taint_flows_with_store`**

```rust
use crate::taint_store::TaintSpecStore;
// Import CpgNode from graph module — adjust path as needed.
use crate::graph::CpgNode;

#[derive(Debug, Clone)]
pub struct TaintFlow {
    pub source_node: u32,
    pub sink_node: u32,
    pub path: Vec<u32>,
}

/// Find source→sink flows using runtime-extensible specs from `store`.
/// `edges` is a list of directed data-flow edges (from_id, to_id).
pub fn find_taint_flows_with_store(
    nodes: &[CpgNode],
    edges: &[(u32, u32)],
    store: &TaintSpecStore,
) -> Vec<TaintFlow> {
    let sources: Vec<u32> = nodes.iter()
        .filter(|n| store.is_source(&n.name))
        .map(|n| n.id).collect();
    let sinks: Vec<u32> = nodes.iter()
        .filter(|n| store.is_sink(&n.name))
        .map(|n| n.id).collect();
    let sanitizer_ids: std::collections::HashSet<u32> = nodes.iter()
        .filter(|n| store.is_sanitizer(&n.name))
        .map(|n| n.id).collect();

    let mut flows = Vec::new();
    for &src in &sources {
        for &sink in &sinks {
            if let Some(path) = reachable_without_sanitizer(src, sink, edges, &sanitizer_ids) {
                flows.push(TaintFlow { source_node: src, sink_node: sink, path });
            }
        }
    }
    flows
}

fn reachable_without_sanitizer(
    src: u32, sink: u32,
    edges: &[(u32, u32)],
    sanitizers: &std::collections::HashSet<u32>,
) -> Option<Vec<u32>> {
    // BFS from src to sink, avoiding sanitizer nodes.
    use std::collections::{VecDeque, HashMap};
    let mut queue = VecDeque::from([(src, vec![src])]);
    let mut visited = std::collections::HashSet::new();
    while let Some((cur, path)) = queue.pop_front() {
        if cur == sink { return Some(path); }
        if !visited.insert(cur) { continue; }
        for &(from, to) in edges {
            if from == cur && !sanitizers.contains(&to) {
                let mut new_path = path.clone();
                new_path.push(to);
                queue.push_back((to, new_path));
            }
        }
    }
    None
}
```

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p apex-cpg taint_flows_store 2>&1 | tail -5
git add crates/apex-cpg/src/taint_flows_store.rs crates/apex-cpg/src/lib.rs
git commit -m "feat(cpg): add find_taint_flows_with_store for runtime-extensible IRIS taint specs"
```

---

### Task 1.14: Type-Based Taint Tracking (`type_taint.rs`)

**Why:** arXiv:2504.18529 shows that propagating taint through type annotations (e.g., `str` parameters from HTTP handlers are always tainted) catches 20% more injection paths than call-graph taint analysis alone.

**Files:**
- New: `crates/apex-cpg/src/type_taint.rs`
- Modify: `crates/apex-cpg/src/lib.rs` — add `pub mod type_taint; pub use type_taint::{TypeTaintRule, TypeTaintAnalyzer};`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_string_param_is_tainted() {
        let mut analyzer = TypeTaintAnalyzer::new();
        analyzer.add_rule(TypeTaintRule {
            annotation: "HttpRequest".into(),
            taint_fields: vec!["body".into(), "query_params".into()],
        });
        assert!(analyzer.is_tainted("HttpRequest", "body"));
        assert!(!analyzer.is_tainted("HttpRequest", "headers"));
    }

    #[test]
    fn unknown_type_not_tainted() {
        let analyzer = TypeTaintAnalyzer::new();
        assert!(!analyzer.is_tainted("MyClass", "field"));
    }

    #[test]
    fn rule_count_matches_added() {
        let mut analyzer = TypeTaintAnalyzer::new();
        analyzer.add_rule(TypeTaintRule { annotation: "A".into(), taint_fields: vec!["f".into()] });
        analyzer.add_rule(TypeTaintRule { annotation: "B".into(), taint_fields: vec!["g".into()] });
        assert_eq!(analyzer.rule_count(), 2);
    }
}
```

- [ ] **Step 2: Implement `TypeTaintAnalyzer`**

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TypeTaintRule {
    pub annotation: String,
    pub taint_fields: Vec<String>,
}

#[derive(Debug, Default)]
pub struct TypeTaintAnalyzer {
    rules: HashMap<String, Vec<String>>,
}

impl TypeTaintAnalyzer {
    pub fn new() -> Self { Default::default() }

    pub fn add_rule(&mut self, rule: TypeTaintRule) {
        self.rules.insert(rule.annotation, rule.taint_fields);
    }

    pub fn is_tainted(&self, type_name: &str, field: &str) -> bool {
        self.rules.get(type_name).map_or(false, |fields| fields.iter().any(|f| f == field))
    }

    pub fn rule_count(&self) -> usize { self.rules.len() }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-cpg type_taint 2>&1 | tail -5
git add crates/apex-cpg/src/type_taint.rs crates/apex-cpg/src/lib.rs
git commit -m "feat(cpg): add TypeTaintAnalyzer for annotation-based taint propagation"
```

---

### Task 1.15: ML Taint Triage (`taint_triage.rs`)

**Why:** arXiv:2510.20739 shows that a lightweight feature-based scorer (without ML at inference time) can rank taint flows by exploitability — cutting false-positive review burden by 60%. The scorer uses path length, sanitizer presence, and sink severity as features.

**Files:**
- New: `crates/apex-cpg/src/taint_triage.rs`
- Modify: `crates/apex-cpg/src/lib.rs` — add `pub mod taint_triage; pub use taint_triage::{TaintTriageScorer, TriagedFlow};`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::taint_flows_store::TaintFlow;

    fn flow(path_len: usize, sink: &str) -> (TaintFlow, String) {
        let path = (0..path_len as u32).collect();
        (TaintFlow { source_node: 0, sink_node: path_len as u32 - 1, path }, sink.to_string())
    }

    #[test]
    fn exec_sink_scores_higher_than_log() {
        let scorer = TaintTriageScorer::default();
        let (f1, s1) = flow(3, "exec");
        let (f2, s2) = flow(3, "logging.info");
        let score_exec = scorer.score(&f1, &s1);
        let score_log  = scorer.score(&f2, &s2);
        assert!(score_exec > score_log);
    }

    #[test]
    fn shorter_path_scores_higher_than_longer() {
        let scorer = TaintTriageScorer::default();
        let (f_short, s) = flow(2, "exec");
        let (f_long, _)  = flow(10, "exec");
        assert!(scorer.score(&f_short, &s) > scorer.score(&f_long, &s));
    }

    #[test]
    fn ranked_flows_sorted_descending() {
        let scorer = TaintTriageScorer::default();
        let flows = vec![
            (flow(5, "exec").0, "exec".into()),
            (flow(2, "exec").0, "exec".into()),
            (flow(8, "log").0,  "log".into()),
        ];
        let ranked = scorer.rank(flows);
        assert!(ranked[0].score >= ranked[1].score);
        assert!(ranked[1].score >= ranked[2].score);
    }
}
```

- [ ] **Step 2: Implement `TaintTriageScorer`**

```rust
use crate::taint_flows_store::TaintFlow;

const HIGH_SEVERITY_SINKS: &[&str] = &["exec", "eval", "subprocess", "os.system", "pickle.loads"];

#[derive(Debug)]
pub struct TriagedFlow {
    pub flow: TaintFlow,
    pub sink_name: String,
    pub score: f64,
}

#[derive(Debug, Default)]
pub struct TaintTriageScorer;

impl TaintTriageScorer {
    pub fn score(&self, flow: &TaintFlow, sink_name: &str) -> f64 {
        let severity = if HIGH_SEVERITY_SINKS.iter().any(|s| sink_name.contains(s)) { 1.0 } else { 0.2 };
        let path_penalty = 1.0 / (1.0 + flow.path.len() as f64 * 0.1);
        severity * path_penalty
    }

    pub fn rank(&self, flows: Vec<(TaintFlow, String)>) -> Vec<TriagedFlow> {
        let mut triaged: Vec<TriagedFlow> = flows.into_iter().map(|(f, s)| {
            let score = self.score(&f, &s);
            TriagedFlow { flow: f, sink_name: s, score }
        }).collect();
        triaged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        triaged
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p apex-cpg taint_triage 2>&1 | tail -5
git add crates/apex-cpg/src/taint_triage.rs crates/apex-cpg/src/lib.rs
git commit -m "feat(cpg): add TaintTriageScorer for exploitability-ranked flow triage"
```

---

## Phase 1 Gate

Run the full workspace test suite before declaring Phase 1 complete:

```bash
cargo test --workspace 2>&1 | tail -20
```

Expected output: `test result: ok. N passed; 0 failed` across all crates.

If any test fails:
1. Identify the failing test with `cargo test --workspace 2>&1 | grep FAILED`.
2. Read the error: `cargo test -p <crate> <test_name> -- --nocapture`.
3. Fix the implementation (never modify the test unless the test itself was wrong by design).

---

## Summary Table

| # | Technique | Crate | New File | Paper | Est. |
|---|-----------|-------|----------|-------|------|
| 1.1 | Oracle Gap Metric | apex-coverage | `oracle_gap.rs` | Mind the Gap | 0.5d |
| 1.2 | Flaky Detection | apex-index | `flaky.rs` | FlakyKat | 0.5d |
| 1.3 | Semantic Feedback Signals | apex-coverage | `semantic.rs` | arXiv:2511.03995 | 0.5d |
| 1.4 | Thompson Seed Scheduler | apex-fuzz | `thompson.rs` | T-Scheduler | 1d |
| 1.5 | DE Mutation Scheduler | apex-fuzz | `de_scheduler.rs` | DEzzer/JSS 2025 | 1d |
| 1.6 | Semantic Feedback in Fuzzer | apex-fuzz | `semantic_feedback.rs` | arXiv:2511.03995 | 0.5d |
| 1.7 | `PromptStrategy` + `GapHistory` + `GapClassifier` | apex-synth | `strategy.rs`, `classify.rs` | — | 1d |
| 1.8 | Code Elimination from Prompts | apex-synth | `eliminate.rs` | Xu 2026 | 1d |
| 1.9 | CoverUp as `PromptStrategy` | apex-synth | `coverup.rs` | CoverUp | 0.5d |
| 1.10 | `BranchClassifier` + S2F categories | apex-agent | `classifier.rs` | S2F | 0.5d |
| 1.11 | Thompson Strategy Bandit | apex-agent | `bandit.rs` | T-Scheduler ext. | 1d |
| 1.12 | `MutationGuide` | apex-agent | `mutation_guide.rs` | Meta ACH | 1d |
| 1.13 | `find_taint_flows_with_store` | apex-cpg | `taint_flows_store.rs` | IRIS | 1d |
| 1.14 | Type-Based Taint Tracking | apex-cpg | `type_taint.rs` | arXiv:2504.18529 | 1d |
| 1.15 | ML Taint Triage (scoring) | apex-cpg | `taint_triage.rs` | arXiv:2510.20739 | 1d |

**Total:** 15 new files, ~11d of focused work, fully parallelizable across 3 developers (one per track pair: 1A+1B, 1C, 1D+1E).

**Unlocks:** All Phase 2 tasks. Specifically: 1.7 unlocks 2.1-2.7; 1.4+1.5 unlock 2.8-2.10; 1.10 unlocks 2.11-2.13; 1.13 unlocks 2.15.
