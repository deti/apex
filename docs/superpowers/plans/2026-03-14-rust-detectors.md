<!-- status: DONE -->
# Rust Self-Analysis Detectors Implementation Plan

> **For agentic workers:** REQUIRED: Use fleet crew agents (security-detect crew) for implementation. Each task creates one detector file. Tasks are independent and can run in parallel. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 8 Rust-specific pattern detectors to `apex-detect` so APEX can find bugs in Rust codebases — including its own. Each detector found at least one confirmed bug during the 6-crew review.

**Architecture:** Each detector is a unit struct implementing `Detector` trait, using regex-based line scanning of `ctx.source_cache`. All detectors go in `crates/apex-detect/src/detectors/`. Registration via `pipeline.rs::from_config()` with `Language::Rust` guards where appropriate. Follows the exact `HardcodedSecretDetector` pattern.

**Tech Stack:** Rust, async-trait, regex, LazyLock, uuid

---

## Detector Recipe (all tasks follow this)

```
1. Create crates/apex-detect/src/detectors/<name>.rs
2. Add `pub mod <name>;` + `pub use <name>::<Struct>;` in detectors/mod.rs
3. Add `if cfg.enabled.contains(&"<key>".into()) { detectors.push(Box::new(<Struct>)); }` in pipeline.rs
4. Add "<key>" to default_enabled() in config.rs
5. Tests: positive cases (code that should fire) + negative cases (similar code that should NOT fire)
6. Run: cargo test -p apex-detect <name>
7. Commit
```

## File Map

| File | Action |
|------|--------|
| `crates/apex-detect/src/detectors/mod.rs` | Add 8 `pub mod` + `pub use` lines |
| `crates/apex-detect/src/pipeline.rs` | Add 8 registration blocks in `from_config()` |
| `crates/apex-detect/src/config.rs` | Add 8 keys to `default_enabled()` |
| `crates/apex-detect/src/detectors/discarded_async_result.rs` | New — detector #1 |
| `crates/apex-detect/src/detectors/mixed_bool_ops.rs` | New — detector #2 |
| `crates/apex-detect/src/detectors/partial_cmp_unwrap.rs` | New — detector #3 |
| `crates/apex-detect/src/detectors/substring_security.rs` | New — detector #4 |
| `crates/apex-detect/src/detectors/vecdeque_partial.rs` | New — detector #5 |
| `crates/apex-detect/src/detectors/process_exit_in_lib.rs` | New — detector #6 |
| `crates/apex-detect/src/detectors/unsafe_send_sync.rs` | New — detector #7 |
| `crates/apex-detect/src/detectors/duplicated_fn.rs` | New — detector #8 |

---

## Task 0: Registration scaffold

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/mod.rs`
- Modify: `crates/apex-detect/src/pipeline.rs`
- Modify: `crates/apex-detect/src/config.rs`

Prepare the registration points so detector tasks can be implemented independently.

- [ ] **Step 1: Add module declarations to mod.rs**

Add to `crates/apex-detect/src/detectors/mod.rs`:

```rust
pub mod discarded_async_result;
pub mod mixed_bool_ops;
pub mod partial_cmp_unwrap;
pub mod substring_security;
pub mod vecdeque_partial;
pub mod process_exit_in_lib;
pub mod unsafe_send_sync;
pub mod duplicated_fn;

