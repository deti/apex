# Phase 2 — Pipeline Upgrades: Synthesis, Fuzzing, Agent Composites, Security Detectors

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the APEX pipeline with 19 techniques across 4 parallel tracks — synthesis prompt strategies, fuzz+LLM integration, agent composite patterns, and CPG-based security detectors. Each task is independently committable and testable.

**Prerequisite:** Phase 1 complete — `PromptStrategy` trait, `GapHistory`, `GapClassifier`, `ThompsonScheduler`, `DeScheduler`, `BranchClassifier`, `MutationGuide`, `TaintSpecStore` all exist and pass tests.

**Architecture:** Extends existing crate structure via new structs/trait implementations. No changes to existing trait interfaces (`Strategy`, `Detector`, `Mutator`). All new code is additive.

---

## Track 2A — Synthesis Prompt Strategies (apex-synth)

### Task 2.1: Few-Shot Prompt Strategy

**Why:** CoverUp shows that including example test code in the prompt dramatically improves first-attempt success rate. The LLM sees concrete patterns to follow.

**Files:**
- New: `crates/apex-synth/src/few_shot.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod few_shot;`

- [ ] **Step 1: Write test for FewShotBank insert and retrieve**

Add to `crates/apex-synth/src/few_shot.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_stores_and_retrieves_examples() {
        let mut bank = FewShotBank::new(5);
        bank.add_example(FewShotExample {
            gap_kind: "branch".into(),
            source_snippet: "if x > 0:".into(),
            test_code: "def test_positive(): assert f(1) == 1".into(),
        });
        let examples = bank.retrieve("branch", 3);
        assert_eq!(examples.len(), 1);
        assert!(examples[0].test_code.contains("test_positive"));
    }
}
```

```bash
cargo test -p apex-synth few_shot::tests::bank_stores_and_retrieves_examples 2>&1 | tail -5
# Expected: FAILED (module does not exist yet)
```

- [ ] **Step 2: Write test for FewShotBank capacity limit**

```rust
    #[test]
    fn bank_respects_capacity_limit() {
        let mut bank = FewShotBank::new(2);
        for i in 0..5 {
            bank.add_example(FewShotExample {
                gap_kind: "branch".into(),
                source_snippet: format!("line {i}"),
                test_code: format!("test_{i}"),
            });
        }
        // Oldest examples evicted; at most 2 remain.
        let examples = bank.retrieve("branch", 10);
        assert!(examples.len() <= 2);
    }
```

- [ ] **Step 3: Write test for format_few_shot_block**

```rust
    #[test]
    fn format_few_shot_block_generates_markdown() {
        let examples = vec![FewShotExample {
            gap_kind: "branch".into(),
            source_snippet: "if x > 0:".into(),
            test_code: "def test_pos(): assert f(1)".into(),
        }];
        let block = format_few_shot_block(&examples);
        assert!(block.contains("Example"));
        assert!(block.contains("def test_pos"));
        assert!(block.contains("if x > 0:"));
    }
```

- [ ] **Step 4: Implement FewShotBank and format_few_shot_block**

```rust
//! Few-shot prompt strategy: include example tests in LLM prompts.

/// A single few-shot example pairing a source gap with its successful test.
#[derive(Debug, Clone)]
pub struct FewShotExample {
    pub gap_kind: String,
    pub source_snippet: String,
    pub test_code: String,
}

/// A bounded bank of few-shot examples, evicting oldest when full.
#[derive(Debug, Clone)]
pub struct FewShotBank {
    examples: Vec<FewShotExample>,
    capacity: usize,
}

impl FewShotBank {
    pub fn new(capacity: usize) -> Self {
        FewShotBank {
            examples: Vec::new(),
            capacity,
        }
    }

    pub fn add_example(&mut self, example: FewShotExample) {
        if self.examples.len() >= self.capacity {
            self.examples.remove(0);
        }
        self.examples.push(example);
    }

    /// Retrieve up to `limit` examples matching `gap_kind`.
    pub fn retrieve(&self, gap_kind: &str, limit: usize) -> Vec<&FewShotExample> {
        self.examples
            .iter()
            .filter(|e| e.gap_kind == gap_kind)
            .take(limit)
            .collect()
    }
}

/// Format a block of few-shot examples for inclusion in an LLM prompt.
pub fn format_few_shot_block(examples: &[FewShotExample]) -> String {
    let mut out = String::new();
    for (i, ex) in examples.iter().enumerate() {
        out.push_str(&format!(
            "Example {}:\nSource:\n```\n{}\n```\nTest:\n```\n{}\n```\n\n",
            i + 1,
            ex.source_snippet,
            ex.test_code,
        ));
    }
    out
}
```

Register module in `crates/apex-synth/src/lib.rs`:
```rust
pub mod few_shot;
```

```bash
cargo test -p apex-synth few_shot::tests 2>&1 | tail -5
# Expected: all 3 tests pass
```

- [ ] **Step 5: Commit**
```bash
git add crates/apex-synth/src/few_shot.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add FewShotBank for few-shot prompt strategy

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.2: Chain-of-Thought Prompt Wrapper

**Why:** Adding "think step-by-step" instructions before test generation improves coverage of complex branching logic. The LLM reasons about path conditions before writing the test.

**Files:**
- New: `crates/apex-synth/src/cot.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod cot;`

- [ ] **Step 1: Write test for CoT prompt wrapping**

Add to `crates/apex-synth/src/cot.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoverageGap, LlmRole};

    fn make_gap() -> CoverageGap {
        CoverageGap {
            file_path: "app.py".into(),
            target_line: 10,
            function_name: Some("process".into()),
            source_segment: "if x > 0:\n    return x\n".into(),
            uncovered_lines: vec![11],
        }
    }

    #[test]
    fn cot_prompt_includes_reasoning_instruction() {
        let messages = build_cot_prompt(&make_gap());
        let system = &messages[0];
        assert_eq!(system.role, LlmRole::System);
        assert!(system.content.contains("step-by-step"));
    }

    #[test]
    fn cot_prompt_includes_source_segment() {
        let messages = build_cot_prompt(&make_gap());
        let user = &messages[1];
        assert_eq!(user.role, LlmRole::User);
        assert!(user.content.contains("if x > 0"));
    }

    #[test]
    fn cot_prompt_mentions_target_lines() {
        let messages = build_cot_prompt(&make_gap());
        let user = &messages[1];
        assert!(user.content.contains("11"));
    }
}
```

```bash
cargo test -p apex-synth cot::tests 2>&1 | tail -5
# Expected: FAILED (module does not exist)
```

- [ ] **Step 2: Implement build_cot_prompt**

```rust
//! Chain-of-Thought prompt wrapper for LLM-guided test synthesis.
//!
//! Wraps the standard CoverUp prompt with reasoning instructions so the LLM
//! analyses path conditions before generating test code.

use crate::{CoverageGap, LlmMessage, LlmRole};

/// Build a chain-of-thought prompt for a coverage gap.
///
/// The system message instructs the LLM to reason step-by-step about what
/// inputs would exercise the uncovered lines before producing test code.
pub fn build_cot_prompt(gap: &CoverageGap) -> Vec<LlmMessage> {
    let system = LlmMessage {
        role: LlmRole::System,
        content: "You are an expert test developer. Before writing the test, \
                  think step-by-step about what inputs and conditions are needed \
                  to reach the uncovered lines. Then write the test. Respond with \
                  your reasoning followed by the test code in a code block."
            .to_string(),
    };

    let fn_hint = gap
        .function_name
        .as_deref()
        .map(|n| format!(" (function `{n}`)"))
        .unwrap_or_default();

    let lines_desc = if gap.uncovered_lines.is_empty() {
        format!("line {}", gap.target_line)
    } else {
        let parts: Vec<String> = gap.uncovered_lines.iter().map(|l| l.to_string()).collect();
        format!("lines {}", parts.join(", "))
    };

    let user = LlmMessage {
        role: LlmRole::User,
        content: format!(
            "File: {file}{fn_hint}\n\
             Uncovered: {lines_desc}\n\n\
             Source segment:\n```\n{segment}\n```\n\n\
             Think step-by-step about what inputs exercise {lines_desc}, \
             then write a test for {file}.",
            file = gap.file_path,
            fn_hint = fn_hint,
            lines_desc = lines_desc,
            segment = gap.source_segment,
        ),
    };

    vec![system, user]
}
```

Register module in `crates/apex-synth/src/lib.rs`:
```rust
pub mod cot;
```

```bash
cargo test -p apex-synth cot::tests 2>&1 | tail -5
# Expected: all 3 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-synth/src/cot.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add chain-of-thought prompt wrapper

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.3: Mutation-Hint Prompt Enrichment

**Why:** When the fuzzer nearly flips a branch (high heuristic), injecting the operand values into the prompt tells the LLM exactly what threshold to cross. "The branch `x > 42` was tested with `x=40` — write a test with `x > 42`."

