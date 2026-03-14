# Phase 0 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build shared infrastructure (LlmClient, bug fixes, type extensions) that unblocks all 49 research techniques across 5 subsystems.

**Architecture:** 7 prerequisite tasks adding traits, types, and bug fixes to apex-core, apex-fuzz, apex-coverage, apex-cpg, and apex-detect. All changes are additive — no existing behavior changes.

**Tech Stack:** Rust, async_trait, serde, reqwest, DashMap

---

## Task 0.1: LlmClient trait + AnthropicClient impl

**Crate:** `apex-core`
**Create:** `crates/apex-core/src/llm.rs`
**Modify:** `crates/apex-core/src/lib.rs` (add `pub mod llm;`)
**Modify:** `crates/apex-core/Cargo.toml` (add `reqwest` dependency)

### Step 0.1.1 — Add `reqwest` to Cargo.toml

- [ ] Add `reqwest` dependency to `crates/apex-core/Cargo.toml`:

```toml
reqwest = { version = "0.11", features = ["json"], default-features = false, optional = true }
```

And add a feature:
```toml
[features]
default = []
llm = ["reqwest"]
```

**File:** `/Users/ad/prj/bcov/crates/apex-core/Cargo.toml`

Add after the `[dependencies]` section, before `[dev-dependencies]`:
```toml
reqwest = { version = "0.11", features = ["json", "rustls-tls"], default-features = false }
```

- [ ] Verify it compiles:
```bash
cd /Users/ad/prj/bcov && cargo check -p apex-core
```

### Step 0.1.2 — Write failing test for LlmMessage

- [ ] Create `/Users/ad/prj/bcov/crates/apex-core/src/llm.rs` with test only:

```rust
//! LLM client abstraction for APEX — trait + Anthropic implementation + mock.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_message_creation_and_clone() {
        let msg = LlmMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        let cloned = msg.clone();
        assert_eq!(cloned.role, "user");
        assert_eq!(cloned.content, "hello");
    }
}
```

- [ ] Add `pub mod llm;` to `/Users/ad/prj/bcov/crates/apex-core/src/lib.rs` after the existing modules:

```rust
pub mod llm;
```

