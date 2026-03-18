<!-- status: ACTIVE -->

# Language Parity Plan 5: Concolic Execution Expansion (Revised)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add concolic execution support for Rust, Java, Go, C++, C, C#, Swift, Kotlin, Ruby, and WASM — currently only Python and JavaScript have it.

**Architecture (revised after code review):** The shared infrastructure already exists:
- `ConditionTree` / `Expr` / `CompareOp` in `condition_tree.rs` — the language-agnostic condition IR
- `js_conditions.rs` — reference pattern: `parse_js_condition(&str) -> ConditionTree` via recursive descent
- `taint.rs`, `search.rs`, `selective.rs` — all language-agnostic, no changes needed

What's needed per language is a **condition parser function** `parse_<lang>_condition(&str) -> ConditionTree` that follows the `js_conditions.rs` pattern. Then a single shared `StaticConcolicStrategy` uses any parser to extract conditions from source files and generate boundary seeds.

**Key insight:** Do NOT create a parallel `ExtractedCondition` type. Parse directly into the existing `ConditionTree` IR.

**Tech Stack:** regex-lite for condition extraction, existing `ConditionTree` IR, `Strategy` trait from apex-core

**Depends on:** Plan 1 (done)

---

## Chunk 1: Shared Infrastructure

### Task 1: Boundary Seed Generator from ConditionTree

**Files:**
- Create: `crates/apex-concolic/src/boundary.rs`
- Modify: `crates/apex-concolic/src/lib.rs`

The boundary seed generator takes a `ConditionTree` and produces concrete test values near decision boundaries.

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition_tree::*;

    #[test]
    fn boundary_for_int_gt() {
        let tree = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        let seeds = boundary_values(&tree);
        assert!(seeds.contains(&"10".to_string()));
        assert!(seeds.contains(&"11".to_string()));
        assert!(seeds.contains(&"9".to_string()));
    }

    #[test]
    fn boundary_for_null_check() {
        let tree = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("x".into())),
            is_null: true,
        };
        let seeds = boundary_values(&tree);
        assert!(seeds.iter().any(|s| s.contains("null") || s.contains("None")));
    }

    #[test]
    fn boundary_for_string_eq() {
        let tree = ConditionTree::Compare {
            left: Box::new(Expr::Variable("s".into())),
            op: CompareOp::Eq,
            right: Box::new(Expr::StringLiteral("hello".into())),
        };
        let seeds = boundary_values(&tree);
        assert!(seeds.iter().any(|s| s.contains("hello")));
        assert!(seeds.iter().any(|s| s.contains("\"\"") || s.contains("''")));
    }

    #[test]
    fn boundary_for_length_check() {
        let tree = ConditionTree::LengthCheck {
            expr: Box::new(Expr::Variable("arr".into())),
            op: CompareOp::Gt,
            value: Box::new(Expr::IntLiteral(0)),
        };
        let seeds = boundary_values(&tree);
        assert!(seeds.contains(&"0".to_string()));
        assert!(seeds.contains(&"1".to_string()));
    }

    #[test]
    fn boundary_for_unknown_returns_empty() {
        let tree = ConditionTree::Unknown("complex()".into());
        let seeds = boundary_values(&tree);
        assert!(seeds.is_empty());
    }
}
```

- [ ] **Step 2: Implement `boundary_values(tree: &ConditionTree) -> Vec<String>`**

Extract literal values from the tree, generate boundary mutations:
- `Compare { right: IntLiteral(n), op: Gt, .. }` → `[n-1, n, n+1, 0, -1]`
- `Compare { right: StringLiteral(s), .. }` → `[s, "", s+"x"]`
- `NullCheck { is_null: true, .. }` → `["null", "0", "\"\""]`
- `LengthCheck { value: IntLiteral(n), op: Gt, .. }` → `[n-1, n, n+1, 0]`
- `And/Or/Not` → recurse into children, merge seeds
- `Unknown` → empty

- [ ] **Step 3: Add to lib.rs, run tests, commit**

---

### Task 2: Static Concolic Strategy

**Files:**
- Create: `crates/apex-concolic/src/static_strategy.rs`
- Modify: `crates/apex-concolic/src/lib.rs`

A language-agnostic concolic strategy that works for any language with a condition parser.

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_conditions_from_source() {
        let source = "if x > 10 {\n    do_thing();\n}";
        // Use a simple parser that understands "if <cond> {"
        let conditions = extract_if_conditions(source);
        assert!(!conditions.is_empty());
    }

    #[test]
    fn strategy_name() {
        let strategy = StaticConcolicStrategy::new(
            "rust",
            |s| parse_generic_conditions(s),
        );
        assert_eq!(strategy.name(), "concolic-rust");
    }
}
```