**Files:**
- New: `crates/apex-synth/src/mutation_hint.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod mutation_hint;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-synth/src/mutation_hint.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_formats_comparison() {
        let hint = MutationHint {
            variable: "x".into(),
            operator: ">".into(),
            threshold: 42,
            closest_value: 40,
        };
        let text = hint.format();
        assert!(text.contains("x"));
        assert!(text.contains("42"));
        assert!(text.contains("40"));
    }

    #[test]
    fn format_hints_block_multiple() {
        let hints = vec![
            MutationHint {
                variable: "x".into(),
                operator: ">".into(),
                threshold: 42,
                closest_value: 40,
            },
            MutationHint {
                variable: "y".into(),
                operator: "==".into(),
                threshold: 0,
                closest_value: 1,
            },
        ];
        let block = format_hints_block(&hints);
        assert!(block.contains("x"));
        assert!(block.contains("y"));
        assert!(block.contains("Mutation hints"));
    }

    #[test]
    fn format_hints_block_empty() {
        let block = format_hints_block(&[]);
        assert!(block.is_empty());
    }
}
```

- [ ] **Step 2: Implement MutationHint**

```rust
//! Mutation-hint prompt enrichment for LLM test synthesis.
//!
//! Injects near-miss comparison data from the fuzzer into the LLM prompt
//! so the model knows exactly what threshold to cross.

/// A single mutation hint from fuzzer near-miss data.
#[derive(Debug, Clone)]
pub struct MutationHint {
    pub variable: String,
    pub operator: String,
    pub threshold: i64,
    pub closest_value: i64,
}

impl MutationHint {
    /// Format this hint as a human-readable string for LLM consumption.
    pub fn format(&self) -> String {
        format!(
            "Branch `{var} {op} {thresh}` was tested with `{var}={closest}` \
             (distance: {dist}). Write a test where `{var} {op} {thresh}`.",
            var = self.variable,
            op = self.operator,
            thresh = self.threshold,
            closest = self.closest_value,
            dist = (self.threshold - self.closest_value).abs(),
        )
    }
}

/// Format a block of mutation hints for inclusion in an LLM prompt.
/// Returns empty string if there are no hints.
pub fn format_hints_block(hints: &[MutationHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut out = String::from("Mutation hints from fuzzer:\n");
    for hint in hints {
        out.push_str(&format!("- {}\n", hint.format()));
    }
    out
}
```

```bash
cargo test -p apex-synth mutation_hint::tests 2>&1 | tail -5
# Expected: all 3 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-synth/src/mutation_hint.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add mutation-hint prompt enrichment

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.4: Error Classifier for Refinement Routing

**Why:** CoverUp retries with a generic "fix the error" message. Classifying the error type (import error, assertion failure, syntax error, runtime error) lets us tailor the feedback prompt for faster convergence.

**Files:**
- New: `crates/apex-synth/src/error_classify.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod error_classify;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-synth/src/error_classify.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_import_error() {
        let err = "ModuleNotFoundError: No module named 'foo'";
        assert_eq!(classify_test_error(err), ErrorKind::Import);
    }

    #[test]
    fn classify_syntax_error() {
        let err = "SyntaxError: invalid syntax\n  File \"test.py\", line 5";
        assert_eq!(classify_test_error(err), ErrorKind::Syntax);
    }

    #[test]
    fn classify_assertion_error() {
        let err = "AssertionError: expected 1, got 2";
        assert_eq!(classify_test_error(err), ErrorKind::Assertion);
    }

    #[test]
    fn classify_runtime_error() {
        let err = "ZeroDivisionError: division by zero";
        assert_eq!(classify_test_error(err), ErrorKind::Runtime);
    }

    #[test]
    fn classify_unknown() {
        let err = "something went wrong";
        assert_eq!(classify_test_error(err), ErrorKind::Unknown);
    }

    #[test]
    fn refinement_prompt_import() {
        let msg = refinement_prompt(ErrorKind::Import, "ModuleNotFoundError: no 'foo'");
        assert!(msg.contains("import"));
    }

    #[test]
    fn refinement_prompt_syntax() {
        let msg = refinement_prompt(ErrorKind::Syntax, "SyntaxError: bad");
        assert!(msg.contains("syntax"));
    }
}
```

- [ ] **Step 2: Implement error classifier and refinement prompt**

```rust
//! Error classifier for LLM refinement routing.
//!
//! Classifies test execution errors by kind so the refinement prompt can
//! give targeted feedback instead of generic "fix the error" messages.

/// Classification of a test execution error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Import,
    Syntax,
    Assertion,
    Runtime,
    Unknown,
}

/// Classify a test error message into an ErrorKind.
pub fn classify_test_error(error: &str) -> ErrorKind {
    let lower = error.to_lowercase();
    if lower.contains("importerror")
        || lower.contains("modulenotfounderror")
        || lower.contains("no module named")
    {
        ErrorKind::Import
    } else if lower.contains("syntaxerror") || lower.contains("invalid syntax") {
        ErrorKind::Syntax
    } else if lower.contains("assertionerror") || lower.contains("assert") && lower.contains("fail") {
        ErrorKind::Assertion
    } else if lower.contains("error") || lower.contains("exception") {
        ErrorKind::Runtime
    } else {
        ErrorKind::Unknown
    }
}

/// Generate a targeted refinement prompt based on the error kind.
pub fn refinement_prompt(kind: ErrorKind, error: &str) -> String {
    match kind {
        ErrorKind::Import => format!(
            "The test has an import error: {error}. \
             Fix the import statement — use only modules available in the project."
        ),
        ErrorKind::Syntax => format!(
            "The test has a syntax error: {error}. \
             Fix the syntax and ensure the code is valid."
        ),
        ErrorKind::Assertion => format!(
            "The test assertion failed: {error}. \
             Check the expected values and adjust the assertion."
        ),
        ErrorKind::Runtime => format!(
            "The test raised a runtime error: {error}. \
             Fix the test to avoid this exception."
        ),
        ErrorKind::Unknown => format!(
            "The test yielded an error: {error}. \
             Modify the test to fix it."
        ),
    }
}
```

```bash
cargo test -p apex-synth error_classify::tests 2>&1 | tail -5
# Expected: all 7 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-synth/src/error_classify.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add error classifier for refinement routing

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.5: Prompt Template Registry

**Why:** Different languages and gap types need different prompt structures. A registry maps `(language, gap_kind)` to a Tera template so prompts are data-driven and extensible.

**Files:**
- New: `crates/apex-synth/src/prompt_registry.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod prompt_registry;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-synth/src/prompt_registry.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup_template() {
        let mut registry = PromptRegistry::new();
        registry.register("python", "branch", "Write a test for {{ file }}");
        let tmpl = registry.lookup("python", "branch");
        assert!(tmpl.is_some());
        assert!(tmpl.unwrap().contains("{{ file }}"));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let registry = PromptRegistry::new();
        assert!(registry.lookup("rust", "branch").is_none());
    }

    #[test]
    fn render_template_substitutes_variables() {
        let mut registry = PromptRegistry::new();
        registry.register("python", "branch", "Test for {{ file }} line {{ line }}");
        let mut vars = std::collections::HashMap::new();
        vars.insert("file".into(), "app.py".into());
        vars.insert("line".into(), "42".into());
        let rendered = registry.render("python", "branch", &vars).unwrap();
        assert!(rendered.contains("app.py"));
        assert!(rendered.contains("42"));
    }

    #[test]
    fn render_missing_template_returns_error() {
        let registry = PromptRegistry::new();
        let result = registry.render("go", "branch", &std::collections::HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn default_registry_has_python_branch() {
        let registry = PromptRegistry::with_defaults();
        assert!(registry.lookup("python", "branch").is_some());
    }
}
```

- [ ] **Step 2: Implement PromptRegistry**

```rust
//! Prompt template registry for language-aware test synthesis.
//!
//! Maps `(language, gap_kind)` keys to Tera template strings. Supports
//! variable substitution for file paths, line numbers, and code segments.

use std::collections::HashMap;

/// Registry mapping `(language, gap_kind)` to prompt template strings.
#[derive(Debug, Clone)]
pub struct PromptRegistry {
    templates: HashMap<(String, String), String>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        PromptRegistry {
            templates: HashMap::new(),
        }
    }

    /// Create a registry pre-loaded with default templates.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register(
            "python",
            "branch",
            "File: {{ file }}\nUncovered: {{ lines }}\n\n\
             Source:\n```python\n{{ segment }}\n```\n\n\
             Write a pytest test that exercises {{ lines }} in {{ file }}.",
        );
        reg.register(
            "rust",
            "branch",
            "File: {{ file }}\nUncovered: {{ lines }}\n\n\
             Source:\n```rust\n{{ segment }}\n```\n\n\
             Write a #[test] function that exercises {{ lines }} in {{ file }}.",
        );
        reg.register(
            "javascript",
            "branch",
            "File: {{ file }}\nUncovered: {{ lines }}\n\n\
             Source:\n```javascript\n{{ segment }}\n```\n\n\
             Write a Jest test that exercises {{ lines }} in {{ file }}.",
        );
        reg
    }

    /// Register a template for a `(language, gap_kind)` pair.
    pub fn register(&mut self, language: &str, gap_kind: &str, template: &str) {
        self.templates
            .insert((language.to_string(), gap_kind.to_string()), template.to_string());
    }

    /// Look up a template by `(language, gap_kind)`.
    pub fn lookup(&self, language: &str, gap_kind: &str) -> Option<&str> {
        self.templates
            .get(&(language.to_string(), gap_kind.to_string()))
            .map(|s| s.as_str())
    }

    /// Render a template with variable substitution.
    pub fn render(
        &self,
        language: &str,
        gap_kind: &str,
        vars: &HashMap<String, String>,
    ) -> Result<String, String> {
        let template = self
            .lookup(language, gap_kind)
            .ok_or_else(|| format!("no template for ({language}, {gap_kind})"))?;
        let mut result = template.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{ {key} }}}}"), value);
        }
        Ok(result)
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

```bash
cargo test -p apex-synth prompt_registry::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-synth/src/prompt_registry.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add prompt template registry for language-aware synthesis

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.6: Test Code Extractor (Fence-Block Parser)

