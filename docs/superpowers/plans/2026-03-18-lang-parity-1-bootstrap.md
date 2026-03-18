<!-- status: DONE -->

# Language Parity Plan 1: Ruby & Kotlin Bootstrap

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring Ruby and Kotlin up to "Mid" tier — coverage instrumentation, sandbox, and per-test indexing so that downstream plans (synthesis, concolic) have a foundation.

**Architecture:** Ruby already has a test runner (`apex-lang/src/ruby.rs`) and partial instrumentor (`apex-instrument/src/ruby.rs`). Kotlin shares JaCoCo with Java but lacks a per-test indexer. WASM is excluded from bootstrap — its partial support is sufficient for now.

**Tech Stack:** SimpleCov (Ruby coverage), JaCoCo (Kotlin coverage), RSpec/Minitest, JUnit

**Depends on:** Nothing — this is the foundation plan.

---

## Chunk 1: Ruby Coverage Sandbox

### Task 1: Ruby Test Sandbox

**Files:**
- Create: `crates/apex-sandbox/src/ruby.rs`
- Modify: `crates/apex-sandbox/src/lib.rs` (add `pub mod ruby;`)

- [ ] **Step 1: Write the failing test**

```rust
// In crates/apex-sandbox/src/ruby.rs
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{BranchId, InputSeed, SeedOrigin};
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn ruby_sandbox_language() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox = RubyTestSandbox::new(
            oracle,
            Arc::new(HashMap::new()),
            std::path::PathBuf::from("/tmp/test"),
        );
        assert_eq!(sandbox.language(), apex_core::types::Language::Ruby);
    }

    #[test]
    fn ruby_sandbox_constructs() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox = RubyTestSandbox::new(
            oracle,
            Arc::new(HashMap::new()),
            std::path::PathBuf::from("/tmp/test"),
        );
        assert_eq!(sandbox.timeout_ms, 30_000);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p apex-sandbox ruby_sandbox`
Expected: FAIL — `RubyTestSandbox` not defined

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/apex-sandbox/src/ruby.rs
use apex_core::{
    error::{ApexError, Result},
    types::{BranchId, ExecutionResult, InputSeed, Language},
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

pub struct RubyTestSandbox {
    oracle: Arc<CoverageOracle>,
    file_paths: Arc<HashMap<u64, PathBuf>>,
    target_dir: PathBuf,
    pub timeout_ms: u64,
}

impl RubyTestSandbox {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        file_paths: Arc<HashMap<u64, PathBuf>>,
        target_dir: PathBuf,
    ) -> Self {
        Self {
            oracle,
            file_paths,
            target_dir,
            timeout_ms: 30_000,
        }
    }

    pub fn language(&self) -> Language {
        Language::Ruby
    }
}
```

- [ ] **Step 4: Add module to lib.rs**

In `crates/apex-sandbox/src/lib.rs`, add `pub mod ruby;` alongside the existing modules.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p apex-sandbox ruby_sandbox`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/apex-sandbox/src/ruby.rs crates/apex-sandbox/src/lib.rs
git commit -m "feat(sandbox): add RubyTestSandbox skeleton"
```

---

### Task 2: Ruby SimpleCov JSON Parsing in Sandbox

**Files:**
- Modify: `crates/apex-sandbox/src/ruby.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parse_simplecov_json_extracts_branches() {
    let json = r#"{
        "RSpec": {
            "coverage": {
                "/app/lib/foo.rb": {
                    "lines": [1, 1, null, 0, 1, 0, null]
                }
            }
        }
    }"#;
    let (all, covered) = parse_simplecov_branches(json).unwrap();
    // Lines with values: 1,2,4,5,6 (0-indexed → 1-indexed: lines 1,2,4,5,6)
    // Covered (>0): lines 1,2,5
    // Uncovered (==0): lines 4,6
    assert!(all.len() >= 5);
    assert!(covered.len() >= 3);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p apex-sandbox parse_simplecov`
Expected: FAIL — function not defined

- [ ] **Step 3: Implement SimpleCov parser**

```rust
/// Parse SimpleCov JSON into (all_branches, covered_branches).
pub fn parse_simplecov_branches(json: &str) -> Result<(Vec<BranchId>, Vec<BranchId>)> {
    let parsed: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ApexError::Sandbox(format!("simplecov JSON: {e}")))?;

    let mut all = Vec::new();
    let mut covered = Vec::new();

    // SimpleCov format: { "<runner>": { "coverage": { "<file>": { "lines": [...] } } } }
    for (_runner, runner_data) in parsed.as_object().into_iter().flatten() {
        let coverage = runner_data.get("coverage").and_then(|c| c.as_object());
        for (file, file_data) in coverage.into_iter().flatten() {
            let file_id = apex_core::types::file_id(file);
            let lines = file_data.get("lines").and_then(|l| l.as_array());
            for (idx, val) in lines.into_iter().flatten().enumerate() {
                if let Some(count) = val.as_i64() {
                    let line = (idx + 1) as u32;
                    let bid = BranchId::new(file_id, line, 0, 0);
                    all.push(bid);
                    if count > 0 {
                        covered.push(bid);
                    }
                }
                // null means non-executable line — skip
            }
        }
    }
    Ok((all, covered))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p apex-sandbox parse_simplecov`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-sandbox/src/ruby.rs
git commit -m "feat(sandbox): SimpleCov JSON parser for Ruby coverage"
```