pub use discarded_async_result::DiscardedAsyncResultDetector;
pub use mixed_bool_ops::MixedBoolOpsDetector;
pub use partial_cmp_unwrap::PartialCmpUnwrapDetector;
pub use substring_security::SubstringSecurityDetector;
pub use vecdeque_partial::VecdequePartialDetector;
pub use process_exit_in_lib::ProcessExitInLibDetector;
pub use unsafe_send_sync::UnsafeSendSyncDetector;
pub use duplicated_fn::DuplicatedFnDetector;
```

- [ ] **Step 2: Add registration in pipeline.rs::from_config()**

Add after the existing detector registrations:

```rust
// Rust-specific detectors
if cfg.enabled.contains(&"discarded-async-result".into()) {
    detectors.push(Box::new(DiscardedAsyncResultDetector));
}
if cfg.enabled.contains(&"mixed-bool-ops".into()) {
    detectors.push(Box::new(MixedBoolOpsDetector));
}
if cfg.enabled.contains(&"partial-cmp-unwrap".into()) {
    detectors.push(Box::new(PartialCmpUnwrapDetector));
}
if cfg.enabled.contains(&"substring-security".into()) {
    detectors.push(Box::new(SubstringSecurityDetector));
}
if cfg.enabled.contains(&"vecdeque-partial".into()) {
    detectors.push(Box::new(VecdequePartialDetector));
}
if cfg.enabled.contains(&"process-exit-in-lib".into()) {
    detectors.push(Box::new(ProcessExitInLibDetector));
}
if cfg.enabled.contains(&"unsafe-send-sync".into()) {
    detectors.push(Box::new(UnsafeSendSyncDetector));
}
if cfg.enabled.contains(&"duplicated-fn".into()) {
    detectors.push(Box::new(DuplicatedFnDetector));
}
```

- [ ] **Step 3: Add keys to default_enabled() in config.rs**

Add to the `default_enabled()` vec:

```rust
"discarded-async-result", "mixed-bool-ops", "partial-cmp-unwrap",
"substring-security", "vecdeque-partial", "process-exit-in-lib",
"unsafe-send-sync", "duplicated-fn",
```

- [ ] **Step 4: Create stub files for each detector**

Create 8 files, each with a minimal stub so the project compiles:

```rust
// crates/apex-detect/src/detectors/<name>.rs
use crate::{context::AnalysisContext, finding::Finding, Detector};
use apex_core::error::Result;
use async_trait::async_trait;

pub struct <StructName>;

#[async_trait]
impl Detector for <StructName> {
    fn name(&self) -> &str { "<config-key>" }
    async fn analyze(&self, _ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        Ok(Vec::new()) // stub
    }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p apex-detect`
Expected: PASS (all stubs compile)

- [ ] **Step 6: Commit**

```bash
git add crates/apex-detect/src/
git commit -m "feat: scaffold 8 Rust-specific detector stubs"
```

---

## Task 1: `discarded-async-result` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/discarded_async_result.rs`

Catches `let _ = expr.await;` — silently discarding async Results. CWE-252.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use std::path::PathBuf;

    fn make_ctx(code: &str) -> AnalysisContext {
        let mut ctx = AnalysisContext::test_default();
        ctx.language = Language::Rust;
        ctx.source_cache.insert(PathBuf::from("src/lib.rs"), code.to_string());
        ctx
    }

    #[tokio::test]
    async fn detects_discarded_await_result() {
        let ctx = make_ctx("fn foo() {\n    let _ = bar().await;\n}");
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2));
        assert!(findings[0].cwe_ids.contains(&252));
    }

    #[tokio::test]
    async fn ignores_assigned_await() {
        let ctx = make_ctx("fn foo() {\n    let result = bar().await;\n}");
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_question_mark_await() {
        let ctx = make_ctx("fn foo() {\n    bar().await?;\n}");
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_with_method_chain() {
        let ctx = make_ctx("fn f() {\n    let _ = strategy.observe(result).await;\n}");
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_non_rust() {
        let mut ctx = make_ctx("let _ = foo().await;");
        ctx.language = Language::Python;
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut ctx = AnalysisContext::test_default();
        ctx.language = Language::Rust;
        ctx.source_cache.insert(
            PathBuf::from("tests/integration.rs"),
            "let _ = foo().await;".to_string(),
        );
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p apex-detect discarded_async_result`
Expected: FAIL (stub returns empty vec)

- [ ] **Step 3: Implement the detector**

```rust
use crate::{context::AnalysisContext, finding::*, Detector};
use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use std::sync::LazyLock;
use uuid::Uuid;

static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"let\s+_\s*=\s*.*\.await\s*;").unwrap()
});

pub struct DiscardedAsyncResultDetector;

#[async_trait]
impl Detector for DiscardedAsyncResultDetector {
    fn name(&self) -> &str { "discarded-async-result" }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::Rust {
            return Ok(Vec::new());
        }

        let mut findings = Vec::new();
        for (path, source) in &ctx.source_cache {
            if super::util::is_test_file(path) {
                continue;
            }
            for (i, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if super::util::is_comment(trimmed, ctx.language) {
                    continue;
                }
                if PATTERN.is_match(trimmed) {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some((i + 1) as u32),
                        title: "Discarded async Result".into(),
                        description: format!(
                            "`let _ = ...await` silently discards errors. \
                             Use `if let Err(e) = ...await {{ warn!(...) }}` or propagate with `?`."
                        ),
                        evidence: vec![Evidence::StaticAnalysis {
                            tool: self.name().into(),
                            detail: trimmed.to_string(),
                        }],
                        covered: false,
                        suggestion: "Log or propagate the error instead of discarding".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![252],
                    });
                }
            }
        }
        Ok(findings)
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect discarded_async_result`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/detectors/discarded_async_result.rs
git commit -m "feat: add discarded-async-result detector (CWE-252)"
```

