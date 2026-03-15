<!-- status: ACTIVE -->
# Language Support — Gap Closure

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close remaining gaps across 6 partially-implemented languages. Most subsystems already exist — this plan targets detector parity, the missing C/C++ index, and the thin Ruby index.

**Architecture:** All languages already have enum, runner, instrumentor, index (except C/C++), and call graph extractor. The gaps are: (1) detector count disparity (Python has 26, Go/C#/Swift have 1 each), (2) missing C/C++ coverage index, (3) thin Ruby index. Sandbox gaps are deferred — only Python/JS/Rust have sandboxes, and this doesn't block core analysis.

**Tech Stack:** Rust, regex, serde

---

## Current State (audited 2026-03-15)

| Language | Enum | Runner | Instr | Index | Reach | Detectors | Overall |
|----------|------|--------|-------|-------|-------|-----------|---------|
| Python | ✅ | ✅ 1062L | ✅ 929L | ✅ 1110L | ✅ 623L | ✅ 26 | **COMPLETE** |
| JavaScript | ✅ | ✅ 792L | ✅ 2252L | ✅ 961L | ✅ 884L | ✅ 20 | **COMPLETE** |
| Rust | ✅ | ✅ 526L | ✅ 1205L | ✅ 1382L | ✅ 955L | ✅ 17 | **COMPLETE** |
| Java | ✅ | ✅ 626L | ✅ 900L | ✅ 370L | ✅ 332L | ✅ 20 | **COMPLETE** |
| C/C++ | ✅ | ✅ 2156L | ✅ 306L | ❌ | ✅ 373L | ✅ 2 | 5/7 — index missing |
| Go | ✅ | ✅ 232L | ✅ 296L | ✅ 275L | ✅ 337L | 🔧 1 | 6/7 — thin detectors |
| C# | ✅ | ✅ 235L | ✅ 314L | ✅ 285L | ✅ 323L | 🔧 1 | 6/7 — thin detectors |
| Ruby | ✅ | ✅ 205L | ✅ 207L | 🔧 89L | ✅ 301L | ✅ 6 | 6/7 — thin index |
| Swift | ✅ | ✅ 220L | ✅ 268L | ✅ 299L | ✅ 282L | 🔧 1 | 6/7 — thin detectors |
| Kotlin | ✅ | ✅ 332L | ❌ | ❌ | ❌ | 🔧 1 | 2/7 — minimal |

**Deferred:** Sandbox support for non-Python/JS/Rust languages. Kotlin full support.

---

## Task 1: C/C++ Coverage Index (Critical Gap)

The only language missing an entire subsystem. C/C++ has runner, instrumentor, and call graph but no coverage parser.

**Files:**
- Create: `crates/apex-index/src/c_cpp.rs`
- Modify: `crates/apex-index/src/lib.rs`

- [ ] **Step 1:** Read `crates/apex-index/src/rust.rs` and `crates/apex-instrument/src/c_coverage.rs` to understand the index pattern and what coverage format the C instrumentor produces
- [ ] **Step 2:** Create `crates/apex-index/src/c_cpp.rs` — parse gcov and llvm-cov JSON coverage output into `BranchIndex`
  - gcov format: `count:line:source` per line
  - llvm-cov JSON: `{"data": [{"files": [{"filename": "...", "segments": [...]}]}]}`
  - Auto-detect format from file structure
- [ ] **Step 3:** Wire into `apex-index/src/lib.rs` module list
- [ ] **Step 4:** Tests with fixture coverage files for both formats
- [ ] **Step 5:** Commit

**Effort:** ~200-300 lines

---

## Task 2: Go Detector Parity

Go has 1 detector vs Python's 26. Add the standard security patterns.

**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs` (add Go `SecurityPattern` entries)

- [ ] **Step 1:** Read existing Go patterns in `security_pattern.rs` to see what's already there
- [ ] **Step 2:** Add Go security patterns:
  - `exec.Command(` (CWE-78) — command injection
  - `db.Query(` / `db.Exec(` + string concat/Sprintf (CWE-89) — SQL injection
  - `http.Get(variable)` / `http.Post(variable)` (CWE-918) — SSRF
  - `template.HTML(` (CWE-79) — XSS via unescaped HTML
  - `os.Open(variable)` / `os.ReadFile(variable)` (CWE-22) — path traversal
  - `json.Unmarshal(` into `interface{}` (CWE-502) — unsafe deserialization
  - `md5.New()` / `sha1.New()` (CWE-327) — weak crypto
  - `log.Fatal(` / `os.Exit(` in library (CWE-705) — exit in library
- [ ] **Step 3:** Tests for each pattern
- [ ] **Step 4:** Commit

**Effort:** ~150-200 lines

---

## Task 3: C# Detector Parity

C# has 1 detector. Add standard .NET security patterns.

**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1:** Read existing C# patterns
- [ ] **Step 2:** Add C# security patterns:
  - `Process.Start(` (CWE-78) — command injection
  - `SqlCommand(` + string concat/interpolation (CWE-89) — SQL injection
  - `HttpClient.GetAsync(variable)` (CWE-918) — SSRF
  - `BinaryFormatter.Deserialize(` (CWE-502) — insecure deserialization
  - `MD5.Create()` / `SHA1.Create()` (CWE-327) — weak crypto
  - `Response.Write(` (CWE-79) — XSS
  - `File.ReadAllText(variable)` (CWE-22) — path traversal
  - `Environment.Exit(` in library (CWE-705)
- [ ] **Step 3:** Tests for each pattern
- [ ] **Step 4:** Commit

**Effort:** ~150-200 lines

---

## Task 4: Swift Detector Parity

Swift has 1 detector. Add Apple platform security patterns.

**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1:** Read existing Swift patterns
- [ ] **Step 2:** Add Swift security patterns:
  - `Process()` / `NSTask` (CWE-78) — command execution
  - `URLSession.shared.dataTask(with: URL(string: variable)` (CWE-918) — SSRF
  - `NSAppleScript(source:` (CWE-94) — code injection
  - `UserDefaults` for sensitive data (CWE-312) — cleartext storage
  - `NSKeyedUnarchiver.unarchiveObject(` (CWE-502) — insecure deserialization
  - `try!` / `fatalError(` in library code (CWE-705)
  - `CC_MD5` / `CC_SHA1` (CWE-327) — weak crypto
- [ ] **Step 3:** Tests for each pattern
- [ ] **Step 4:** Commit

**Effort:** ~150-200 lines

---

## Task 5: Ruby Index Hardening

Ruby index is only 89 lines — minimal SimpleCov JSON parser. Needs branch-level coverage support and test-to-branch mapping.

**Files:**
- Modify: `crates/apex-index/src/ruby.rs`

- [ ] **Step 1:** Read current `ruby.rs` index implementation
- [ ] **Step 2:** Add branch coverage parsing from SimpleCov JSON `branches` key (SimpleCov 0.18+ with `enable_coverage :branch`)
- [ ] **Step 3:** Add test-to-branch mapping from per-test SimpleCov runs
- [ ] **Step 4:** Tests with fixture JSON including branch coverage
- [ ] **Step 5:** Commit

**Effort:** ~100-150 lines

---

## Task 6: C/C++ Detector Expansion

C/C++ has only 2 detectors. Add memory safety and classic C vulnerability patterns.

**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1:** Read existing C/C++ patterns
- [ ] **Step 2:** Add C patterns:
  - `gets(` (CWE-242) — banned function
  - `strcpy(` / `strcat(` / `sprintf(` (CWE-120) — buffer overflow
  - `printf(variable)` without format string (CWE-134)
  - `system(` (CWE-78) — command injection
  - `free(` patterns (CWE-416) — use-after-free heuristic
- [ ] **Step 3:** Add C++ patterns:
  - `reinterpret_cast<` (CWE-704) — unsafe cast
  - `std::system(` (CWE-78)
  - `sprintf` / `vsprintf` (CWE-120) — use `snprintf`
- [ ] **Step 4:** Tests for each pattern
- [ ] **Step 5:** Commit

**Effort:** ~150-200 lines

---

## Execution Order

```
Task 1 (C/C++ index)     ─┐
Task 2 (Go detectors)     ─┤
Task 3 (C# detectors)     ─┼── all independent, dispatch in parallel (Wave 1)
Task 4 (Swift detectors)  ─┤
Task 5 (Ruby index)        ─┤
Task 6 (C/C++ detectors)  ─┘
```

All 6 tasks are independent — different files, different languages. Maximum parallelism.

## Expected Outcomes

| Language | Before | After |
|----------|--------|-------|
| C/C++ | 5/7 (no index) | **6/7** (index + more detectors) |
| Go | 6/7 (1 detector) | **6/7** (8+ detectors) |
| C# | 6/7 (1 detector) | **6/7** (8+ detectors) |
| Swift | 6/7 (1 detector) | **6/7** (7+ detectors) |
| Ruby | 6/7 (thin index) | **6/7** (branch-level index) |

**Total: 6 tasks, ~900-1300 lines, all parallelizable.**

## Verification

After all tasks complete:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo nextest run --workspace
```
