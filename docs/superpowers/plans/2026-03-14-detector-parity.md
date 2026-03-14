<!-- status: DONE -->
# Detector Language Parity Plan

> **For agentic workers:** REQUIRED: Use fleet crew agents (security-detect crew) for implementation. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the detector gap between Python (21), Rust (18), and JS/TS (10) so all three primary languages have equivalent security coverage.

**Architecture:** Three tracks: (1) Generalize 4 existing Rust-only detectors to work cross-language, (2) Add 7 new JS/TS-specific detectors, (3) Add 2 missing detectors for Rust. All detectors follow the existing `HardcodedSecretDetector` pattern — unit struct, `Detector` trait, regex scanning, `AnalysisContext::test_default()` tests.

**Tech Stack:** Rust, async-trait, regex, LazyLock, uuid

---

## Current State

| Category | Python | Rust | JS/TS | Gap |
|----------|--------|------|-------|-----|
| Injection (SQL, cmd, code) | 4 | 0 | 1 (partial) | JS needs SQL, cmd; Rust needs cmd |
| Path traversal | 2 | 1 | 1 | At parity |
| SSRF | 1 | 0 | 0 | JS needs SSRF |
| Crypto | 1 | 0 | 0 | JS needs crypto |
| Secrets | 2 | 2 | 2 | At parity |
| Session | 1 | 0 | 1 | At parity |
| Deps/License | 2 | 2 | 2 | At parity |
| Timeout | 1 | 0 | 0 | JS needs timeout |
| Panic/abort | 1 | 1 | 1 | At parity |
| Code quality | 1 | 1 | 1 | At parity |
| Language tool (bandit/clippy) | 1 | 1 | 0 | JS needs eslint-security |
| Self-analysis | 0 | 8 | 0 | Generalize 4 to cross-lang |
| **Total** | **17 unique** | **16 unique** | **9 unique** | |

## Target State

After this plan, all three languages will have:
- SQL injection, command injection, SSRF, path traversal
- Crypto failure detection
- HTTP timeout detection
- Secret scanning + session security
- Dependency audit + license scan
- Panic/abort paths
- Code quality (discarded results, operator precedence, duplicated code, exit-in-lib)
- Language-specific tooling integration

---

## Track 1: Generalize Existing Detectors (4 tasks)

These Rust-only detectors use patterns that apply to all languages. Remove the `Language::Rust` guard and add per-language regex patterns.

### Task 1: Generalize `mixed-bool-ops` to all languages

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/mixed_bool_ops.rs`

The `||`/`&&` precedence issue exists in Python (`or`/`and`), JS (`||`/`&&`), Java, C, Ruby — every C-family language.

- [ ] **Step 1: Add tests for Python and JS**

```rust
#[tokio::test]
async fn detects_python_or_and_without_parens() {
    let mut files = HashMap::new();
    files.insert(PathBuf::from("src/main.py"), "if a or b and c:\n".into());
    let ctx = make_ctx(files, Language::Python);
    let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}

#[tokio::test]
async fn detects_js_mixed_ops() {
    let mut files = HashMap::new();
    files.insert(PathBuf::from("src/main.js"), "if (a || b && c) {\n".into());
    let ctx = make_ctx(files, Language::JavaScript);
    let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}
```

- [ ] **Step 2: Remove Language::Rust guard, add Python `or`/`and` support**

```rust
// Remove: if ctx.language != Language::Rust { return Ok(findings); }

// Add Python-specific check alongside existing || && check:
fn has_unparenthesized_mixed_ops(line: &str, lang: Language) -> bool {
    match lang {
        Language::Python => {
            // Python uses `or` and `and` keywords
            let has_or = line.contains(" or ");
            let has_and = line.contains(" and ");
            if !has_or || !has_and { return false; }
            let or_pos = line.find(" or ").unwrap();
            let after_or = &line[or_pos + 4..];
            if !after_or.contains(" and ") { return false; }
            // Check parens
            let before_or = &line[..or_pos];
            let open = before_or.chars().filter(|&c| c == '(').count() as i32;
            let close = before_or.chars().filter(|&c| c == ')').count() as i32;
            open == close
        }
        _ => {
            // C-family: || and &&
            // ... existing logic
        }
    }
}
```

- [ ] **Step 3: Run tests, commit**

```bash
cargo test -p apex-detect mixed_bool_ops
git commit -m "feat: generalize mixed-bool-ops detector to Python and JS"
```

---

### Task 2: Generalize `process-exit-in-lib` to Python and JS

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/process_exit_in_lib.rs`