---

## Task 2: `mixed-bool-ops` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/mixed_bool_ops.rs`

Catches `a || b && c` without parentheses. CWE-783.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_mixed_ops_no_parens() {
    let ctx = make_ctx("if x.contains('[') || x.contains('.') && x.contains('>') {");
    let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
    assert!(findings[0].cwe_ids.contains(&783));
}

#[tokio::test]
async fn allows_parenthesized() {
    let ctx = make_ctx("if (x.contains('[') || x.contains('.')) && x.contains('>') {");
    let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn allows_single_operator_type() {
    let ctx = make_ctx("if a && b && c {");
    let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}
```

- [ ] **Step 2: Implement**

Pattern: line contains both `||` and `&&` but no parentheses grouping them.

```rust
static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    // Matches: expr || expr && expr  (no parens around the || group)
    // Negative lookahead for parens is hard in regex — instead, check:
    // line has both || and &&, and no '(' before the ||
    regex::Regex::new(r"[^(]\s*\|\|\s*[^(].*&&").unwrap()
});
```

Use `Severity::Medium`, `FindingCategory::LogicBug`, `cwe_ids: vec![783]`.

- [ ] **Step 3: Run tests, commit**

Run: `cargo test -p apex-detect mixed_bool_ops`

```bash
git commit -m "feat: add mixed-bool-ops detector (CWE-783)"
```

---

## Task 3: `partial-cmp-unwrap` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/partial_cmp_unwrap.rs`

Catches `.partial_cmp(...).unwrap()` — panics on NaN. CWE-754.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_partial_cmp_unwrap() {
    let ctx = make_ctx("scores.sort_by(|a, b| a.partial_cmp(b).unwrap())");
    let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}

#[tokio::test]
async fn allows_unwrap_or() {
    let ctx = make_ctx("scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))");
    let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn allows_total_cmp() {
    let ctx = make_ctx("scores.sort_by(|a, b| a.total_cmp(b))");
    let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}
```

- [ ] **Step 2: Implement**

```rust
static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"partial_cmp\([^)]*\)\s*\.unwrap\(\)").unwrap()
});
```

`Severity::Medium`, `FindingCategory::PanicPath`, `cwe_ids: vec![754]`.
Suggestion: "Use `.unwrap_or(std::cmp::Ordering::Equal)` or `.total_cmp()` for NaN safety."

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add partial-cmp-unwrap detector (CWE-754)"
```

---

## Task 4: `substring-security` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/substring_security.rs`