- [ ] Run test to verify it fails (struct doesn't exist yet):
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core llm_message_creation_and_clone 2>&1 | head -20
```
Expected: compilation error — `LlmMessage` not found.

### Step 0.1.3 — Implement LlmMessage, LlmResponse, LlmClient trait

- [ ] Write the types and trait above the `#[cfg(test)]` module in `/Users/ad/prj/bcov/crates/apex-core/src/llm.rs`:

```rust
//! LLM client abstraction for APEX — trait + Anthropic implementation + mock.

use async_trait::async_trait;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, messages: &[LlmMessage], max_tokens: u32) -> Result<LlmResponse>;
    fn model_name(&self) -> &str;
}
```

- [ ] Run the test:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core llm_message_creation_and_clone
```
Expected: `test llm::tests::llm_message_creation_and_clone ... ok`

### Step 0.1.4 — Write failing test for MockLlmClient

- [ ] Add to the `tests` module in `/Users/ad/prj/bcov/crates/apex-core/src/llm.rs`:

```rust
    #[tokio::test]
    async fn mock_returns_queued_responses() {
        let mock = MockLlmClient::new(vec![
            "response one".to_string(),
            "response two".to_string(),
        ]);
        let msgs = [LlmMessage { role: "user".into(), content: "hi".into() }];
        let r1 = mock.complete(&msgs, 100).await.unwrap();
        assert_eq!(r1.content, "response one");
        let r2 = mock.complete(&msgs, 100).await.unwrap();
        assert_eq!(r2.content, "response two");
    }

    #[tokio::test]
    async fn mock_returns_error_when_empty() {
        let mock = MockLlmClient::new(vec![]);
        let msgs = [LlmMessage { role: "user".into(), content: "hi".into() }];
        let result = mock.complete(&msgs, 100).await;
        assert!(result.is_err());
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core mock_returns_queued 2>&1 | head -20
```
Expected: compilation error — `MockLlmClient` not found.

### Step 0.1.5 — Implement MockLlmClient

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-core/src/llm.rs`:

```rust
/// Mock LLM client that returns pre-queued responses. For testing only.
pub struct MockLlmClient {
    responses: std::sync::Mutex<Vec<String>>,
}

impl MockLlmClient {
    pub fn new(responses: Vec<String>) -> Self {
        // Reverse so we can pop from the end (FIFO order)
        let mut reversed = responses;
        reversed.reverse();
        MockLlmClient {
            responses: std::sync::Mutex::new(reversed),
        }
    }
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, _messages: &[LlmMessage], _max_tokens: u32) -> Result<LlmResponse> {
        let mut queue = self.responses.lock().map_err(|e| {
            crate::error::ApexError::Other(format!("mock mutex poisoned: {e}"))
        })?;
        match queue.pop() {
            Some(content) => Ok(LlmResponse {
                content,
                input_tokens: 0,
                output_tokens: 0,
            }),
            None => Err(crate::error::ApexError::Agent(
                "MockLlmClient: no more queued responses".into(),
            )),
        }
    }

    fn model_name(&self) -> &str {
        "mock"
    }
}
```

- [ ] Run both tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core mock_returns
```
Expected: both `mock_returns_queued_responses` and `mock_returns_error_when_empty` pass.

### Step 0.1.6 — Write failing test for AnthropicClient

- [ ] Add to `tests` module in `/Users/ad/prj/bcov/crates/apex-core/src/llm.rs`:

```rust
    #[test]
    fn anthropic_client_new_without_api_key_returns_error() {
        // Temporarily clear the env var to ensure the error path
        let saved = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = AnthropicClient::from_env();
        assert!(result.is_err(), "should fail without ANTHROPIC_API_KEY");
        // Restore
        if let Some(val) = saved {
            std::env::set_var("ANTHROPIC_API_KEY", val);
        }
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core anthropic_client_new 2>&1 | head -20
```
Expected: compilation error — `AnthropicClient` not found.

### Step 0.1.7 — Implement AnthropicClient

- [ ] Add above `MockLlmClient` in `/Users/ad/prj/bcov/crates/apex-core/src/llm.rs`:

```rust
/// Anthropic API client using the Messages API.
pub struct AnthropicClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicClient {
    /// Create a new client with the given model name and API key.
    pub fn new(model: &str, api_key: String) -> Result<Self> {
        Ok(AnthropicClient {
            client: reqwest::Client::new(),
            api_key,
            model: model.to_string(),
        })
    }

    /// Create from ANTHROPIC_API_KEY environment variable, defaulting to claude-sonnet-4-20250514.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            crate::error::ApexError::Config(
                "ANTHROPIC_API_KEY environment variable not set".into(),
            )
        })?;
        Self::new("claude-sonnet-4-20250514", api_key)
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, messages: &[LlmMessage], max_tokens: u32) -> Result<LlmResponse> {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": api_messages,
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::error::ApexError::Agent(format!("HTTP error: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| crate::error::ApexError::Agent(format!("response read error: {e}")))?;

        if !status.is_success() {
            return Err(crate::error::ApexError::Agent(format!(
                "Anthropic API error {status}: {text}"
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&text)?;

        let content = json["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block["text"].as_str())
            .unwrap_or("")
            .to_string();

        let input_tokens = json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

        Ok(LlmResponse {
            content,
            input_tokens,
            output_tokens,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
```

- [ ] Run all llm tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core llm::tests
```
Expected: all 4 tests pass.

### Step 0.1.8 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-core/src/llm.rs crates/apex-core/src/lib.rs crates/apex-core/Cargo.toml && git commit -m "feat(apex-core): add LlmClient trait, AnthropicClient, and MockLlmClient

Phase 0.1: shared LLM abstraction for agent-driven test generation.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Task 0.2: Fix observe() corpus feedback (P1 bug)

**Crate:** `apex-fuzz`
**Modify:** `crates/apex-fuzz/src/lib.rs` (around line 160)

The current `observe()` logs new coverage but never adds the winning input to the corpus. This blocks feedback-driven fuzzing.

### Step 0.2.1 — Write failing test: observe with input adds to corpus

- [ ] Add to `#[cfg(test)] mod tests` in `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`, after the existing helper `make_result_with_branches`:

```rust
    fn make_result_with_input_and_branches(
        input: Option<Vec<u8>>,
        branches: Vec<apex_core::types::BranchId>,
    ) -> ExecutionResult {
        ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: branches,
            trace: None,
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
            input,
        }
    }

    #[tokio::test]
    async fn observe_with_new_branches_and_input_adds_to_corpus() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        assert_eq!(strategy.corpus.lock().unwrap().len(), 0);

        let branch = apex_core::types::BranchId::new(1, 10, 0, 0);
        let result = make_result_with_input_and_branches(
            Some(b"winning input".to_vec()),
            vec![branch],
        );
        strategy.observe(&result).await.unwrap();

        assert_eq!(
            strategy.corpus.lock().unwrap().len(),
            1,
            "observe should add input to corpus when new branches found"
        );
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz observe_with_new_branches_and_input_adds_to_corpus 2>&1 | head -30
```
Expected: compilation error — `ExecutionResult` has no field `input`.

> **Note:** This test depends on Task 0.4 (adding `input` to `ExecutionResult`). Tasks 0.2 and 0.4 must be implemented together. Proceed to Task 0.4 first, then return here.

### Step 0.2.2 — Write failing test: observe with no input does not add

- [ ] Add to tests in `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`:

```rust
    #[tokio::test]
    async fn observe_with_new_branches_but_no_input_does_not_add() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);

        let branch = apex_core::types::BranchId::new(1, 10, 0, 0);
        let result = make_result_with_input_and_branches(None, vec![branch]);
        strategy.observe(&result).await.unwrap();

        assert_eq!(
            strategy.corpus.lock().unwrap().len(),
            0,
            "observe should NOT add to corpus when input is None"
        );
    }
```

### Step 0.2.3 — Write failing test: observe with no new branches does not add

- [ ] Add to tests in `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`:

```rust
    #[tokio::test]
    async fn observe_with_no_new_branches_does_not_add() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);

        let result = make_result_with_input_and_branches(
            Some(b"some input".to_vec()),
            vec![], // no new branches
        );
        strategy.observe(&result).await.unwrap();

        assert_eq!(
            strategy.corpus.lock().unwrap().len(),
            0,
            "observe should NOT add to corpus when no new branches"
        );
    }
```

### Step 0.2.4 — Fix observe() implementation

- [ ] In `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`, replace the `observe` method (lines 160-173):

Replace:
```rust
    async fn observe(&self, result: &ExecutionResult) -> Result<()> {
        // Add to corpus any input that found new coverage.
        if !result.new_branches.is_empty() {
            info!(
                newly_covered = result.new_branches.len(),
                "fuzzer: interesting input added to corpus"
            );
            // The seed data is not stored in ExecutionResult; the orchestrator
            // must call seed_corpus() separately with the winning input.
            // TODO(phase3): thread the winning InputSeed back through result.
        }
        Ok(())
    }
```

With:
```rust
    async fn observe(&self, result: &ExecutionResult) -> Result<()> {
        if !result.new_branches.is_empty() {
            if let Some(input) = &result.input {
                let mut corpus = self
                    .corpus
                    .lock()
                    .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?;
                corpus.add(input.clone(), result.new_branches.len());
                info!(
                    newly_covered = result.new_branches.len(),
                    corpus_size = corpus.len(),
                    "fuzzer: interesting input added to corpus"
                );
            } else {
                info!(
                    newly_covered = result.new_branches.len(),
                    "fuzzer: new coverage but no input available to add"
                );
            }
        }
        Ok(())
    }
```

- [ ] Also update the existing `make_result_with_branches` helper to include the new field:

Replace in `/Users/ad/prj/bcov/crates/apex-fuzz/src/lib.rs`:
```rust
    fn make_result_with_branches(branches: Vec<apex_core::types::BranchId>) -> ExecutionResult {
        ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: branches,
            trace: None,
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
```

With:
```rust
    fn make_result_with_branches(branches: Vec<apex_core::types::BranchId>) -> ExecutionResult {
        ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: branches,
            trace: None,
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
            input: None,
        }
    }
```

- [ ] Run all three new tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz observe_with
```
Expected: all 3 tests pass.

### Step 0.2.5 — Run full apex-fuzz test suite

- [ ] Verify no regressions:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz
```
Expected: all tests pass.

### Step 0.2.6 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-fuzz/src/lib.rs && git commit -m "fix(apex-fuzz): observe() now adds winning inputs to corpus

P1 bug: observe() logged new coverage but never called corpus.add().
Now adds input when new branches are discovered and input is present.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Task 0.3: Fix mutate_with_index() on MOptScheduler (P2 bug)

**Crate:** `apex-fuzz`
**Modify:** `crates/apex-fuzz/src/scheduler.rs`

Two bugs: (1) `report_miss()` EMA update is identical to `report_hit()` — miss should decay weight. (2) No way to get mutator index from `mutate()`.

### Step 0.3.1 — Write failing test: report_hit increases, report_miss decreases

- [ ] Add to `#[cfg(test)] mod tests` in `/Users/ad/prj/bcov/crates/apex-fuzz/src/scheduler.rs`:

```rust
    #[test]
    fn report_hit_increases_and_miss_decreases_weight() {
        let mut sched_hit = make_scheduler(1);
        let mut sched_miss = make_scheduler(1);

        // Both start with applications=10, hits=5 (50% yield), ema=1.0
        sched_hit.stats[0].applications = 10;
        sched_hit.stats[0].coverage_hits = 5;
        sched_miss.stats[0].applications = 10;
        sched_miss.stats[0].coverage_hits = 5;

        sched_hit.report_hit(0);
        sched_miss.report_miss(0);

        // After hit: coverage_hits becomes 6, yield = 6/10 = 0.6
        // After miss: coverage_hits stays 5, yield should decay
        // Bug: currently both compute the same thing
        assert!(
            sched_hit.stats[0].ema_yield > sched_miss.stats[0].ema_yield,
            "hit ema ({}) should be > miss ema ({})",
            sched_hit.stats[0].ema_yield,
            sched_miss.stats[0].ema_yield
        );
    }
```

- [ ] Run to verify it fails:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz report_hit_increases_and_miss_decreases 2>&1 | tail -15
```
Expected: assertion failure — both EMA values are equal because `report_miss` has the same formula as `report_hit`.

### Step 0.3.2 — Fix report_miss() to decay weight

- [ ] In `/Users/ad/prj/bcov/crates/apex-fuzz/src/scheduler.rs`, replace `report_miss` (lines 82-93):

Replace:
```rust
    pub fn report_miss(&mut self, mutator_idx: usize) {
        if mutator_idx >= self.stats.len() {
            return;
        }
        let s = &mut self.stats[mutator_idx];
        let yield_now = if s.applications > 0 {
            s.coverage_hits as f64 / s.applications as f64
        } else {
            0.0
        };
        s.ema_yield = self.alpha * yield_now + (1.0 - self.alpha) * s.ema_yield;
    }
```

With:
```rust
    pub fn report_miss(&mut self, mutator_idx: usize) {
        if mutator_idx >= self.stats.len() {
            return;
        }
        let s = &mut self.stats[mutator_idx];
        // On a miss, decay the EMA toward zero (yield_now = 0.0)
        s.ema_yield = (1.0 - self.alpha) * s.ema_yield;
    }
```

- [ ] Run the test:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz report_hit_increases_and_miss_decreases
```
Expected: `test scheduler::tests::report_hit_increases_and_miss_decreases_weight ... ok`

### Step 0.3.3 — Write failing test for mutate_with_index

- [ ] Add to `tests` module in `/Users/ad/prj/bcov/crates/apex-fuzz/src/scheduler.rs`:

```rust
    #[test]
    fn mutate_with_index_returns_valid_index() {
        let mut scheduler = make_scheduler(5);
        let mut rng = StdRng::seed_from_u64(42);
        let input = b"hello";
        for _ in 0..50 {
            let (output, idx) = scheduler.mutate_with_index(input, &mut rng);
            assert!(idx < 5, "index {idx} out of range");
            // ConstMutator returns input unchanged
            assert_eq!(output, input);
        }
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz mutate_with_index_returns 2>&1 | head -15
```
Expected: compilation error — no method `mutate_with_index`.

### Step 0.3.4 — Implement mutate_with_index

- [ ] Add after the existing `mutate` method in `/Users/ad/prj/bcov/crates/apex-fuzz/src/scheduler.rs` (after line 66):

```rust
    pub fn mutate_with_index(&mut self, input: &[u8], rng: &mut dyn RngCore) -> (Vec<u8>, usize) {
        let idx = self.select(rng);
        self.stats[idx].applications += 1;
        let output = self.mutators[idx].mutate(input, rng);
        (output, idx)
    }
```

- [ ] Run the test:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz mutate_with_index_returns
```
Expected: `test scheduler::tests::mutate_with_index_returns_valid_index ... ok`

### Step 0.3.5 — Write test for weight divergence

- [ ] Add to tests:

```rust
    #[test]
    fn weights_diverge_after_hit_and_miss() {
        let mut scheduler = make_scheduler(2);
        scheduler.stats[0].applications = 10;
        scheduler.stats[1].applications = 10;

        // Hit mutator 0, miss mutator 1
        scheduler.report_hit(0);
        scheduler.report_miss(1);

        assert!(
            scheduler.stats[0].ema_yield > scheduler.stats[1].ema_yield,
            "hit mutator ema ({}) should exceed miss mutator ema ({})",
            scheduler.stats[0].ema_yield,
            scheduler.stats[1].ema_yield
        );
    }
```

- [ ] Run:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz weights_diverge_after
```
Expected: pass.

### Step 0.3.6 — Run full scheduler test suite

- [ ] Verify no regressions:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-fuzz scheduler::tests
```
Expected: all tests pass (note: `report_miss_zero_applications_yields_zero_now` may need updating if its expected value changed).

> **Check:** The existing test `report_miss_zero_applications_yields_zero_now` expects `ema = 0.1*0.0 + 0.9*1.0 = 0.9`. With the new formula: `ema = (1-0.1)*1.0 = 0.9`. Same result. No change needed.

> **Check:** The existing test `report_miss_updates_ema` checks `scheduler.stats[1].ema_yield < initial_ema`. With old formula: `0.1*(0/5) + 0.9*1.0 = 0.9 < 1.0`. With new formula: `0.9 * 1.0 = 0.9 < 1.0`. Same result. No change needed.

> **Check:** The existing test `report_hit_then_miss_sequence` checks that after hit then miss, ema changes. With new miss formula, the miss will purely decay. Still different from after_hit. Still passes.

### Step 0.3.7 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-fuzz/src/scheduler.rs && git commit -m "fix(apex-fuzz): report_miss() now decays weight + add mutate_with_index()

P2 bug: report_miss() used same EMA formula as report_hit().
Now decays toward zero on miss. Also adds mutate_with_index() to
return which mutator was used for feedback attribution.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Task 0.4: Extend ExecutionResult with input field

**Crate:** `apex-core`
**Modify:** `crates/apex-core/src/types.rs`

> **Ordering:** Implement this BEFORE Task 0.2, since 0.2 depends on the `input` field.

### Step 0.4.1 — Write failing test for input field

- [ ] Add to `#[cfg(test)] mod tests` in `/Users/ad/prj/bcov/crates/apex-core/src/types.rs`:

```rust
    #[test]
    fn execution_result_with_input_round_trips_serde() {
        let result = ExecutionResult {
            seed_id: SeedId::new(),
            status: ExecutionStatus::Pass,
            new_branches: vec![],
            trace: None,
            duration_ms: 42,
            stdout: "ok".into(),
            stderr: String::new(),
            input: Some(vec![1, 2, 3]),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ExecutionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.input, Some(vec![1, 2, 3]));
        assert_eq!(deserialized.duration_ms, 42);
    }

    #[test]
    fn execution_result_without_input_is_backward_compatible() {
        // JSON without "input" field should deserialize with input=None
        let json = r#"{
            "seed_id": "00000000-0000-0000-0000-000000000000",
            "status": "Pass",
            "new_branches": [],
            "trace": null,
            "duration_ms": 10,
            "stdout": "",
            "stderr": ""
        }"#;
        let result: ExecutionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.input, None);
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core execution_result_with_input 2>&1 | head -20
```
Expected: compilation error — no field `input` on `ExecutionResult`.

### Step 0.4.2 — Add input field to ExecutionResult

- [ ] In `/Users/ad/prj/bcov/crates/apex-core/src/types.rs`, add the field to the `ExecutionResult` struct (after line 393, the `stderr` field):

```rust
    /// The raw input bytes that produced this result (if available).
    /// Populated by sandbox runners to enable corpus feedback in observe().
    #[serde(default)]
    pub input: Option<Vec<u8>>,
```

The full struct becomes:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub seed_id: SeedId,
    pub status: ExecutionStatus,
    /// Branches newly covered by this run (delta vs oracle before the run).
    pub new_branches: Vec<BranchId>,
    pub trace: Option<ExecutionTrace>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    /// The raw input bytes that produced this result (if available).
    /// Populated by sandbox runners to enable corpus feedback in observe().
    #[serde(default)]
    pub input: Option<Vec<u8>>,
}
```

- [ ] Run the new tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core execution_result_with_input execution_result_without_input
```
Expected: both pass.

### Step 0.4.3 — Fix all construction sites across the workspace

Every `ExecutionResult { ... }` literal must now include `input: None` (or `input: Some(...)` where appropriate). The following files need updating:

**Production code (all set to `input: None`):**

1. `/Users/ad/prj/bcov/crates/apex-sandbox/src/python.rs` — lines 155 and 217
2. `/Users/ad/prj/bcov/crates/apex-sandbox/src/process.rs` — lines 164 and 193
3. `/Users/ad/prj/bcov/crates/apex-sandbox/src/javascript.rs` — line 149
4. `/Users/ad/prj/bcov/crates/apex-sandbox/src/firecracker.rs` — line 499
5. `/Users/ad/prj/bcov/crates/apex-sandbox/src/rust_test.rs` — line 122
6. `/Users/ad/prj/bcov/crates/apex-agent/src/orchestrator.rs` — lines 206, 691, 1145, 1223
7. `/Users/ad/prj/bcov/crates/apex-agent/src/driller.rs` — lines 230, 416
8. `/Users/ad/prj/bcov/crates/apex-concolic/src/python.rs` — line 980

**Test code (set to `input: None`):**

9. `/Users/ad/prj/bcov/crates/apex-agent/src/ledger.rs` — line 141
10. `/Users/ad/prj/bcov/crates/apex-coverage/src/oracle.rs` — lines 289, 500
11. `/Users/ad/prj/bcov/crates/apex-cli/tests/integration_test.rs` — lines 221, 235

For each file, add `input: None,` as the last field in every `ExecutionResult { ... }` struct literal.

- [ ] Fix all construction sites by adding `input: None,` to each.

- [ ] Verify the full workspace compiles:
```bash
cd /Users/ad/prj/bcov && cargo check --workspace 2>&1 | tail -5
```
Expected: no errors.

### Step 0.4.4 — Run full workspace tests

- [ ] Verify no regressions:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-core
```
Expected: all tests pass.

### Step 0.4.5 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add -u && git commit -m "feat(apex-core): add input field to ExecutionResult

Adds Optional<Vec<u8>> input field so observe() can feed winning
inputs back to the corpus. Uses #[serde(default)] for backward
compatibility with existing serialized data.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Task 0.5: MutationOperator + MutationKind types

**Crate:** `apex-coverage`
**Create:** `crates/apex-coverage/src/mutation.rs`
**Modify:** `crates/apex-coverage/src/lib.rs`

### Step 0.5.1 — Write failing test for MutationKind

- [ ] Create `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs` with tests only:

```rust
//! Mutation testing types — operator descriptors and results.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn mutation_kind_eq_and_hash() {
        let a = MutationKind::StatementDeletion;
        let b = MutationKind::StatementDeletion;
        let c = MutationKind::ConditionalNegation;
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&MutationKind::StatementDeletion));
        assert!(!set.contains(&MutationKind::ArithmeticReplace));
    }
}
```

- [ ] Add `pub mod mutation;` to `/Users/ad/prj/bcov/crates/apex-coverage/src/lib.rs`:

```rust
pub mod mutation;
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation_kind_eq 2>&1 | head -15
```
Expected: compilation error — `MutationKind` not found.

### Step 0.5.2 — Implement MutationKind

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
//! Mutation testing types — operator descriptors and results.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MutationKind {
    StatementDeletion,
    ConditionalNegation,
    ReturnValueChange,
    ArithmeticReplace,
    BoundaryShift,
    ExceptionRemoval,
    ConstantReplace,
}
```

- [ ] Run test:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation_kind_eq
```
Expected: pass.

### Step 0.5.3 — Write failing test for MutationOperator serde

- [ ] Add to tests in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
    #[test]
    fn mutation_operator_serde_round_trip() {
        let op = MutationOperator {
            kind: MutationKind::ConditionalNegation,
            file: "src/lib.py".to_string(),
            line: 42,
            original: "if x > 0:".to_string(),
            replacement: "if not (x > 0):".to_string(),
        };
        let json = serde_json::to_string(&op).unwrap();
        let deserialized: MutationOperator = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.kind, MutationKind::ConditionalNegation);
        assert_eq!(deserialized.file, "src/lib.py");
        assert_eq!(deserialized.line, 42);
        assert_eq!(deserialized.original, "if x > 0:");
        assert_eq!(deserialized.replacement, "if not (x > 0):");
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation_operator_serde 2>&1 | head -15
```
Expected: compilation error — `MutationOperator` not found.

### Step 0.5.4 — Implement MutationOperator

- [ ] Add after `MutationKind` in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationOperator {
    pub kind: MutationKind,
    pub file: String,
    pub line: u32,
    pub original: String,
    pub replacement: String,
}
```

- [ ] Run test:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation_operator_serde
```
Expected: pass.

### Step 0.5.5 — Write failing test for MutationResult

- [ ] Add to tests:

```rust
    #[test]
    fn mutation_result_killed_and_survived() {
        let op = MutationOperator {
            kind: MutationKind::StatementDeletion,
            file: "app.py".to_string(),
            line: 10,
            original: "x = 1".to_string(),
            replacement: "pass".to_string(),
        };

        let killed = MutationResult {
            operator: op.clone(),
            killed: true,
            killing_tests: vec!["test_foo".to_string()],
        };
        let survived = MutationResult {
            operator: op,
            killed: false,
            killing_tests: vec![],
        };

        assert!(killed.killed);
        assert_eq!(killed.killing_tests.len(), 1);
        assert!(!survived.killed);
        assert!(survived.killing_tests.is_empty());

        // Serde round-trip
        let json = serde_json::to_string(&killed).unwrap();
        let deserialized: MutationResult = serde_json::from_str(&json).unwrap();
        assert!(deserialized.killed);
        assert_eq!(deserialized.killing_tests, vec!["test_foo"]);
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation_result_killed 2>&1 | head -15
```
Expected: compilation error — `MutationResult` not found.

### Step 0.5.6 — Implement MutationResult

- [ ] Add after `MutationOperator` in `/Users/ad/prj/bcov/crates/apex-coverage/src/mutation.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    pub operator: MutationOperator,
    pub killed: bool,
    pub killing_tests: Vec<String>,
}
```

- [ ] Run all mutation tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-coverage mutation::tests
```
Expected: all 3 tests pass.

### Step 0.5.7 — Add serde_json dev-dependency if needed

- [ ] Check if `serde_json` is in apex-coverage's dependencies. Currently it is NOT in `Cargo.toml`. Add it to `[dev-dependencies]`:

```toml
[dev-dependencies]
proptest = { workspace = true }
serde_json = "1"
```

**File:** `/Users/ad/prj/bcov/crates/apex-coverage/Cargo.toml`

### Step 0.5.8 — Export the new types

- [ ] Add re-exports in `/Users/ad/prj/bcov/crates/apex-coverage/src/lib.rs`:

```rust
pub use mutation::{MutationKind, MutationOperator, MutationResult};
```

### Step 0.5.9 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-coverage/src/mutation.rs crates/apex-coverage/src/lib.rs crates/apex-coverage/Cargo.toml && git commit -m "feat(apex-coverage): add MutationKind, MutationOperator, MutationResult types

Foundation types for mutation testing subsystem. Supports 7 mutation
kinds with serde serialization.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Task 0.6: CPG wiring into apex-detect pipeline

**Crate:** `apex-detect`
**Modify:** `crates/apex-detect/src/context.rs`
**Modify:** `crates/apex-detect/Cargo.toml`

### Step 0.6.1 — Write failing test for AnalysisContext with cpg field

- [ ] Add to `#[cfg(test)] mod tests` in `/Users/ad/prj/bcov/crates/apex-detect/src/context.rs`:

```rust
    #[test]
    fn analysis_context_with_none_cpg_works() {
        let ctx = AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: Language::Python,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: HashMap::new(),
            fuzz_corpus: None,
            config: DetectConfig::default(),
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg: None,
        };
        assert!(ctx.cpg.is_none());
        let dbg = format!("{ctx:?}");
        assert!(dbg.contains("AnalysisContext"));
    }

    #[test]
    fn analysis_context_with_some_cpg_provides_access() {
        let mut cpg = apex_cpg::Cpg::new();
        cpg.add_node(apex_cpg::NodeKind::Method {
            name: "foo".into(),
            file: "test.py".into(),
            line: 1,
        });
        let ctx = AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: Language::Python,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: HashMap::new(),
            fuzz_corpus: None,
            config: DetectConfig::default(),
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg: Some(Arc::new(cpg)),
        };
        assert!(ctx.cpg.is_some());
        assert_eq!(ctx.cpg.as_ref().unwrap().node_count(), 1);
    }
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect analysis_context_with_none_cpg 2>&1 | head -20
```
Expected: compilation error — no field `cpg` on `AnalysisContext`.

### Step 0.6.2 — Add apex-cpg dependency to apex-detect

- [ ] In `/Users/ad/prj/bcov/crates/apex-detect/Cargo.toml`, add to `[dependencies]`:

```toml
apex-cpg = { path = "../apex-cpg" }
```

### Step 0.6.3 — Add cpg field to AnalysisContext

- [ ] In `/Users/ad/prj/bcov/crates/apex-detect/src/context.rs`, add the field to the struct (after `runner`):

```rust
    pub cpg: Option<Arc<apex_cpg::Cpg>>,
```

- [ ] Update the `Debug` impl to include cpg:

Replace:
```rust
impl fmt::Debug for AnalysisContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisContext")
            .field("target_root", &self.target_root)
            .field("language", &self.language)
            .field("file_paths", &self.file_paths.len())
            .field("source_cache", &self.source_cache.len())
            .field("fuzz_corpus", &self.fuzz_corpus)
            .field("runner", &"<CommandRunner>")
            .finish()
    }
}
```

With:
```rust
impl fmt::Debug for AnalysisContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisContext")
            .field("target_root", &self.target_root)
            .field("language", &self.language)
            .field("file_paths", &self.file_paths.len())
            .field("source_cache", &self.source_cache.len())
            .field("fuzz_corpus", &self.fuzz_corpus)
            .field("runner", &"<CommandRunner>")
            .field("cpg", &self.cpg.as_ref().map(|c| c.node_count()))
            .finish()
    }
}
```

### Step 0.6.4 — Fix all AnalysisContext construction sites

Every `AnalysisContext { ... }` literal must now include `cpg: None`. Search for construction sites:

- [ ] Find all construction sites:
```bash
cd /Users/ad/prj/bcov && grep -rn "AnalysisContext {" crates/ --include="*.rs" | grep -v "\.claude/"
```

- [ ] Add `cpg: None,` to each construction site.

- [ ] Update existing tests in `context.rs` (the two existing tests) to include `cpg: None,`:

In `debug_impl_does_not_dump_full_cache`:
```rust
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg: None,
```

In `debug_impl_with_no_corpus`:
```rust
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg: None,
```

### Step 0.6.5 — Run tests

- [ ] Run:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect context::tests
```
Expected: all 4 tests pass (2 existing + 2 new).

### Step 0.6.6 — Run full detect test suite

- [ ] Verify no regressions:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-detect
```
Expected: all tests pass.

### Step 0.6.7 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-detect/src/context.rs crates/apex-detect/Cargo.toml && git commit -m "feat(apex-detect): wire CPG into AnalysisContext

Adds Option<Arc<Cpg>> to AnalysisContext so security detectors can
query the Code Property Graph for taint flows and data dependencies.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Task 0.7: TaintSpecStore (runtime-extensible)

**Crate:** `apex-cpg`
**Create:** `crates/apex-cpg/src/taint_store.rs`
**Modify:** `crates/apex-cpg/src/lib.rs`

### Step 0.7.1 — Write failing test for python_defaults

- [ ] Create `/Users/ad/prj/bcov/crates/apex-cpg/src/taint_store.rs` with test only:

```rust
//! Runtime-extensible taint specification store.
//!
//! Replaces hardcoded `PYTHON_SOURCES`, `PYTHON_SINKS`, `PYTHON_SANITIZERS`
//! arrays with a mutable store that can be extended at runtime (e.g. from
//! LLM-discovered taint specs or user configuration).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taint::{PYTHON_SOURCES, PYTHON_SINKS, PYTHON_SANITIZERS};

    #[test]
    fn python_defaults_contains_all_hardcoded_sources() {
        let store = TaintSpecStore::python_defaults();
        for src in PYTHON_SOURCES {
            assert!(
                store.is_source(src),
                "missing source: {src}"
            );
        }
    }

    #[test]
    fn python_defaults_contains_all_hardcoded_sinks() {
        let store = TaintSpecStore::python_defaults();
        for sink in PYTHON_SINKS {
            assert!(
                store.is_sink(sink),
                "missing sink: {sink}"
            );
        }
    }

    #[test]
    fn python_defaults_contains_all_hardcoded_sanitizers() {
        let store = TaintSpecStore::python_defaults();
        for san in PYTHON_SANITIZERS {
            assert!(
                store.is_sanitizer(san),
                "missing sanitizer: {san}"
            );
        }
    }
}
```

- [ ] Add `pub mod taint_store;` to `/Users/ad/prj/bcov/crates/apex-cpg/src/lib.rs`:

```rust
pub mod taint_store;
```

- [ ] Run to verify failure:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-cpg python_defaults 2>&1 | head -15
```
Expected: compilation error — `TaintSpecStore` not found.

### Step 0.7.2 — Implement TaintSpecStore with python_defaults

- [ ] Add above `#[cfg(test)]` in `/Users/ad/prj/bcov/crates/apex-cpg/src/taint_store.rs`:

```rust
//! Runtime-extensible taint specification store.
//!
//! Replaces hardcoded `PYTHON_SOURCES`, `PYTHON_SINKS`, `PYTHON_SANITIZERS`
//! arrays with a mutable store that can be extended at runtime (e.g. from
//! LLM-discovered taint specs or user configuration).

use std::collections::HashSet;

use crate::taint::{PYTHON_SANITIZERS, PYTHON_SINKS, PYTHON_SOURCES};

#[derive(Debug, Clone, Default)]
pub struct TaintSpecStore {
    sources: HashSet<String>,
    sinks: HashSet<String>,
    sanitizers: HashSet<String>,
}

impl TaintSpecStore {
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a store pre-populated with all hardcoded Python taint specs.
    pub fn python_defaults() -> Self {
        let mut store = Self::new();
        for s in PYTHON_SOURCES {
            store.sources.insert(s.to_string());
        }
        for s in PYTHON_SINKS {
            store.sinks.insert(s.to_string());
        }
        for s in PYTHON_SANITIZERS {
            store.sanitizers.insert(s.to_string());
        }
        store
    }

    pub fn add_source(&mut self, name: String) {
        self.sources.insert(name);
    }

    pub fn add_sink(&mut self, name: String) {
        self.sinks.insert(name);
    }

    pub fn add_sanitizer(&mut self, name: String) {
        self.sanitizers.insert(name);
    }

    pub fn is_source(&self, name: &str) -> bool {
        self.sources.contains(name)
    }

    pub fn is_sink(&self, name: &str) -> bool {
        self.sinks.contains(name)
    }

    pub fn is_sanitizer(&self, name: &str) -> bool {
        self.sanitizers.contains(name)
    }

    pub fn sources(&self) -> &HashSet<String> {
        &self.sources
    }

    pub fn sinks(&self) -> &HashSet<String> {
        &self.sinks
    }

    pub fn sanitizers(&self) -> &HashSet<String> {
        &self.sanitizers
    }
}
```

- [ ] Run the defaults tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-cpg python_defaults
```
Expected: all 3 tests pass.

### Step 0.7.3 — Write failing test for add and query

- [ ] Add to tests in `/Users/ad/prj/bcov/crates/apex-cpg/src/taint_store.rs`:

```rust
    #[test]
    fn add_source_and_query() {
        let mut store = TaintSpecStore::new();
        assert!(!store.is_source("custom.input"));
        store.add_source("custom.input".to_string());
        assert!(store.is_source("custom.input"));
    }

    #[test]
    fn add_sink_and_query() {
        let mut store = TaintSpecStore::new();
        assert!(!store.is_sink("dangerous.func"));
        store.add_sink("dangerous.func".to_string());
        assert!(store.is_sink("dangerous.func"));
    }

    #[test]
    fn add_sanitizer_and_query() {
        let mut store = TaintSpecStore::new();
        assert!(!store.is_sanitizer("bleach.clean"));
        store.add_sanitizer("bleach.clean".to_string());
        assert!(store.is_sanitizer("bleach.clean"));
    }

    #[test]
    fn accessors_return_correct_sets() {
        let mut store = TaintSpecStore::new();
        store.add_source("s1".to_string());
        store.add_source("s2".to_string());
        store.add_sink("k1".to_string());
        store.add_sanitizer("z1".to_string());

        assert_eq!(store.sources().len(), 2);
        assert_eq!(store.sinks().len(), 1);
        assert_eq!(store.sanitizers().len(), 1);
        assert!(store.sources().contains("s1"));
        assert!(store.sources().contains("s2"));
        assert!(store.sinks().contains("k1"));
        assert!(store.sanitizers().contains("z1"));
    }

    #[test]
    fn empty_store_returns_false_for_all_queries() {
        let store = TaintSpecStore::new();
        assert!(!store.is_source("anything"));
        assert!(!store.is_sink("anything"));
        assert!(!store.is_sanitizer("anything"));
        assert!(store.sources().is_empty());
        assert!(store.sinks().is_empty());
        assert!(store.sanitizers().is_empty());
    }

    #[test]
    fn clone_produces_independent_copy() {
        let mut store = TaintSpecStore::new();
        store.add_source("a".to_string());
        let mut cloned = store.clone();
        cloned.add_source("b".to_string());
        assert!(!store.is_source("b"), "original should not see cloned addition");
        assert!(cloned.is_source("a"));
        assert!(cloned.is_source("b"));
    }
```

- [ ] Run all taint_store tests:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-cpg taint_store::tests
```
Expected: all 9 tests pass (3 defaults + 6 new).

### Step 0.7.4 — Export from lib.rs

- [ ] Add re-export in `/Users/ad/prj/bcov/crates/apex-cpg/src/lib.rs`:

```rust
pub use taint_store::TaintSpecStore;
```

### Step 0.7.5 — Run full apex-cpg test suite

- [ ] Verify no regressions:
```bash
cd /Users/ad/prj/bcov && cargo test -p apex-cpg
```
Expected: all tests pass.

### Step 0.7.6 — Commit

- [ ] Commit:
```bash
cd /Users/ad/prj/bcov && git add crates/apex-cpg/src/taint_store.rs crates/apex-cpg/src/lib.rs && git commit -m "feat(apex-cpg): add TaintSpecStore for runtime-extensible taint specs

Wraps hardcoded PYTHON_SOURCES/SINKS/SANITIZERS arrays in a mutable
HashSet-based store. Allows runtime addition of taint specs from
LLM discovery or user configuration.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Implementation Order

Tasks have the following dependency:

```
Task 0.1 (LlmClient)         — independent
Task 0.4 (ExecutionResult)    — independent, but MUST come before 0.2
Task 0.2 (observe fix)        — depends on 0.4
Task 0.3 (scheduler fix)      — independent
Task 0.5 (MutationOperator)   — independent
Task 0.6 (CPG wiring)         — independent
Task 0.7 (TaintSpecStore)     — independent
```

Recommended execution order:
1. Task 0.1 (LlmClient)
2. Task 0.4 (ExecutionResult input field)
3. Task 0.2 (observe() corpus fix) — needs 0.4
4. Task 0.3 (scheduler fix)
5. Task 0.5 (MutationOperator types)
6. Task 0.6 (CPG wiring)
7. Task 0.7 (TaintSpecStore)

Tasks 0.1, 0.3, 0.5, 0.6, 0.7 are fully independent and can be parallelized.

## Final Verification

After all 7 tasks are complete:

- [ ] Full workspace build:
```bash
cd /Users/ad/prj/bcov && cargo build --workspace
```

- [ ] Full workspace tests:
```bash
cd /Users/ad/prj/bcov && cargo test --workspace
```

- [ ] Verify no clippy warnings in changed files:
```bash
cd /Users/ad/prj/bcov && cargo clippy -p apex-core -p apex-fuzz -p apex-coverage -p apex-detect -p apex-cpg -- -D warnings
```