- Python: `sys.exit()`, `os._exit()`, `exit()` in non-`__main__` files
- JS: `process.exit()` in non-entry files

- [ ] **Step 1: Add per-language patterns**

```rust
static RUST_EXIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(std::)?process::exit\s*\(").unwrap()
});
static PYTHON_EXIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(sys\.exit|os\._exit|exit)\s*\(").unwrap()
});
static JS_EXIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"process\.exit\s*\(").unwrap()
});
```

- [ ] **Step 2: Remove Language::Rust guard, dispatch by language**

```rust
let (pattern, main_file) = match ctx.language {
    Language::Rust => (&*RUST_EXIT, "main.rs"),
    Language::Python => (&*PYTHON_EXIT, "__main__.py"),
    Language::JavaScript => (&*JS_EXIT, "index.js"),  // also skip main.js, server.js
    _ => return Ok(Vec::new()),
};
```

- [ ] **Step 3: Tests for each language, commit**

---

### Task 3: Generalize `discarded-async-result` to JS

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/discarded_async_result.rs`

JS pattern: unhandled promise — `someAsyncFn()` without `await`, `.catch()`, or assignment.
Simpler: detect `void somePromise` or fire-and-forget async calls.

Actually, the most impactful JS pattern is `// eslint-disable-next-line @typescript-eslint/no-floating-promises` — but for detection, flag:
- `void asyncFn()` — explicit discard

- [ ] **Step 1: Add JS test**

```rust
#[tokio::test]
async fn detects_js_void_async() {
    let mut files = HashMap::new();
    files.insert(PathBuf::from("src/lib.js"), "void doAsync();\n".into());
    let ctx = make_ctx(files, Language::JavaScript);
    let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}
```

- [ ] **Step 2: Add JS pattern**

```rust
static JS_VOID_ASYNC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*void\s+\w").unwrap()
});
```

- [ ] **Step 3: Tests, commit**

---

### Task 4: Generalize `duplicated-fn` to Python and JS

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/duplicated_fn.rs`

- Python: `def function_name(` — skip methods inside `class` blocks
- JS: `function functionName(` — skip methods inside `class` blocks

- [ ] **Step 1: Add per-language function regex**

```rust
static RUST_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub\s+)?fn\s+(\w+)\s*\(").unwrap()
});
static PYTHON_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^def\s+(\w+)\s*\(").unwrap()
});
static JS_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:export\s+)?(?:async\s+)?function\s+(\w+)\s*\(").unwrap()
});
```

- [ ] **Step 2: Dispatch by language, skip class blocks for Python/JS**

- [ ] **Step 3: Tests for Python and JS, commit**

---

## Track 2: New JS/TS Detectors (7 tasks)

### Task 5: JS SQL injection detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_sql_injection.rs`

Patterns:
- Template literal in query: `` query(`SELECT * FROM ${table}`) ``
- String concat in query: `"SELECT * FROM " + userInput`
- Raw query methods: `.raw(userInput)`, `knex.raw(`, `sequelize.query(`

```rust
static PATTERNS: &[(&str, &str)] = &[
    (r"\.query\s*\(\s*`[^`]*\$\{", "Template literal in SQL query"),
    (r"\.query\s*\([^)]*\+", "String concatenation in SQL query"),
    (r"\.raw\s*\(", "Raw SQL query — verify parameterized"),
    (r#"\.execute\s*\(\s*["'`]"#, "Direct SQL execution with string literal"),
];
```

CWE-89, `Severity::High`, `FindingCategory::Injection`.
Language guard: `Language::JavaScript` only.

- [ ] **Step 1-4: Tests (positive + negative), implement, commit**

---

### Task 6: JS command injection detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_command_injection.rs`

Patterns:
- `child_process.exec(userInput)` — already partially in security-pattern, but needs specific detector
- `child_process.execSync(` with string interpolation
- `require('child_process').exec(`
- `shelljs.exec(`

CWE-78, `Severity::High`, `FindingCategory::Injection`.

- [ ] **Step 1-4: Tests, implement, commit**

---

### Task 7: JS SSRF detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_ssrf.rs`