Catches `.contains()` in security-critical functions (taint matching, auth). CWE-183.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_contains_in_is_sink() {
    let ctx = make_ctx(r#"
pub fn is_sink(&self, name: &str) -> bool {
    self.sinks.iter().any(|s| name.contains(s.as_str()))
}
"#);
    let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}

#[tokio::test]
async fn allows_exact_match() {
    let ctx = make_ctx(r#"
pub fn is_sink(&self, name: &str) -> bool {
    self.sinks.iter().any(|s| name == s.as_str())
}
"#);
    let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn allows_contains_in_normal_function() {
    let ctx = make_ctx(r#"
fn search(&self, query: &str) -> bool {
    self.items.iter().any(|s| s.contains(query))
}
"#);
    let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}
```

- [ ] **Step 2: Implement**

Two-phase detection:
1. Check if current function name matches security-critical patterns: `is_source`, `is_sink`, `is_sanitizer`, `is_trusted`, `is_authorized`, `check_permission`
2. Within those functions, flag `.contains(` calls

```rust
// Track function context while scanning lines
let mut in_security_fn = false;
for (i, line) in source.lines().enumerate() {
    let trimmed = line.trim();
    if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
        in_security_fn = SECURITY_FN_PATTERN.is_match(trimmed);
    }
    if in_security_fn && trimmed.contains(".contains(") && !is_comment(trimmed, ctx.language) {
        // emit finding
    }
}
```

`Severity::High`, `FindingCategory::SecuritySmell`, `cwe_ids: vec![183]`.

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add substring-security detector (CWE-183)"
```

---

## Task 5: `vecdeque-partial` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/vecdeque_partial.rs`

Catches `.as_slices().0` — only using first half of VecDeque data. CWE-682.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_as_slices_dot_zero() {
    let ctx = make_ctx("let data = ring.as_slices().0;");
    let findings = VecdequePartialDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}

#[tokio::test]
async fn allows_destructured_as_slices() {
    let ctx = make_ctx("let (a, b) = ring.as_slices();");
    let findings = VecdequePartialDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn allows_as_slices_dot_one() {
    // Using .1 is unusual but not the same bug
    let ctx = make_ctx("let data = ring.as_slices().1;");
    let findings = VecdequePartialDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty()); // .1 alone is also partial, but different intent
}
```

- [ ] **Step 2: Implement**

```rust
static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\.as_slices\(\)\s*\.0").unwrap()
});
```

`Severity::High`, `FindingCategory::LogicBug`, `cwe_ids: vec![682]`.

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add vecdeque-partial detector (CWE-682)"
```

---

## Task 6: `process-exit-in-lib` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/process_exit_in_lib.rs`

Catches `std::process::exit()` in non-main files. CWE-705.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_exit_in_lib() {
    let mut ctx = AnalysisContext::test_default();
    ctx.language = Language::Rust;
    ctx.source_cache.insert(
        PathBuf::from("src/lib.rs"),
        "fn fail() { std::process::exit(1); }".to_string(),
    );
    let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}

#[tokio::test]
async fn allows_exit_in_main() {
    let mut ctx = AnalysisContext::test_default();
    ctx.language = Language::Rust;
    ctx.source_cache.insert(
        PathBuf::from("src/main.rs"),
        "fn main() { std::process::exit(1); }".to_string(),
    );
    let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn detects_short_form() {
    let mut ctx = AnalysisContext::test_default();
    ctx.language = Language::Rust;
    ctx.source_cache.insert(
        PathBuf::from("src/cli.rs"),
        "use std::process;\nprocess::exit(1);".to_string(),
    );
    let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}
```

- [ ] **Step 2: Implement**

```rust
static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(std::)?process::exit\s*\(").unwrap()
});

// In analyze():
for (path, source) in &ctx.source_cache {
    // Skip main.rs — exit is legitimate there
    if path.file_name().map(|f| f == "main.rs").unwrap_or(false) {
        continue;
    }
    // ... scan lines for PATTERN
}
```

`Severity::Medium`, `FindingCategory::LogicBug`, `cwe_ids: vec![705]`.

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add process-exit-in-lib detector (CWE-705)"
```

---

## Task 7: `unsafe-send-sync` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/unsafe_send_sync.rs`

Catches `unsafe impl Send/Sync` without `// SAFETY:` comment. CWE-362.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_bare_unsafe_send() {
    let ctx = make_ctx("unsafe impl Send for Foo {}");
    let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}

#[tokio::test]
async fn allows_with_safety_comment() {
    let ctx = make_ctx("// SAFETY: Foo is only accessed behind Arc<Mutex<_>>\nunsafe impl Send for Foo {}");
    let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn detects_sync_too() {
    let ctx = make_ctx("unsafe impl Sync for Bar {}");
    let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}
```

- [ ] **Step 2: Implement**

Scan with context: for each `unsafe impl (Send|Sync)` line, check the preceding 3 lines for `// SAFETY:` or `// Safety:`.

```rust
static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"unsafe\s+impl\s+(Send|Sync)\s+for").unwrap()
});

// In the line loop:
if PATTERN.is_match(trimmed) {
    let has_safety = (i.saturating_sub(3)..i)
        .any(|j| lines.get(j).map(|l| l.contains("SAFETY:") || l.contains("Safety:")).unwrap_or(false));
    if !has_safety {
        // emit finding
    }
}
```

`Severity::High`, `FindingCategory::UnsafeCode`, `cwe_ids: vec![362]`.

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add unsafe-send-sync detector (CWE-362)"
```

---

## Task 8: `duplicated-fn` detector

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/duplicated_fn.rs`

Catches the same function defined in multiple files. CWE-1041.

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn detects_duplicate_function() {
    let mut ctx = AnalysisContext::test_default();
    ctx.language = Language::Rust;
    ctx.source_cache.insert(
        PathBuf::from("src/a.rs"),
        "fn fnv1a_hash(s: &str) -> u64 { 42 }".to_string(),
    );
    ctx.source_cache.insert(
        PathBuf::from("src/b.rs"),
        "fn fnv1a_hash(s: &str) -> u64 { 42 }".to_string(),
    );
    let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn allows_unique_functions() {
    let mut ctx = AnalysisContext::test_default();
    ctx.language = Language::Rust;
    ctx.source_cache.insert(PathBuf::from("src/a.rs"), "fn foo() {}".to_string());
    ctx.source_cache.insert(PathBuf::from("src/b.rs"), "fn bar() {}".to_string());
    let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}

#[tokio::test]
async fn allows_trait_impls_same_name() {
    // Different trait impls with the same method name are fine
    let mut ctx = AnalysisContext::test_default();
    ctx.language = Language::Rust;
    ctx.source_cache.insert(
        PathBuf::from("src/a.rs"),
        "impl Display for Foo { fn fmt(&self) {} }".to_string(),
    );
    ctx.source_cache.insert(
        PathBuf::from("src/b.rs"),
        "impl Display for Bar { fn fmt(&self) {} }".to_string(),
    );
    let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}
```

- [ ] **Step 2: Implement**

Two-pass approach:
1. **Collect:** For each file, extract standalone `fn name(` definitions (not inside `impl` blocks). Store `HashMap<fn_name, Vec<PathBuf>>`.
2. **Report:** Any function name appearing in 2+ files gets a finding.

```rust
// Extract free-standing function names (not in impl blocks)
static FN_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\s*(?:pub\s+)?fn\s+(\w+)\s*\(").unwrap()
});

// Track impl block depth to skip trait method impls
let mut in_impl = 0u32;
for line in source.lines() {
    if line.trim().starts_with("impl ") { in_impl += 1; }
    if in_impl > 0 {
        in_impl += line.matches('{').count() as u32;
        in_impl = in_impl.saturating_sub(line.matches('}').count() as u32);
        continue;
    }
    if let Some(caps) = FN_PATTERN.captures(line) {
        let name = caps[1].to_string();
        fn_locations.entry(name).or_default().push(path.clone());
    }
}
```

`Severity::Low`, `FindingCategory::SecuritySmell`, `cwe_ids: vec![1041]`.

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add duplicated-fn detector (CWE-1041)"
```

---

## Dispatch Plan

All 8 detector tasks are independent (each creates a new file). After Task 0 (scaffold), dispatch all 8 to the security-detect crew in parallel:

```
Task 0: scaffold (sequential — creates stubs)
Then parallel:
  ├── Task 1: discarded-async-result
  ├── Task 2: mixed-bool-ops
  ├── Task 3: partial-cmp-unwrap
  ├── Task 4: substring-security
  ├── Task 5: vecdeque-partial
  ├── Task 6: process-exit-in-lib
  ├── Task 7: unsafe-send-sync
  └── Task 8: duplicated-fn
```

After all complete, run `cargo test -p apex-detect` and `cargo clippy -p apex-detect -- -D warnings`.

Then run `apex run --target . --lang rust` to self-analyze.

---

## Self-Analysis Validation

After all detectors are implemented, run APEX on itself:

```bash
cargo run --bin apex -- run --target . --lang rust --strategy agent --output-format json
```

**Expected findings on the APEX codebase:**
- `discarded-async-result`: `orchestrator.rs:163` (`let _ = strategy.observe(result).await`)
- `mixed-bool-ops`: `classifier.rs:24`, `error_classify.rs:26`
- `substring-security`: `taint_rules.rs:119,124,129`
- `vecdeque-partial`: `cmplog.rs:218`
- `process-exit-in-lib`: `lib.rs:1274`, `doctor.rs:386`, and 3+ more
- `unsafe-send-sync`: `shm.rs:23-24`
- `duplicated-fn`: `fnv1a_hash` in 6+ files

This validates the detectors against real, confirmed bugs.

---

## Summary

| Task | Detector | CWE | Confirmed Bugs in APEX |
|------|----------|-----|----------------------|
| 0 | (scaffold) | — | — |
| 1 | `discarded-async-result` | 252 | orchestrator.rs:163 |
| 2 | `mixed-bool-ops` | 783 | classifier.rs:24, error_classify.rs:26 |
| 3 | `partial-cmp-unwrap` | 754 | taint_triage.rs:51, thompson.rs:46 |
| 4 | `substring-security` | 183 | taint_rules.rs:119,124,129 |
| 5 | `vecdeque-partial` | 682 | cmplog.rs:218 |
| 6 | `process-exit-in-lib` | 705 | lib.rs:1274, doctor.rs:386, +3 more |
| 7 | `unsafe-send-sync` | 362 | shm.rs:23-24 |
| 8 | `duplicated-fn` | 1041 | fnv1a_hash in 6+ files |