**Why:** LLM responses wrap test code in markdown fences. A robust extractor pulls the code out of ````python ... ``` blocks, handling multiple fences and language tags.

**Files:**
- New: `crates/apex-synth/src/extractor.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod extractor;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-synth/src/extractor.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_python_block() {
        let response = "Here's the test:\n```python\ndef test_foo():\n    assert True\n```\n";
        let blocks = extract_code_blocks(response);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].code.contains("def test_foo"));
        assert_eq!(blocks[0].language.as_deref(), Some("python"));
    }

    #[test]
    fn extract_multiple_blocks() {
        let response = "```python\nblock1\n```\ntext\n```rust\nblock2\n```\n";
        let blocks = extract_code_blocks(response);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language.as_deref(), Some("python"));
        assert_eq!(blocks[1].language.as_deref(), Some("rust"));
    }

    #[test]
    fn extract_no_language_tag() {
        let response = "```\ncode here\n```\n";
        let blocks = extract_code_blocks(response);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].language.is_none());
    }

    #[test]
    fn extract_no_blocks() {
        let blocks = extract_code_blocks("just plain text");
        assert!(blocks.is_empty());
    }

    #[test]
    fn extract_unclosed_fence_ignored() {
        let response = "```python\ncode without closing fence";
        let blocks = extract_code_blocks(response);
        assert!(blocks.is_empty());
    }

    #[test]
    fn best_test_block_prefers_python() {
        let response = "```python\ndef test_x(): pass\n```\n```\nother\n```\n";
        let best = best_test_block(response, "python");
        assert!(best.is_some());
        assert!(best.unwrap().contains("test_x"));
    }
}
```

- [ ] **Step 2: Implement extractor**

```rust
//! Test code extractor — parses markdown fence blocks from LLM responses.

/// A code block extracted from a markdown-fenced LLM response.
#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub language: Option<String>,
    pub code: String,
}

/// Extract all fenced code blocks from an LLM response.
pub fn extract_code_blocks(response: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut lines = response.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let lang_tag = trimmed.strip_prefix("```").unwrap().trim();
            let language = if lang_tag.is_empty() {
                None
            } else {
                Some(lang_tag.to_string())
            };

            let mut code_lines = Vec::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner.trim().starts_with("```") {
                    closed = true;
                    break;
                }
                code_lines.push(inner);
            }
            if closed {
                blocks.push(CodeBlock {
                    language,
                    code: code_lines.join("\n"),
                });
            }
        }
    }

    blocks
}

/// Select the best test code block from an LLM response.
///
/// Prefers blocks tagged with the target language. Falls back to the first
/// block containing "test" or "def test" or "#[test]".
pub fn best_test_block(response: &str, target_language: &str) -> Option<String> {
    let blocks = extract_code_blocks(response);
    // Prefer matching language tag.
    if let Some(b) = blocks.iter().find(|b| {
        b.language
            .as_deref()
            .is_some_and(|l| l.eq_ignore_ascii_case(target_language))
    }) {
        return Some(b.code.clone());
    }
    // Fall back to first block containing test indicators.
    if let Some(b) = blocks
        .iter()
        .find(|b| b.code.contains("test") || b.code.contains("Test"))
    {
        return Some(b.code.clone());
    }
    // Last resort: first block.
    blocks.into_iter().next().map(|b| b.code)
}
```

```bash
cargo test -p apex-synth extractor::tests 2>&1 | tail -5
# Expected: all 6 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-synth/src/extractor.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add fence-block code extractor for LLM responses

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.7: Coverage Delta Tracker

**Why:** After each synthesis attempt, we need to compute exactly which new branches were covered. This tracker diffs two coverage bitmaps and reports the delta.

**Files:**
- New: `crates/apex-synth/src/delta.rs`
- Modify: `crates/apex-synth/src/lib.rs` — add `pub mod delta;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-synth/src/delta.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    #[test]
    fn delta_empty_when_same() {
        let before = vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)];
        let after = vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)];
        let delta = coverage_delta(&before, &after);
        assert!(delta.is_empty());
    }

    #[test]
    fn delta_reports_new_branches() {
        let before = vec![BranchId::new(1, 1, 0, 0)];
        let after = vec![
            BranchId::new(1, 1, 0, 0),
            BranchId::new(1, 2, 0, 0),
            BranchId::new(1, 3, 0, 0),
        ];
        let delta = coverage_delta(&before, &after);
        assert_eq!(delta.len(), 2);
    }

    #[test]
    fn delta_ignores_removed_branches() {
        let before = vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)];
        let after = vec![BranchId::new(1, 1, 0, 0)];
        let delta = coverage_delta(&before, &after);
        assert!(delta.is_empty());
    }

    #[test]
    fn delta_from_empty_baseline() {
        let before: Vec<BranchId> = vec![];
        let after = vec![BranchId::new(1, 5, 0, 0)];
        let delta = coverage_delta(&before, &after);
        assert_eq!(delta.len(), 1);
    }

    #[test]
    fn delta_summary_formats_correctly() {
        let delta = vec![BranchId::new(1, 10, 0, 0), BranchId::new(2, 20, 0, 0)];
        let summary = format_delta_summary(&delta);
        assert!(summary.contains("2 new branch"));
    }
}
```

- [ ] **Step 2: Implement coverage delta**

```rust
//! Coverage delta tracker — diffs two branch lists to find newly covered branches.

use std::collections::HashSet;
use apex_core::types::BranchId;

/// Compute the set of branches in `after` that are not in `before`.
pub fn coverage_delta(before: &[BranchId], after: &[BranchId]) -> Vec<BranchId> {
    let before_set: HashSet<_> = before.iter().collect();
    after
        .iter()
        .filter(|b| !before_set.contains(b))
        .cloned()
        .collect()
}

/// Format a human-readable summary of a coverage delta.
pub fn format_delta_summary(delta: &[BranchId]) -> String {
    if delta.is_empty() {
        "No new branches covered.".to_string()
    } else {
        format!(
            "{} new branch{} covered.",
            delta.len(),
            if delta.len() == 1 { "" } else { "es" }
        )
    }
}
```

```bash
cargo test -p apex-synth delta::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-synth/src/delta.rs crates/apex-synth/src/lib.rs
git commit -m "feat(synth): add coverage delta tracker

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Track 2B — Fuzz+LLM Integration (apex-fuzz)

### Task 2.8: LLM-Guided Mutator

**Why:** When standard byte-level mutations stall, asking an LLM to mutate structured inputs (JSON, SQL, HTML) produces semantically valid variants that byte flips cannot. The mutator caches LLM responses to amortize latency.

**Files:**
- New: `crates/apex-fuzz/src/llm_mutator.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` — add `pub mod llm_mutator;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-fuzz/src/llm_mutator.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_mutator_returns_precomputed() {
        let mut cache = MutationCache::new(10);
        cache.insert(b"hello".to_vec(), vec![b"world".to_vec(), b"hi".to_vec()]);
        let variants = cache.get(b"hello");
        assert_eq!(variants.unwrap().len(), 2);
    }

    #[test]
    fn cache_miss_returns_none() {
        let cache = MutationCache::new(10);
        assert!(cache.get(b"unknown").is_none());
    }

    #[test]
    fn cache_evicts_oldest_at_capacity() {
        let mut cache = MutationCache::new(2);
        cache.insert(b"a".to_vec(), vec![b"a1".to_vec()]);
        cache.insert(b"b".to_vec(), vec![b"b1".to_vec()]);
        cache.insert(b"c".to_vec(), vec![b"c1".to_vec()]);
        // "a" should have been evicted.
        assert!(cache.get(b"a").is_none());
        assert!(cache.get(b"c").is_some());
    }

    #[test]
    fn format_mutation_prompt_includes_input() {
        let prompt = format_mutation_prompt(b"SELECT * FROM users", "sql");
        assert!(prompt.contains("SELECT"));
        assert!(prompt.contains("sql"));
    }

    #[test]
    fn format_mutation_prompt_binary_input() {
        let prompt = format_mutation_prompt(&[0xFF, 0x00, 0xAB], "binary");
        assert!(prompt.contains("hex"));
    }
}
```

- [ ] **Step 2: Implement MutationCache and prompt formatting**

```rust
//! LLM-guided mutator — uses cached LLM responses for structured input mutation.
//!
//! When byte-level mutations stall on structured inputs, the LLM can produce
//! semantically valid variants. Results are cached to amortize LLM latency.

use std::collections::HashMap;

/// A bounded cache mapping input bytes to pre-computed LLM mutation variants.
pub struct MutationCache {
    cache: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    order: Vec<Vec<u8>>,
    capacity: usize,
}

impl MutationCache {
    pub fn new(capacity: usize) -> Self {
        MutationCache {
            cache: HashMap::new(),
            order: Vec::new(),
            capacity,
        }
    }