---

### Task 3: Ruby Sandbox `run` Method

**Files:**
- Modify: `crates/apex-sandbox/src/ruby.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn ruby_sandbox_run_returns_execution_result() {
    // This tests the structure, not actual Ruby execution
    let oracle = Arc::new(CoverageOracle::new());
    let sandbox = RubyTestSandbox::new(
        oracle,
        Arc::new(HashMap::new()),
        std::path::PathBuf::from("/nonexistent"),
    );
    let seed = InputSeed {
        data: b"puts 'hello'".to_vec(),
        origin: SeedOrigin::User,
        metadata: HashMap::new(),
    };
    // Should fail gracefully (target dir doesn't exist)
    let result = sandbox.run(&seed).await;
    assert!(result.is_err() || result.unwrap().new_branches.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p apex-sandbox ruby_sandbox_run`
Expected: FAIL — `run` method not defined

- [ ] **Step 3: Implement run method**

Follow the pattern from `python.rs`: decode seed as UTF-8 Ruby source, write to temp file, run `COVERAGE=true bundle exec rspec <file>` or `ruby <file>`, parse SimpleCov JSON output, compare against oracle.

```rust
impl RubyTestSandbox {
    pub async fn run(&self, seed: &InputSeed) -> Result<ExecutionResult> {
        let code = String::from_utf8(seed.data.clone())
            .map_err(|e| ApexError::Sandbox(format!("invalid UTF-8: {e}")))?;

        let test_file = self.target_dir.join("apex_probe_test.rb");
        let coverage_dir = self.target_dir.join("coverage");

        // Prepend SimpleCov setup
        let wrapped = format!(
            "require 'simplecov'\nrequire 'simplecov-json'\n\
             SimpleCov.start do\n  formatter SimpleCov::Formatter::JSONFormatter\n\
             coverage_dir '{}'\nend\n\n{}",
            coverage_dir.display(),
            code
        );

        std::fs::write(&test_file, &wrapped)
            .map_err(|e| ApexError::Sandbox(format!("write test: {e}")))?;

        let spec = apex_core::command::CommandSpec::new("ruby", &self.target_dir)
            .args([test_file.to_string_lossy().as_ref()])
            .timeout_ms(self.timeout_ms);

        let _output = apex_core::command::RealCommandRunner
            .run_command(&spec)
            .await?;

        // Parse coverage
        let cov_file = coverage_dir.join(".resultset.json");
        let new_branches = if cov_file.exists() {
            let json = std::fs::read_to_string(&cov_file)
                .map_err(|e| ApexError::Sandbox(format!("read coverage: {e}")))?;
            let (_all, covered) = parse_simplecov_branches(&json)?;
            covered
                .into_iter()
                .filter(|b| !self.oracle.is_covered(b))
                .collect()
        } else {
            vec![]
        };

        // Cleanup
        let _ = std::fs::remove_file(&test_file);
        let _ = std::fs::remove_dir_all(&coverage_dir);

        Ok(ExecutionResult {
            new_branches,
            ..Default::default()
        })
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p apex-sandbox ruby`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-sandbox/src/ruby.rs
git commit -m "feat(sandbox): Ruby sandbox run method with SimpleCov integration"
```

---

## Chunk 2: Kotlin Per-Test Index

### Task 4: Kotlin Indexer

**Files:**
- Create: `crates/apex-index/src/kotlin.rs`
- Modify: `crates/apex-index/src/lib.rs` (add `pub mod kotlin;`)

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jacoco_test_name() {
        // JaCoCo test names from gradle: "com.example.FooTest > testBar PASSED"
        let names = parse_gradle_test_list(
            "com.example.FooTest > testBar PASSED\ncom.example.FooTest > testBaz PASSED\n",
        );
        assert_eq!(names, vec!["com.example.FooTest.testBar", "com.example.FooTest.testBaz"]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p apex-index parse_jacoco_test`