- [ ] **Step 2: Implement StaticConcolicStrategy**

```rust
pub struct StaticConcolicStrategy {
    language_name: String,
    parser: Box<dyn Fn(&str) -> Vec<(u32, ConditionTree)> + Send + Sync>,
}

impl StaticConcolicStrategy {
    pub fn new(
        language: &str,
        parser: impl Fn(&str) -> Vec<(u32, ConditionTree)> + Send + Sync + 'static,
    ) -> Self {
        Self {
            language_name: language.to_string(),
            parser: Box::new(parser),
        }
    }
}

#[async_trait]
impl Strategy for StaticConcolicStrategy {
    fn name(&self) -> &str { &format!("concolic-{}", self.language_name) }

    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        // 1. Read source files from ctx
        // 2. For each file, call (self.parser)(source) to get (line, ConditionTree) pairs
        // 3. Filter to uncovered branches (via ctx.oracle)
        // 4. Generate boundary_values for each condition
        // 5. Return InputSeeds with boundary values as test parameters
    }

    async fn observe(&self, _result: &ExecutionResult) -> Result<()> { Ok(()) }
}
```

- [ ] **Step 3: Implement `extract_if_conditions(source: &str) -> Vec<(u32, ConditionTree)>`**

A generic helper that extracts `if <condition>` patterns from C-family source code. Works for Rust, Go, Java, C#, Swift, C, C++. Each language parser can call this as a baseline and add language-specific patterns on top.

Uses regex: `if\s*\(?(.+?)\)?\s*\{` to find if-conditions, then delegates to `parse_generic_condition(&str) -> ConditionTree` which handles numeric comparisons, null checks, and boolean operators.

- [ ] **Step 4: Add to lib.rs, run tests, commit**

---

## Chunk 2: Language Condition Parsers (Tier A — C-family syntax)

Each parser is a module with `pub fn parse_<lang>_conditions(source: &str) -> Vec<(u32, ConditionTree)>` that extracts branch conditions from source code. They all share the generic `extract_if_conditions()` base and add language-specific patterns.

### Task 3: Rust Condition Parser

**Files:** Create `crates/apex-concolic/src/rust_conditions.rs`

Language-specific patterns beyond generic:
- `match` arms with guards: `<pat> if <cond> => ...`
- `if let Some(x) = ...` → `NullCheck { is_null: false }`
- `if let Err(e) = ...` → `TypeCheck { type_name: "Err" }`
- `.is_some()` / `.is_none()` / `.is_ok()` / `.is_err()` → null/type checks

Tests: 6+ tests covering each Rust-specific pattern.

- [ ] **Step 1: Write tests for Rust patterns**
- [ ] **Step 2: Implement parser**
- [ ] **Step 3: Add to lib.rs, run tests, commit**

---

### Task 4: Go Condition Parser

**Files:** Create `crates/apex-concolic/src/go_conditions.rs`

Language-specific patterns:
- `if err != nil` (most common Go pattern)
- `switch x { case 1: ... }` → multiple `Compare { op: Eq }` nodes
- `if _, ok := m[key]; ok` → `NullCheck` on map lookup
- `if len(s) > 0` → `LengthCheck`

Tests: 6+ tests.

- [ ] **Step 1-3: Same flow**

---

### Task 5: Java/Kotlin Condition Parser

**Files:** Create `crates/apex-concolic/src/java_conditions.rs`