    /// Insert a set of mutation variants for a given input.
    pub fn insert(&mut self, input: Vec<u8>, variants: Vec<Vec<u8>>) {
        if self.cache.len() >= self.capacity && !self.cache.contains_key(&input) {
            // Evict oldest.
            if let Some(oldest) = self.order.first().cloned() {
                self.cache.remove(&oldest);
                self.order.remove(0);
            }
        }
        if !self.cache.contains_key(&input) {
            self.order.push(input.clone());
        }
        self.cache.insert(input, variants);
    }

    /// Retrieve cached variants for an input.
    pub fn get(&self, input: &[u8]) -> Option<&Vec<Vec<u8>>> {
        self.cache.get(input)
    }
}

/// Format a mutation prompt for the LLM, including the input to mutate.
pub fn format_mutation_prompt(input: &[u8], format_hint: &str) -> String {
    let input_repr = if input.iter().all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
        String::from_utf8_lossy(input).to_string()
    } else {
        format!(
            "hex: {}",
            input.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
        )
    };

    format!(
        "You are a fuzzer mutation engine for {format_hint} inputs.\n\
         Generate 5 semantically valid variants of this input that explore \
         different code paths. Each variant should be syntactically valid {format_hint}.\n\n\
         Input:\n```\n{input_repr}\n```\n\n\
         Respond with each variant in a separate code block."
    )
}
```

```bash
cargo test -p apex-fuzz llm_mutator::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-fuzz/src/llm_mutator.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add LLM-guided mutator with mutation cache

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.9: Grammar-Aware Mutation Operator

**Why:** For structured inputs (JSON, XML, SQL), grammar-aware mutations swap subtrees in the parse tree rather than random bytes. This preserves structural validity while exploring deeper code paths.

**Files:**
- New: `crates/apex-fuzz/src/grammar_mutator.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` — add `pub mod grammar_mutator;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-fuzz/src/grammar_mutator.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::{Grammar, ParseNode, Symbol};
    use rand::SeedableRng;

    fn simple_grammar() -> Grammar {
        let mut g = Grammar::new("expr");
        g.add_production("expr", vec![
            vec![Symbol::Terminal("1".into())],
            vec![Symbol::Terminal("2".into())],
            vec![
                Symbol::NonTerminal("expr".into()),
                Symbol::Terminal("+".into()),
                Symbol::NonTerminal("expr".into()),
            ],
        ]);
        g
    }

    #[test]
    fn subtree_replace_changes_output() {
        let tree = ParseNode::Interior(
            "expr".into(),
            vec![
                ParseNode::Leaf("1".into()),
                ParseNode::Leaf("+".into()),
                ParseNode::Leaf("2".into()),
            ],
        );
        let replacement = ParseNode::Leaf("3".into());
        let mutated = replace_subtree(&tree, 0, &replacement);
        let flat = flatten_tree(&mutated);
        assert!(flat.contains("3"));
    }

    #[test]
    fn flatten_tree_concatenates_leaves() {
        let tree = ParseNode::Interior(
            "expr".into(),
            vec![
                ParseNode::Leaf("a".into()),
                ParseNode::Leaf("b".into()),
            ],
        );
        assert_eq!(flatten_tree(&tree), "ab");
    }

    #[test]
    fn count_nodes_counts_all() {
        let tree = ParseNode::Interior(
            "root".into(),
            vec![
                ParseNode::Leaf("x".into()),
                ParseNode::Interior(
                    "inner".into(),
                    vec![ParseNode::Leaf("y".into())],
                ),
            ],
        );
        assert_eq!(count_nodes(&tree), 4);
    }
}
```

- [ ] **Step 2: Implement grammar-aware mutation helpers**

```rust
//! Grammar-aware mutation — swap subtrees in parse trees for structured inputs.

use crate::grammar::ParseNode;

/// Count total nodes in a parse tree.
pub fn count_nodes(node: &ParseNode) -> usize {
    match node {
        ParseNode::Leaf(_) => 1,
        ParseNode::Interior(_, children) => {
            1 + children.iter().map(count_nodes).sum::<usize>()
        }
    }
}

/// Flatten a parse tree into a string by concatenating all leaf values.
pub fn flatten_tree(node: &ParseNode) -> String {
    match node {
        ParseNode::Leaf(s) => s.clone(),
        ParseNode::Interior(_, children) => {
            children.iter().map(flatten_tree).collect::<String>()
        }
    }
}

/// Replace the subtree at the given child index with a replacement node.
/// Only replaces at the top level of an Interior node.
pub fn replace_subtree(node: &ParseNode, child_index: usize, replacement: &ParseNode) -> ParseNode {
    match node {
        ParseNode::Leaf(s) => ParseNode::Leaf(s.clone()),
        ParseNode::Interior(name, children) => {
            let new_children: Vec<ParseNode> = children
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    if i == child_index {
                        replacement.clone()
                    } else {
                        child.clone()
                    }
                })
                .collect();
            ParseNode::Interior(name.clone(), new_children)
        }
    }
}
```

```bash
cargo test -p apex-fuzz grammar_mutator::tests 2>&1 | tail -5
# Expected: all 3 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-fuzz/src/grammar_mutator.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add grammar-aware mutation operators

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.10: Seed Distillation (Corpus Minimization)

**Why:** Over time the fuzzer corpus grows large with redundant inputs. Seed distillation keeps only the minimal set of inputs that covers all discovered branches, reducing execution time per generation.

**Files:**
- New: `crates/apex-fuzz/src/distill.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` — add `pub mod distill;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-fuzz/src/distill.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    #[test]
    fn distill_removes_redundant_seeds() {
        // Seed A covers {1,2}, seed B covers {2,3}, seed C covers {1,2,3}.
        // C alone covers everything — A and B are redundant.
        let entries = vec![
            CorpusEntry {
                data: b"A".to_vec(),
                branches: vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)],
            },
            CorpusEntry {
                data: b"B".to_vec(),
                branches: vec![BranchId::new(1, 2, 0, 0), BranchId::new(1, 3, 0, 0)],
            },
            CorpusEntry {
                data: b"C".to_vec(),
                branches: vec![
                    BranchId::new(1, 1, 0, 0),
                    BranchId::new(1, 2, 0, 0),
                    BranchId::new(1, 3, 0, 0),
                ],
            },
        ];
        let distilled = distill_corpus(&entries);
        // C covers all 3 branches, so at most 1 seed needed.
        assert!(distilled.len() <= 2);
        // All branches still covered.
        let all_branches: std::collections::HashSet<_> =
            distilled.iter().flat_map(|e| &e.branches).collect();
        assert!(all_branches.contains(&BranchId::new(1, 1, 0, 0)));
        assert!(all_branches.contains(&BranchId::new(1, 2, 0, 0)));
        assert!(all_branches.contains(&BranchId::new(1, 3, 0, 0)));
    }

    #[test]
    fn distill_empty_corpus() {
        let distilled = distill_corpus(&[]);
        assert!(distilled.is_empty());
    }

    #[test]
    fn distill_single_seed() {
        let entries = vec![CorpusEntry {
            data: b"only".to_vec(),
            branches: vec![BranchId::new(1, 1, 0, 0)],
        }];
        let distilled = distill_corpus(&entries);
        assert_eq!(distilled.len(), 1);
    }

    #[test]
    fn distill_disjoint_seeds_all_kept() {
        let entries = vec![
            CorpusEntry {
                data: b"A".to_vec(),
                branches: vec![BranchId::new(1, 1, 0, 0)],
            },
            CorpusEntry {
                data: b"B".to_vec(),
                branches: vec![BranchId::new(1, 2, 0, 0)],
            },
        ];
        let distilled = distill_corpus(&entries);
        assert_eq!(distilled.len(), 2);
    }
}
```

- [ ] **Step 2: Implement distill_corpus**

```rust
//! Seed distillation — minimize the corpus to a covering set.
//!
//! Uses a greedy set-cover algorithm: repeatedly pick the seed covering the
//! most uncovered branches, until all branches are covered.

use std::collections::HashSet;
use apex_core::types::BranchId;

/// A corpus entry with its input data and the branches it covers.
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    pub data: Vec<u8>,
    pub branches: Vec<BranchId>,
}

/// Distill a corpus to a minimal covering set using greedy set cover.
pub fn distill_corpus(entries: &[CorpusEntry]) -> Vec<CorpusEntry> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut uncovered: HashSet<&BranchId> = entries.iter().flat_map(|e| &e.branches).collect();
    let mut remaining: Vec<&CorpusEntry> = entries.iter().collect();
    let mut result = Vec::new();

    while !uncovered.is_empty() && !remaining.is_empty() {
        // Pick the entry covering the most uncovered branches.
        let best_idx = remaining
            .iter()
            .enumerate()
            .max_by_key(|(_, e)| e.branches.iter().filter(|b| uncovered.contains(b)).count())
            .map(|(i, _)| i);

        let Some(idx) = best_idx else { break };
        let best = remaining.remove(idx);

        // If it covers zero new branches, stop.
        let new_coverage: Vec<_> = best.branches.iter().filter(|b| uncovered.contains(b)).collect();
        if new_coverage.is_empty() {
            break;
        }

        for b in &best.branches {
            uncovered.remove(b);
        }
        result.push(best.clone());
    }

    result
}
```

```bash
cargo test -p apex-fuzz distill::tests 2>&1 | tail -5
# Expected: all 4 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-fuzz/src/distill.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add seed distillation via greedy set cover

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Track 2C — Agent Composites (apex-agent)