Patterns:
- `fetch(userInput)`, `axios.get(userInput)`, `got(userInput)`
- `http.get(userInput)`, `https.get(userInput)`
- URL constructed from user input: `new URL(userInput)`

Negative: `fetch("https://api.example.com")` — hardcoded URL is fine.

CWE-918, `Severity::High`, `FindingCategory::Injection`.

- [ ] **Step 1-4: Tests, implement, commit**

---

### Task 8: JS crypto failure detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_crypto_failure.rs`

Patterns:
- `crypto.createHash('md5')` or `('sha1')` — weak hash
- `crypto.createCipher(` — deprecated, use `createCipheriv`
- `Math.random()` for security purposes (near `token`, `key`, `secret`, `password`)

CWE-327/328, `Severity::Medium`, `FindingCategory::SecuritySmell`.

- [ ] **Step 1-4: Tests, implement, commit**

---

### Task 9: JS timeout detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_timeout.rs`

Patterns:
- `fetch(url)` without `AbortController` / `signal` — no timeout
- `axios.get(url)` without `timeout:` config
- `http.get(` without `timeout` option

CWE-400, `Severity::Low`, `FindingCategory::SecuritySmell`.

- [ ] **Step 1-4: Tests, implement, commit**

---

### Task 10: JS insecure deserialization detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_insecure_deser.rs`

Patterns:
- `yaml.load(` without `safeLoad` — js-yaml < 4.0
- `eval(JSON.parse(` — code execution via JSON
- `new Function(userInput)` — already in security-pattern but without CWE

CWE-502, `Severity::High`, `FindingCategory::Injection`.

- [ ] **Step 1-4: Tests, implement, commit**

---

### Task 11: JS path traversal detector

**Crew:** security-detect
**Files:**
- Create: `crates/apex-detect/src/detectors/js_path_traversal.rs`

Patterns:
- `fs.readFile(userInput)`, `fs.writeFile(userInput)`
- `path.join(baseDir, userInput)` without `path.resolve` + prefix check
- `res.sendFile(userInput)`

CWE-22, `Severity::High`, `FindingCategory::PathTraversal`.

- [ ] **Step 1-4: Tests, implement, commit**

---

## Track 3: Missing Rust Detectors (2 tasks)

### Task 12: Rust command injection patterns in security-pattern

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

Rust currently has only 2 security patterns (`Command::new`, `std::process::Command`). Add:
- `Command::new(user_input)` with `.arg(user_input)` — flag if arg is not a literal
- `std::process::Command::new(format!(...))` — command from format string

- [ ] **Step 1: Add patterns to Rust section, tests, commit**

---

### Task 13: Rust SSRF / HTTP request patterns

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

Add patterns:
- `reqwest::get(user_input)` — URL from variable
- `Client::new().get(format!(...))` — URL from format string
- `hyper::Uri::from_str(user_input)`

- [ ] **Step 1: Add patterns, tests, commit**

---

## Registration & Config

After all detectors are created, update:
- `crates/apex-detect/src/detectors/mod.rs` — add `pub mod` + `pub use` for new files
- `crates/apex-detect/src/pipeline.rs` — register with `Language::JavaScript` guards
- `crates/apex-detect/src/config.rs` — add to `default_enabled()`

---

## Dispatch Plan

```
Track 1 (generalize — modify existing files, sequential):
  Tasks 1-4: mixed-bool-ops, process-exit, discarded-result, duplicated-fn

Track 2 (new JS detectors — new files, parallel):
  ├── Task 5: js-sql-injection
  ├── Task 6: js-command-injection
  ├── Task 7: js-ssrf
  ├── Task 8: js-crypto-failure
  ├── Task 9: js-timeout
  ├── Task 10: js-insecure-deser
  └── Task 11: js-path-traversal

Track 3 (Rust patterns — modify existing file, sequential):
  Tasks 12-13: security-pattern additions
```

Track 1 and Track 3 modify existing files → sequential within track.
Track 2 creates new files → all 7 can run in parallel.
All three tracks are independent → run simultaneously.

---

## Projected Result

| Language | Before | After | Delta |
|----------|--------|-------|-------|
| Python | 21 | 21 | +0 (already benchmark) |
| Rust | 18 | 20 | +2 (cmd injection, SSRF patterns) |
| JS/TS | 10 | 21 | +11 (7 new + 4 generalized) |

All three languages reach **20-21 detectors** with equivalent CWE coverage.