Expected: FAIL

- [ ] **Step 3: Implement Kotlin indexer**

Pattern: follow `java.rs` — Kotlin uses the same JaCoCo coverage and Gradle test runner. The indexer:
1. Enumerates tests via `./gradlew test --dry-run` or `./gradlew test --tests '*' --list-tests`
2. Runs each test individually with JaCoCo: `./gradlew test --tests "ClassName.testMethod" jacocoTestReport`
3. Parses JaCoCo XML per test run
4. Aggregates into `BranchIndex`

```rust
// crates/apex-index/src/kotlin.rs
use apex_core::error::Result;
use std::path::Path;
use crate::BranchIndex;

pub fn parse_gradle_test_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.contains("PASSED") || l.contains("FAILED"))
        .filter_map(|l| {
            let parts: Vec<&str> = l.split(" > ").collect();
            if parts.len() >= 2 {
                let class = parts[0].trim();
                let method = parts[1].split_whitespace().next()?;
                Some(format!("{class}.{method}"))
            } else {
                None
            }
        })
        .collect()
}

pub async fn build_kotlin_index(target_root: &Path, parallelism: usize) -> Result<BranchIndex> {
    // Reuse Java index logic — Kotlin shares JaCoCo and Gradle
    crate::java::build_java_index(target_root, parallelism).await
}
```

- [ ] **Step 4: Register in lib.rs**

Add `pub mod kotlin;` and `pub use kotlin::build_kotlin_index;` to `crates/apex-index/src/lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo nextest run -p apex-index kotlin`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/apex-index/src/kotlin.rs crates/apex-index/src/lib.rs
git commit -m "feat(index): add Kotlin per-test indexer via JaCoCo"
```

---

### Task 5: Wire Language Dispatch

**Files:**
- Modify: `crates/apex-cli/src/lib.rs` (index dispatch match arm)

- [ ] **Step 1: Find the index dispatch**

Search for the `match` on `Language` in the index command handler. Add:
```rust
Language::Kotlin => apex_index::build_kotlin_index(&target, parallelism).await?,
```

- [ ] **Step 2: Verify Ruby sandbox is wired in CLI**

Check that the sandbox dispatch has a `Language::Ruby` arm. If not, add it.

- [ ] **Step 3: Run full build**

Run: `cargo build --workspace`
Expected: Clean build

- [ ] **Step 4: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat(cli): wire Kotlin index and Ruby sandbox dispatch"
```