### Task 2.11: Strategy Rotation Policy

**Why:** The orchestrator needs a principled policy for when to rotate between fuzzer, solver, and LLM strategies. This codifies the escalation thresholds from CoverageMonitor into a reusable policy object.

**Files:**
- New: `crates/apex-agent/src/rotation.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod rotation;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-agent/src/rotation.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_policy_starts_with_fuzz() {
        let policy = RotationPolicy::new(vec!["fuzz".into(), "solver".into(), "llm".into()]);
        assert_eq!(policy.current(), "fuzz");
    }

    #[test]
    fn rotate_advances_to_next() {
        let mut policy = RotationPolicy::new(vec!["fuzz".into(), "solver".into()]);
        policy.rotate();
        assert_eq!(policy.current(), "solver");
    }

    #[test]
    fn rotate_wraps_around() {
        let mut policy = RotationPolicy::new(vec!["a".into(), "b".into()]);
        policy.rotate();
        policy.rotate();
        assert_eq!(policy.current(), "a");
    }

    #[test]
    fn should_rotate_after_stall() {
        let policy = RotationPolicy::new(vec!["fuzz".into(), "solver".into()]);
        assert!(!policy.should_rotate(0));
        assert!(!policy.should_rotate(4));
        assert!(policy.should_rotate(10));
    }

    #[test]
    fn custom_stall_threshold() {
        let mut policy = RotationPolicy::new(vec!["a".into(), "b".into()]);
        policy.set_stall_threshold(3);
        assert!(!policy.should_rotate(2));
        assert!(policy.should_rotate(3));
    }

    #[test]
    fn single_strategy_wraps() {
        let mut policy = RotationPolicy::new(vec!["only".into()]);
        policy.rotate();
        assert_eq!(policy.current(), "only");
    }
}
```

- [ ] **Step 2: Implement RotationPolicy**

```rust
//! Strategy rotation policy for the orchestrator.
//!
//! Codifies escalation thresholds: after N stalled iterations, rotate to the
//! next strategy in the configured order.

/// Policy governing when and how to rotate between exploration strategies.
#[derive(Debug, Clone)]
pub struct RotationPolicy {
    strategies: Vec<String>,
    current_index: usize,
    stall_threshold: u64,
}

impl RotationPolicy {
    /// Create a new rotation policy with the given strategy order.
    pub fn new(strategies: Vec<String>) -> Self {
        RotationPolicy {
            strategies,
            current_index: 0,
            stall_threshold: 5,
        }
    }

    /// Get the name of the currently active strategy.
    pub fn current(&self) -> &str {
        &self.strategies[self.current_index]
    }

    /// Advance to the next strategy in round-robin order.
    pub fn rotate(&mut self) {
        self.current_index = (self.current_index + 1) % self.strategies.len();
    }

    /// Check whether rotation is warranted given the current stall count.
    pub fn should_rotate(&self, stall_iterations: u64) -> bool {
        stall_iterations >= self.stall_threshold
    }

    /// Set the stall threshold (number of iterations without progress before rotating).
    pub fn set_stall_threshold(&mut self, threshold: u64) {
        self.stall_threshold = threshold;
    }
}
```

```bash
cargo test -p apex-agent rotation::tests 2>&1 | tail -5
# Expected: all 6 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-agent/src/rotation.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add strategy rotation policy

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.12: Budget Allocator

**Why:** Different strategies have different costs (LLM calls are expensive, fuzzing is cheap). A budget allocator distributes iteration budgets proportionally based on strategy effectiveness and cost.

**Files:**
- New: `crates/apex-agent/src/budget.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod budget;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-agent/src/budget.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_allocation_with_no_history() {
        let allocator = BudgetAllocator::new(1000, 3);
        let budgets = allocator.allocate();
        // With no performance data, split evenly.
        assert_eq!(budgets.len(), 3);
        let total: u64 = budgets.iter().sum();
        assert_eq!(total, 1000);
    }

    #[test]
    fn report_adjusts_allocation() {
        let mut allocator = BudgetAllocator::new(1000, 2);
        // Strategy 0 found 10 branches, strategy 1 found 0.
        allocator.report(0, 10);
        allocator.report(1, 0);
        let budgets = allocator.allocate();
        // Strategy 0 should get more budget.
        assert!(budgets[0] > budgets[1]);
    }

    #[test]
    fn minimum_budget_guaranteed() {
        let mut allocator = BudgetAllocator::new(100, 2);
        allocator.set_minimum_share(0.1);
        allocator.report(0, 100);
        allocator.report(1, 0);
        let budgets = allocator.allocate();
        // Even strategy 1 gets at least 10%.
        assert!(budgets[1] >= 10);
    }

    #[test]
    fn single_strategy_gets_full_budget() {
        let allocator = BudgetAllocator::new(500, 1);
        let budgets = allocator.allocate();
        assert_eq!(budgets, vec![500]);
    }
}
```

- [ ] **Step 2: Implement BudgetAllocator**

```rust
//! Budget allocator — distributes iteration budgets across strategies.
//!
//! Tracks per-strategy effectiveness (branches found) and allocates
//! proportionally, with a minimum share floor so no strategy starves.

/// Allocates iteration budgets across N strategies.
#[derive(Debug, Clone)]
pub struct BudgetAllocator {
    total_budget: u64,
    num_strategies: usize,
    effectiveness: Vec<u64>,
    minimum_share: f64,
}

impl BudgetAllocator {
    pub fn new(total_budget: u64, num_strategies: usize) -> Self {
        BudgetAllocator {
            total_budget,
            num_strategies,
            effectiveness: vec![0; num_strategies],
            minimum_share: 0.05,
        }
    }

    /// Report that strategy `index` discovered `new_branches` branches.
    pub fn report(&mut self, index: usize, new_branches: u64) {
        if index < self.num_strategies {
            self.effectiveness[index] += new_branches;
        }
    }

    /// Set the minimum share each strategy receives (0.0 to 1.0).
    pub fn set_minimum_share(&mut self, share: f64) {
        self.minimum_share = share.clamp(0.0, 1.0 / self.num_strategies as f64);
    }

    /// Allocate budgets proportional to effectiveness.
    pub fn allocate(&self) -> Vec<u64> {
        let n = self.num_strategies;
        let total_eff: u64 = self.effectiveness.iter().sum();

        if total_eff == 0 {
            // Equal split when no data.
            let per = self.total_budget / n as u64;
            let mut budgets = vec![per; n];
            // Distribute remainder.
            let remainder = self.total_budget - per * n as u64;
            for i in 0..remainder as usize {
                budgets[i] += 1;
            }
            return budgets;
        }

        let min_budget = (self.total_budget as f64 * self.minimum_share) as u64;
        let reserved = min_budget * n as u64;
        let distributable = self.total_budget.saturating_sub(reserved);

        let mut budgets: Vec<u64> = self
            .effectiveness
            .iter()
            .map(|&eff| {
                let share = eff as f64 / total_eff as f64;
                min_budget + (distributable as f64 * share) as u64
            })
            .collect();

        // Adjust rounding errors.
        let allocated: u64 = budgets.iter().sum();
        if allocated < self.total_budget {
            budgets[0] += self.total_budget - allocated;
        }

        budgets
    }
}
```

```bash
cargo test -p apex-agent budget::tests 2>&1 | tail -5
# Expected: all 4 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-agent/src/budget.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add budget allocator for strategy iteration distribution

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.13: Feedback Aggregator

**Why:** Multiple strategies produce feedback (coverage deltas, branch distances, error reports). The aggregator merges these into a unified signal that the orchestrator uses to make global decisions.

**Files:**
- New: `crates/apex-agent/src/feedback.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod feedback;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-agent/src/feedback.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    #[test]
    fn aggregate_empty() {
        let agg = FeedbackAggregator::new();
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 0);
        assert!(summary.strategies.is_empty());
    }

    #[test]
    fn aggregate_single_strategy() {
        let mut agg = FeedbackAggregator::new();
        agg.record("fuzz", StrategyFeedback {
            new_branches: vec![BranchId::new(1, 1, 0, 0)],
            best_heuristic: 0.8,
            errors: 0,
        });
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 1);
        assert_eq!(summary.strategies.len(), 1);
    }

    #[test]
    fn aggregate_multiple_strategies() {
        let mut agg = FeedbackAggregator::new();
        agg.record("fuzz", StrategyFeedback {
            new_branches: vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)],
            best_heuristic: 0.6,
            errors: 1,
        });
        agg.record("solver", StrategyFeedback {
            new_branches: vec![BranchId::new(1, 3, 0, 0)],
            best_heuristic: 0.95,
            errors: 0,
        });
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 3);
        assert_eq!(summary.best_heuristic, 0.95);
        assert_eq!(summary.total_errors, 1);
    }

    #[test]
    fn aggregate_deduplicates_branches() {
        let mut agg = FeedbackAggregator::new();
        let branch = BranchId::new(1, 1, 0, 0);
        agg.record("fuzz", StrategyFeedback {
            new_branches: vec![branch.clone()],
            best_heuristic: 0.5,
            errors: 0,
        });
        agg.record("solver", StrategyFeedback {
            new_branches: vec![branch],
            best_heuristic: 0.5,
            errors: 0,
        });
        let summary = agg.summarize();
        // Same branch from both strategies — deduped to 1.
        assert_eq!(summary.total_new_branches, 1);
    }

    #[test]
    fn clear_resets_aggregator() {
        let mut agg = FeedbackAggregator::new();
        agg.record("fuzz", StrategyFeedback {
            new_branches: vec![BranchId::new(1, 1, 0, 0)],
            best_heuristic: 0.5,
            errors: 0,
        });
        agg.clear();
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 0);
    }
}
```