Language-specific patterns:
- `instanceof` → `TypeCheck`
- `switch (x) { case ENUM: ... }` → `Compare { op: Eq }`
- `.equals("value")` → `Compare { op: Eq, right: StringLiteral }`
- `== null` → `NullCheck`
- Kotlin: `is Type` → `TypeCheck`, `when (x) { ... }` → switch equivalent

Tests: 8+ tests (Java + Kotlin).

- [ ] **Step 1-3: Same flow**

---

### Task 6: C# Condition Parser

**Files:** Create `crates/apex-concolic/src/csharp_conditions.rs`

Language-specific patterns:
- `is Type t` (pattern matching) → `TypeCheck`
- `?.` (null-conditional) → `NullCheck`
- `?? default` (null-coalescing) → `NullCheck`
- `switch` with `when` clauses

Tests: 6+ tests.

- [ ] **Step 1-3: Same flow**

---

### Task 7: Swift Condition Parser

**Files:** Create `crates/apex-concolic/src/swift_conditions.rs`

Language-specific patterns:
- `if let x = optional` → `NullCheck { is_null: false }`
- `guard let x = optional else` → `NullCheck`
- `case .enumCase(let val)` → `TypeCheck`
- `where val > 5` in switch → `Compare`

Tests: 6+ tests.

- [ ] **Step 1-3: Same flow**

---

### Task 8: C/C++ Condition Parser

**Files:** Create `crates/apex-concolic/src/c_conditions.rs`

Language-specific patterns:
- `if (ptr != NULL)` / `if (ptr)` → `NullCheck`
- `if (flags & MASK)` → `Compare { op: NotEq, right: IntLiteral(0) }`
- Skip `#ifdef` / `#if defined()` (preprocessor, not runtime)
- C++: `dynamic_cast<T*>(x)` → `TypeCheck`

Conservative: skip macro-heavy code. Tests: 6+ tests.

- [ ] **Step 1-3: Same flow**

---

### Task 9: Ruby Condition Parser

**Files:** Create `crates/apex-concolic/src/ruby_conditions.rs`

Language-specific patterns:
- `if x > 0` / `unless x.nil?` → standard comparisons
- `case x when Type` → `TypeCheck`
- `.nil?` → `NullCheck`
- `.empty?` / `.zero?` → `LengthCheck` / `Compare`

Tests: 4+ tests (lower coverage — Ruby's dynamic typing makes static extraction less reliable).

- [ ] **Step 1-3: Same flow**

---

## Chunk 3: Wiring and Verification

### Task 10: Register All Parsers and Wire CLI

**Files:**
- Modify: `crates/apex-concolic/src/lib.rs` (add all modules + exports)
- Modify: `crates/apex-cli/src/lib.rs` (add concolic strategy dispatch per language)

- [ ] **Step 1: Add all pub mod declarations to lib.rs**

```rust
pub mod boundary;
pub mod rust_conditions;
pub mod go_conditions;
pub mod java_conditions;
pub mod csharp_conditions;
pub mod swift_conditions;
pub mod c_conditions;
pub mod ruby_conditions;
pub mod static_strategy;
```

- [ ] **Step 2: Add CLI dispatch**

In the concolic strategy selection in `apex-cli`, add match arms for each language:
```rust
Language::Rust => StaticConcolicStrategy::new("rust", rust_conditions::parse_rust_conditions),
Language::Go => StaticConcolicStrategy::new("go", go_conditions::parse_go_conditions),
Language::Java | Language::Kotlin => StaticConcolicStrategy::new("java", java_conditions::parse_java_conditions),
Language::CSharp => StaticConcolicStrategy::new("csharp", csharp_conditions::parse_csharp_conditions),
Language::Swift => StaticConcolicStrategy::new("swift", swift_conditions::parse_swift_conditions),
Language::C | Language::Cpp => StaticConcolicStrategy::new("c", c_conditions::parse_c_conditions),
Language::Ruby => StaticConcolicStrategy::new("ruby", ruby_conditions::parse_ruby_conditions),
Language::Wasm => /* stub — return empty suggestions */,
```

- [ ] **Step 3: Full build and test**

```bash
cargo check -p apex-concolic
cargo nextest run -p apex-concolic
cargo clippy -p apex-concolic -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(concolic): wire all language parsers and CLI dispatch"
```