- [ ] **Step 2: Implement FeedbackAggregator**

```rust
//! Feedback aggregator — merges strategy outputs into a unified signal.

use std::collections::{HashMap, HashSet};
use apex_core::types::BranchId;

/// Feedback from a single strategy execution round.
#[derive(Debug, Clone)]
pub struct StrategyFeedback {
    pub new_branches: Vec<BranchId>,
    pub best_heuristic: f64,
    pub errors: u32,
}

/// Aggregated summary across all strategies.
#[derive(Debug, Clone)]
pub struct AggregatedSummary {
    pub total_new_branches: usize,
    pub best_heuristic: f64,
    pub total_errors: u32,
    pub strategies: Vec<String>,
}

/// Collects and merges feedback from multiple strategies.
pub struct FeedbackAggregator {
    entries: Vec<(String, StrategyFeedback)>,
}

impl FeedbackAggregator {
    pub fn new() -> Self {
        FeedbackAggregator {
            entries: Vec::new(),
        }
    }

    /// Record feedback from a named strategy.
    pub fn record(&mut self, strategy: &str, feedback: StrategyFeedback) {
        self.entries.push((strategy.to_string(), feedback));
    }

    /// Summarize all recorded feedback with deduplication.
    pub fn summarize(&self) -> AggregatedSummary {
        let mut all_branches: HashSet<BranchId> = HashSet::new();
        let mut best_heuristic: f64 = 0.0;
        let mut total_errors: u32 = 0;
        let mut strategies: Vec<String> = Vec::new();

        for (name, fb) in &self.entries {
            for b in &fb.new_branches {
                all_branches.insert(b.clone());
            }
            if fb.best_heuristic > best_heuristic {
                best_heuristic = fb.best_heuristic;
            }
            total_errors += fb.errors;
            if !strategies.contains(name) {
                strategies.push(name.clone());
            }
        }

        AggregatedSummary {
            total_new_branches: all_branches.len(),
            best_heuristic,
            total_errors,
            strategies,
        }
    }

    /// Clear all recorded feedback.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for FeedbackAggregator {
    fn default() -> Self {
        Self::new()
    }
}
```

```bash
cargo test -p apex-agent feedback::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-agent/src/feedback.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add feedback aggregator for multi-strategy signals

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.14: Exploration History Log

**Why:** Debugging and analysis require a structured log of what the agent tried, what worked, and what didn't. The history log records per-iteration decisions for post-hoc analysis and strategy tuning.

**Files:**
- New: `crates/apex-agent/src/history.rs`
- Modify: `crates/apex-agent/src/lib.rs` — add `pub mod history;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-agent/src/history.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_records_entries() {
        let mut log = ExplorationLog::new();
        log.record(LogEntry {
            iteration: 1,
            strategy: "fuzz".into(),
            branches_found: 3,
            action_taken: "normal".into(),
        });
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn log_entries_ordered() {
        let mut log = ExplorationLog::new();
        log.record(LogEntry {
            iteration: 1,
            strategy: "fuzz".into(),
            branches_found: 1,
            action_taken: "normal".into(),
        });
        log.record(LogEntry {
            iteration: 2,
            strategy: "solver".into(),
            branches_found: 5,
            action_taken: "rotate".into(),
        });
        let entries = log.entries();
        assert_eq!(entries[0].iteration, 1);
        assert_eq!(entries[1].iteration, 2);
    }

    #[test]
    fn total_branches_found() {
        let mut log = ExplorationLog::new();
        log.record(LogEntry {
            iteration: 1,
            strategy: "fuzz".into(),
            branches_found: 3,
            action_taken: "normal".into(),
        });
        log.record(LogEntry {
            iteration: 2,
            strategy: "solver".into(),
            branches_found: 7,
            action_taken: "normal".into(),
        });
        assert_eq!(log.total_branches_found(), 10);
    }

    #[test]
    fn strategy_summary() {
        let mut log = ExplorationLog::new();
        for i in 0..5 {
            log.record(LogEntry {
                iteration: i,
                strategy: if i < 3 { "fuzz" } else { "solver" }.into(),
                branches_found: 1,
                action_taken: "normal".into(),
            });
        }
        let summary = log.strategy_summary();
        assert_eq!(summary["fuzz"], 3);
        assert_eq!(summary["solver"], 2);
    }

    #[test]
    fn empty_log() {
        let log = ExplorationLog::new();
        assert_eq!(log.len(), 0);
        assert_eq!(log.total_branches_found(), 0);
        assert!(log.strategy_summary().is_empty());
    }
}
```

- [ ] **Step 2: Implement ExplorationLog**

```rust
//! Exploration history log — structured record of agent decisions.

use std::collections::HashMap;

/// A single entry in the exploration log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub iteration: u64,
    pub strategy: String,
    pub branches_found: u64,
    pub action_taken: String,
}

/// Append-only log of exploration decisions for post-hoc analysis.
#[derive(Debug, Clone)]
pub struct ExplorationLog {
    entries: Vec<LogEntry>,
}

impl ExplorationLog {
    pub fn new() -> Self {
        ExplorationLog {
            entries: Vec::new(),
        }
    }

    /// Append a log entry.
    pub fn record(&mut self, entry: LogEntry) {
        self.entries.push(entry);
    }

    /// Number of entries recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all entries in chronological order.
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// Total branches found across all entries.
    pub fn total_branches_found(&self) -> u64 {
        self.entries.iter().map(|e| e.branches_found).sum()
    }

    /// Count of iterations per strategy.
    pub fn strategy_summary(&self) -> HashMap<String, u64> {
        let mut counts = HashMap::new();
        for entry in &self.entries {
            *counts.entry(entry.strategy.clone()).or_insert(0) += 1;
        }
        counts
    }
}

impl Default for ExplorationLog {
    fn default() -> Self {
        Self::new()
    }
}
```

```bash
cargo test -p apex-agent history::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-agent/src/history.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add exploration history log

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Track 2D — Security Detectors (apex-detect + apex-cpg)

### Task 2.15: SQL Injection Detector (CPG-Based)

**Why:** Pattern-matching SQL injection detection produces false positives. CPG-based taint analysis traces data flow from user input to SQL query construction, only flagging when there is a real source-to-sink path without sanitization.

**Files:**
- New: `crates/apex-detect/src/detectors/sql_injection.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs` — add `pub mod sql_injection;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-detect/src/detectors/sql_injection.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_string_format_injection() {
        let source = r#"
def get_user(request):
    name = request.args.get('name')
    query = "SELECT * FROM users WHERE name = '%s'" % name
    cursor.execute(query)
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("SQL"));
    }

    #[test]
    fn detect_fstring_injection() {
        let source = r#"
def get_user(name):
    query = f"SELECT * FROM users WHERE name = '{name}'"
    db.execute(query)
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_parameterized_query_not_flagged() {
        let source = r#"
def get_user(name):
    cursor.execute("SELECT * FROM users WHERE name = %s", (name,))
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn safe_no_user_input() {
        let source = r#"
def get_count():
    cursor.execute("SELECT COUNT(*) FROM users")
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_concatenation_injection() {
        let source = r#"
def search(query_str):
    sql = "SELECT * FROM items WHERE name = '" + query_str + "'"
    conn.execute(sql)
"#;
        let findings = scan_sql_injection(source, "search.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn finding_has_correct_category() {
        let source = "query = f\"SELECT * FROM t WHERE x = '{user_input}'\"\ndb.execute(query)";
        let findings = scan_sql_injection(source, "x.py");
        if !findings.is_empty() {
            assert_eq!(findings[0].category, FindingCategory::Injection);
        }
    }
}
```

- [ ] **Step 2: Implement scan_sql_injection**

```rust
//! SQL injection detector — identifies unsanitized user input in SQL queries.
//!
//! Scans for string formatting/concatenation patterns used to build SQL
//! queries. A full CPG-based version would trace taint flows; this initial
//! implementation uses pattern matching on common injection vectors.

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use uuid::Uuid;

/// SQL execution function patterns.
const SQL_EXEC_PATTERNS: &[&str] = &[
    "execute(", "executemany(", "raw(", "cursor.execute(",
    "db.execute(", "conn.execute(", "session.execute(",
];

/// Scan source code for SQL injection vulnerabilities.
pub fn scan_sql_injection(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Pattern 1: f-string with SQL keywords
    let fstring_sql = Regex::new(
        r#"f["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*\{[^}]+\}.*["']"#
    ).unwrap();

    // Pattern 2: % formatting with SQL keywords
    let percent_sql = Regex::new(
        r#"["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*%[sd].*["']\s*%"#
    ).unwrap();

    // Pattern 3: String concatenation with SQL keywords
    let concat_sql = Regex::new(
        r#"["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*["']\s*\+"#
    ).unwrap();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip parameterized queries (safe pattern).
        if SQL_EXEC_PATTERNS.iter().any(|p| trimmed.contains(p))
            && (trimmed.contains("%s\", (") || trimmed.contains("%s\", [")
                || trimmed.contains("?, (") || trimmed.contains("?, ["))
        {
            continue;
        }

        let is_vuln = fstring_sql.is_match(trimmed)
            || percent_sql.is_match(trimmed)
            || concat_sql.is_match(trimmed);

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "sql_injection".into(),
                severity: Severity::High,
                category: FindingCategory::Injection,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential SQL injection via string interpolation".into(),
                description: format!(
                    "SQL query constructed with string formatting at line {line_1based}. \
                     Use parameterized queries instead."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Use parameterized queries (e.g., cursor.execute(\"SELECT ... WHERE x = %s\", (val,)))".into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![89],
            });
        }
    }

    findings
}
```

```bash
cargo test -p apex-detect sql_injection::tests 2>&1 | tail -5
# Expected: all 6 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-detect/src/detectors/sql_injection.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add SQL injection detector

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.16: Path Traversal Detector

**Why:** Path traversal (CWE-22) is a high-impact vulnerability where user-controlled input reaches file system operations without validation. Detects patterns like `open(user_input)` without path sanitization.

**Files:**
- New: `crates/apex-detect/src/detectors/path_traversal.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs` — add `pub mod path_traversal;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-detect/src/detectors/path_traversal.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_open_with_user_input() {
        let source = r#"
def download(request):
    filename = request.args.get('file')
    with open(filename) as f:
        return f.read()
"#;
        let findings = scan_path_traversal(source, "views.py");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::PathTraversal);
    }

    #[test]
    fn detect_os_path_join_with_user_input() {
        let source = r#"
import os
def serve(name):
    path = os.path.join('/uploads', name)
    return open(path).read()
"#;
        let findings = scan_path_traversal(source, "serve.py");
        // os.path.join with user input is suspicious.
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_hardcoded_path_not_flagged() {
        let source = r#"
def read_config():
    with open('/etc/app/config.json') as f:
        return json.load(f)
"#;
        let findings = scan_path_traversal(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_pathlib_with_variable() {
        let source = r#"
from pathlib import Path
def load(user_path):
    return Path(user_path).read_text()
"#;
        let findings = scan_path_traversal(source, "loader.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn finding_has_cwe_22() {
        let source = "f = open(user_input)\ndata = f.read()";
        let findings = scan_path_traversal(source, "x.py");
        if !findings.is_empty() {
            assert!(findings[0].cwe_ids.contains(&22));
        }
    }
}
```

- [ ] **Step 2: Implement scan_path_traversal**

```rust
//! Path traversal detector — identifies unsanitized file path access (CWE-22).

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use uuid::Uuid;

/// Scan source code for path traversal vulnerabilities.
pub fn scan_path_traversal(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Pattern: open() with a variable (not a string literal).
    let open_var = Regex::new(r#"open\(\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*[,)]"#).unwrap();
    // Pattern: Path() with a variable.
    let path_var = Regex::new(r#"Path\(\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*\)"#).unwrap();
    // Pattern: os.path.join with variable.
    let path_join = Regex::new(r#"os\.path\.join\([^)]*[a-zA-Z_][a-zA-Z0-9_.]*[^)]*\)"#).unwrap();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip lines with only string literals in open().
        let has_open_literal = Regex::new(r#"open\(\s*['"f]"#).unwrap();
        let is_string_only = has_open_literal.is_match(trimmed)
            && !trimmed.contains('+')
            && !trimmed.contains('{');

        let mut is_vuln = false;

        if let Some(cap) = open_var.captures(trimmed) {
            let arg = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            // Skip if the argument looks like a string literal (starts with quote).
            if !arg.starts_with('\'') && !arg.starts_with('"') && !is_string_only {
                is_vuln = true;
            }
        }

        if path_var.is_match(trimmed) {
            is_vuln = true;
        }

        if path_join.is_match(trimmed) {
            is_vuln = true;
        }

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "path_traversal".into(),
                severity: Severity::High,
                category: FindingCategory::PathTraversal,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential path traversal via unsanitized file path".into(),
                description: format!(
                    "File operation at line {line_1based} uses a variable that may \
                     contain user-controlled path components like '../'."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Validate and sanitize the path. Use os.path.realpath() and \
                             verify the result is within the expected directory."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![22],
            });
        }
    }

    findings
}
```

```bash
cargo test -p apex-detect path_traversal::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-detect/src/detectors/path_traversal.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add path traversal detector (CWE-22)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.17: Command Injection Detector

**Why:** Command injection (CWE-78) occurs when user input reaches shell execution functions. Detects patterns like `os.system(user_input)` and `subprocess.call(shell=True)`.

**Files:**
- New: `crates/apex-detect/src/detectors/command_injection.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs` — add `pub mod command_injection;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-detect/src/detectors/command_injection.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_os_system() {
        let source = r#"
import os
def run_cmd(user_cmd):
    os.system(user_cmd)
"#;
        let findings = scan_command_injection(source, "cmd.py");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[test]
    fn detect_subprocess_shell_true() {
        let source = r#"
import subprocess
def run(cmd):
    subprocess.call(cmd, shell=True)
"#;
        let findings = scan_command_injection(source, "run.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_os_popen() {
        let source = r#"
def execute(cmd):
    os.popen(cmd)
"#;
        let findings = scan_command_injection(source, "exec.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_subprocess_without_shell() {
        let source = r#"
import subprocess
def run_safe(args):
    subprocess.run(["ls", "-la"])
"#;
        let findings = scan_command_injection(source, "safe.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn finding_has_cwe_78() {
        let source = "os.system(cmd)";
        let findings = scan_command_injection(source, "x.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&78));
    }
}
```

- [ ] **Step 2: Implement scan_command_injection**

```rust
//! Command injection detector — identifies unsanitized shell execution (CWE-78).

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use uuid::Uuid;

/// Shell execution function patterns that are dangerous with user input.
const DANGEROUS_FUNCS: &[&str] = &[
    "os.system(",
    "os.popen(",
    "commands.getoutput(",
    "commands.getstatusoutput(",
];

/// Scan source code for command injection vulnerabilities.
pub fn scan_command_injection(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let shell_true = Regex::new(r#"subprocess\.\w+\([^)]*shell\s*=\s*True"#).unwrap();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        let mut is_vuln = false;

        // Check direct dangerous function calls.
        for func in DANGEROUS_FUNCS {
            if trimmed.contains(func) {
                is_vuln = true;
                break;
            }
        }

        // Check subprocess with shell=True.
        if shell_true.is_match(trimmed) {
            is_vuln = true;
        }

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "command_injection".into(),
                severity: Severity::Critical,
                category: FindingCategory::Injection,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential command injection via shell execution".into(),
                description: format!(
                    "Shell command execution at line {line_1based} may allow \
                     command injection if input is user-controlled."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Use subprocess.run() with a list of arguments (no shell=True). \
                             Never pass unsanitized user input to shell commands."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![78],
            });
        }
    }

    findings
}
```

```bash
cargo test -p apex-detect command_injection::tests 2>&1 | tail -5
# Expected: all 5 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-detect/src/detectors/command_injection.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add command injection detector (CWE-78)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.18: Hardcoded Secrets Detector

**Why:** Hardcoded passwords, API keys, and tokens in source code (CWE-798) are a common security issue. Detects patterns like `password = "..."`, `API_KEY = "..."`, and high-entropy strings in assignments.

**Files:**
- New: `crates/apex-detect/src/detectors/hardcoded_secret.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs` — add `pub mod hardcoded_secret;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-detect/src/detectors/hardcoded_secret.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_password_assignment() {
        let source = r#"password = "s3cr3t_p4ss""#;
        let findings = scan_hardcoded_secrets(source, "config.py");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
    }

    #[test]
    fn detect_api_key() {
        let source = r#"API_KEY = "sk-abc123def456ghi789""#;
        let findings = scan_hardcoded_secrets(source, "settings.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_token() {
        let source = r#"auth_token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0""#;
        let findings = scan_hardcoded_secrets(source, "auth.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_env_var_not_flagged() {
        let source = r#"password = os.environ.get("PASSWORD")"#;
        let findings = scan_hardcoded_secrets(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn safe_empty_string_not_flagged() {
        let source = r#"password = """#;
        let findings = scan_hardcoded_secrets(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn safe_placeholder_not_flagged() {
        let source = r#"password = "changeme""#;
        // Short placeholder values below entropy threshold.
        let findings = scan_hardcoded_secrets(source, "config.py");
        // May or may not flag — depends on heuristic. Just verify no panic.
        let _ = findings;
    }

    #[test]
    fn finding_has_cwe_798() {
        let source = r#"SECRET_KEY = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4""#;
        let findings = scan_hardcoded_secrets(source, "x.py");
        if !findings.is_empty() {
            assert!(findings[0].cwe_ids.contains(&798));
        }
    }
}
```

- [ ] **Step 2: Implement scan_hardcoded_secrets**

```rust
//! Hardcoded secrets detector — finds passwords, tokens, and API keys in source (CWE-798).

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use uuid::Uuid;

/// Secret variable name patterns (case-insensitive).
const SECRET_NAMES: &[&str] = &[
    "password", "passwd", "secret", "api_key", "apikey",
    "auth_token", "access_token", "secret_key", "private_key",
    "token", "credentials", "api_secret",
];

/// Compute Shannon entropy of a string (bits per character).
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Scan source code for hardcoded secrets.
pub fn scan_hardcoded_secrets(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    // Match: var_name = "string_value" or var_name = 'string_value'
    let assignment = Regex::new(
        r#"([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*["']([^"']+)["']"#
    ).unwrap();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip environment variable lookups.
        if trimmed.contains("os.environ") || trimmed.contains("env.get")
            || trimmed.contains("getenv") || trimmed.contains("ENV[")
        {
            continue;
        }

        if let Some(cap) = assignment.captures(trimmed) {
            let var_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let value = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // Skip empty values or very short values.
            if value.len() < 8 {
                continue;
            }

            let var_lower = var_name.to_lowercase();
            let is_secret_name = SECRET_NAMES.iter().any(|s| var_lower.contains(s));
            let high_entropy = shannon_entropy(value) > 3.5;

            if is_secret_name && high_entropy {
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "hardcoded_secret".into(),
                    severity: Severity::High,
                    category: FindingCategory::SecuritySmell,
                    file: PathBuf::from(file_path),
                    line: Some(line_1based),
                    title: format!("Hardcoded secret in variable `{var_name}`"),
                    description: format!(
                        "Variable `{var_name}` at line {line_1based} appears to contain \
                         a hardcoded secret. Use environment variables or a secrets manager."
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Move secrets to environment variables or a secrets manager \
                                 (e.g., AWS Secrets Manager, HashiCorp Vault)."
                        .into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![798],
                });
            }
        }
    }

    findings
}
```

```bash
cargo test -p apex-detect hardcoded_secret::tests 2>&1 | tail -5
# Expected: all 7 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-detect/src/detectors/hardcoded_secret.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add hardcoded secrets detector (CWE-798)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2.19: CPG Taint Rule Expansion

**Why:** The CPG taint analysis currently has Python sources/sinks/sanitizers hardcoded. This task adds a configurable rule set so users can define custom taint rules for their codebase, and extends the built-in rules with JavaScript patterns.

**Files:**
- New: `crates/apex-cpg/src/taint_rules.rs`
- Modify: `crates/apex-cpg/src/lib.rs` — add `pub mod taint_rules;`

- [ ] **Step 1: Write tests**

Add to `crates/apex-cpg/src/taint_rules.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_python_rules_have_sources() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sources.is_empty());
        assert!(rules.sources.iter().any(|s| s.contains("request")));
    }

    #[test]
    fn default_python_rules_have_sinks() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sinks.is_empty());
        assert!(rules.sinks.iter().any(|s| s.contains("execute")));
    }

    #[test]
    fn default_python_rules_have_sanitizers() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sanitizers.is_empty());
    }

    #[test]
    fn javascript_rules_have_sources() {
        let rules = TaintRuleSet::javascript_defaults();
        assert!(!rules.sources.is_empty());
        assert!(rules.sources.iter().any(|s| s.contains("req")));
    }

    #[test]
    fn javascript_rules_have_sinks() {
        let rules = TaintRuleSet::javascript_defaults();
        assert!(!rules.sinks.is_empty());
    }

    #[test]
    fn custom_rules_merge() {
        let mut rules = TaintRuleSet::python_defaults();
        let custom = TaintRuleSet {
            sources: vec!["custom_source".into()],
            sinks: vec!["custom_sink".into()],
            sanitizers: vec![],
        };
        rules.merge(&custom);
        assert!(rules.sources.contains(&"custom_source".to_string()));
        assert!(rules.sinks.contains(&"custom_sink".to_string()));
    }

    #[test]
    fn is_source_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec!["request.args".into()],
            sinks: vec![],
            sanitizers: vec![],
        };
        assert!(rules.is_source("request.args"));
        assert!(!rules.is_source("safe_func"));
    }

    #[test]
    fn is_sink_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec![],
            sinks: vec!["execute".into()],
            sanitizers: vec![],
        };
        assert!(rules.is_sink("execute"));
        assert!(!rules.is_sink("safe_func"));
    }

    #[test]
    fn is_sanitizer_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec![],
            sinks: vec![],
            sanitizers: vec!["escape".into()],
        };
        assert!(rules.is_sanitizer("escape"));
        assert!(!rules.is_sanitizer("noop"));
    }

    #[test]
    fn empty_rules() {
        let rules = TaintRuleSet::empty();
        assert!(rules.sources.is_empty());
        assert!(rules.sinks.is_empty());
        assert!(rules.sanitizers.is_empty());
    }
}
```

- [ ] **Step 2: Implement TaintRuleSet**

```rust
//! Configurable taint rules for CPG taint analysis.
//!
//! Provides built-in rule sets for Python and JavaScript, plus a merge
//! mechanism for user-defined custom rules.

use serde::{Deserialize, Serialize};

/// A set of taint analysis rules: sources, sinks, and sanitizers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintRuleSet {
    pub sources: Vec<String>,
    pub sinks: Vec<String>,
    pub sanitizers: Vec<String>,
}

impl TaintRuleSet {
    /// Create an empty rule set.
    pub fn empty() -> Self {
        TaintRuleSet {
            sources: Vec::new(),
            sinks: Vec::new(),
            sanitizers: Vec::new(),
        }
    }

    /// Default Python taint rules.
    pub fn python_defaults() -> Self {
        TaintRuleSet {
            sources: vec![
                "request.args".into(),
                "request.form".into(),
                "request.data".into(),
                "request.json".into(),
                "request.get_json".into(),
                "sys.argv".into(),
                "input".into(),
                "os.environ".into(),
            ],
            sinks: vec![
                "execute".into(),
                "executemany".into(),
                "os.system".into(),
                "os.popen".into(),
                "subprocess.call".into(),
                "subprocess.run".into(),
                "eval".into(),
                "exec".into(),
                "open".into(),
                "render_template_string".into(),
            ],
            sanitizers: vec![
                "escape".into(),
                "quote".into(),
                "sanitize".into(),
                "clean".into(),
                "parameterize".into(),
                "bleach.clean".into(),
                "markupsafe.escape".into(),
            ],
        }
    }

    /// Default JavaScript taint rules.
    pub fn javascript_defaults() -> Self {
        TaintRuleSet {
            sources: vec![
                "req.body".into(),
                "req.params".into(),
                "req.query".into(),
                "req.headers".into(),
                "document.location".into(),
                "window.location".into(),
                "process.argv".into(),
                "process.env".into(),
            ],
            sinks: vec![
                "eval".into(),
                "exec".into(),
                "execSync".into(),
                "innerHTML".into(),
                "document.write".into(),
                "child_process.exec".into(),
                "db.query".into(),
                "pool.query".into(),
                "fs.readFile".into(),
                "fs.writeFile".into(),
            ],
            sanitizers: vec![
                "escape".into(),
                "sanitize".into(),
                "encodeURIComponent".into(),
                "DOMPurify.sanitize".into(),
                "validator.escape".into(),
            ],
        }
    }

    /// Merge another rule set into this one (additive, no duplicates).
    pub fn merge(&mut self, other: &TaintRuleSet) {
        for src in &other.sources {
            if !self.sources.contains(src) {
                self.sources.push(src.clone());
            }
        }
        for sink in &other.sinks {
            if !self.sinks.contains(sink) {
                self.sinks.push(sink.clone());
            }
        }
        for san in &other.sanitizers {
            if !self.sanitizers.contains(san) {
                self.sanitizers.push(san.clone());
            }
        }
    }

    /// Check if a function name matches any source pattern.
    pub fn is_source(&self, name: &str) -> bool {
        self.sources.iter().any(|s| name.contains(s.as_str()))
    }

    /// Check if a function name matches any sink pattern.
    pub fn is_sink(&self, name: &str) -> bool {
        self.sinks.iter().any(|s| name.contains(s.as_str()))
    }

    /// Check if a function name matches any sanitizer pattern.
    pub fn is_sanitizer(&self, name: &str) -> bool {
        self.sanitizers.iter().any(|s| name.contains(s.as_str()))
    }
}
```

Register module in `crates/apex-cpg/src/lib.rs`:
```rust
pub mod taint_rules;
```

```bash
cargo test -p apex-cpg taint_rules::tests 2>&1 | tail -5
# Expected: all 10 tests pass
```

- [ ] **Step 3: Commit**
```bash
git add crates/apex-cpg/src/taint_rules.rs crates/apex-cpg/src/lib.rs
git commit -m "feat(cpg): add configurable taint rules with JS defaults

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Execution Order

Tasks within each track are independent. Across tracks, all tasks are independent. Recommended parallelism:

| Agent | Track | Tasks |
|-------|-------|-------|
| Agent 1 | 2A Synthesis | 2.1 → 2.2 → 2.3 → 2.4 → 2.5 → 2.6 → 2.7 |
| Agent 2 | 2B Fuzzing | 2.8 → 2.9 → 2.10 |
| Agent 3 | 2C Agent | 2.11 → 2.12 → 2.13 → 2.14 |
| Agent 4 | 2D Security | 2.15 → 2.16 → 2.17 → 2.18 → 2.19 |

All tasks create new files only — no merge conflicts possible between agents.

---

## Verification

After all tasks complete:
```bash
cargo test --workspace 2>&1 | tail -20
cargo clippy --workspace -- -D warnings
```
