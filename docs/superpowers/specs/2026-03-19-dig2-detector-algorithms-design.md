<!-- status: ACTIVE -->
# Dig 2: Concrete Detection Algorithms for 6 New Detectors

**Date:** 2026-03-19
**Based on research:** `docs/superpowers/specs/2026-03-19-new-detector-research.md`
**At commit:** 1ef1266

---

## Architecture Conventions (from existing detectors)

All detectors follow this exact pattern:

```rust
pub struct FooDetector;

#[async_trait]
impl Detector for FooDetector {
    fn name(&self) -> &str { "foo" }
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> { ... }
}
```

Key utilities from `detectors/util.rs`:
- `is_test_file(path)` — skip test files
- `in_test_block(source, line_idx)` — skip Rust `#[cfg(test)]` blocks
- `is_comment(trimmed, lang)` — skip comment lines
- `strip_string_literals(line)` — prevent false positives from patterns inside string literals
- `reachability_evidence(ctx, path, line)` — attach coverage evidence

All regexes use `LazyLock<Regex>` at module level. Tests use `analyze_source(&PathBuf, source)` helper and `#[tokio::test]` for async detector tests.

---

## Detector 1: `blocking-io-in-async`

### Rationale

Synchronous blocking calls inside async functions starve the executor. One `std::fs::read_to_string` call inside a Tokio handler blocks the worker thread for the entire duration of the I/O, preventing it from polling other futures. No mainstream SAST tool catches this cross-language.

### Struct and Fields

```rust
pub struct BlockingIoInAsyncDetector;
```

No configuration fields — all thresholds are hard-coded constants. The detector is stateless.

### Algorithm

The detection uses a two-phase scan per file:

**Phase 1 — Locate async function boundaries.**

Build a list of `AsyncScope { start_line, end_line }` by scanning each line:
- Open: line matches `ASYNC_FN_START_RE` for the given language (see patterns below).
- Track brace/indent depth to find the matching close brace (Rust/JS) or dedent (Python).
- Rust/JS: count `{` and `}`. Scope ends when depth returns to the pre-open depth.
- Python: scope ends when indentation drops back to or below the `async def` indent level. Use the indent of the first non-blank line after `async def` as the body indent.

**Phase 2 — Scan each async scope for blocking calls.**

For each line inside an async scope, strip string literals, skip comments, then check against `BLOCKING_CALLS` for the language. If a match is found:
- Check suppression: Rust only — if within N lines above, a `spawn_blocking` call is visible, skip.
- Emit a finding.

Suppression for all languages: if `in_test_block` (Rust) or `is_test_file` (all), skip.

```
fn analyze_source(path, source, lang) -> Vec<Finding>:
    async_scopes = find_async_scopes(source, lang)
    findings = []
    for (line_idx, line) in source.lines().enumerate():
        if not inside any async_scope: continue
        trimmed = line.trim()
        if is_comment(trimmed, lang): continue
        stripped = strip_string_literals(trimmed)
        if matches_blocking_call(stripped, lang):
            if is_suppressed(source, line_idx, lang): continue
            findings.push(make_finding(path, line_idx+1, matched_call, lang))
    return findings
```

### Pattern Strings

```rust
// Rust: async fn or async move closure start
static RUST_ASYNC_FN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\basync\s+(fn|move|\|)").unwrap()
});

// Python: async def
static PY_ASYNC_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*async\s+def\s+\w+").unwrap()
});

// JS/TS: async function, async arrow, async method
static JS_ASYNC_FN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\basync\s+(?:function|\(|(?:\w+\s*=>))").unwrap()
});
```

Blocking call patterns per language:

```rust
// Rust blocking calls inside async
const RUST_BLOCKING: &[(&str, &str)] = &[
    ("std::fs::",          "std::fs I/O blocks the Tokio worker thread; use tokio::fs instead"),
    ("std::fs::read",      "std::fs::read blocks; use tokio::fs::read"),
    ("std::fs::write",     "std::fs::write blocks; use tokio::fs::write"),
    ("std::thread::sleep", "std::thread::sleep blocks; use tokio::time::sleep"),
    (".read_to_string(",   "synchronous read_to_string; use tokio::io::AsyncReadExt"),
    (".write_all(",        "check: synchronous write_all may block"),
    // Note: .read_to_string and .write_all are checked only if the object is NOT a tokio type.
    // Heuristic: if the line also contains "tokio::" we skip.
];

// Python blocking calls inside async def
const PYTHON_BLOCKING: &[(&str, &str)] = &[
    ("time.sleep(",           "time.sleep blocks the event loop; use asyncio.sleep"),
    ("requests.get(",         "requests.get is synchronous; use aiohttp or httpx async client"),
    ("requests.post(",        "requests.post is synchronous; use aiohttp or httpx async client"),
    ("requests.put(",         "requests.put is synchronous; use aiohttp or httpx async client"),
    ("requests.delete(",      "requests.delete is synchronous; use aiohttp or httpx async client"),
    ("requests.request(",     "requests.request is synchronous; use aiohttp or httpx async client"),
    ("open(",                 "open() is synchronous; use aiofiles.open or asyncio.to_thread"),
    ("urllib.request.urlopen","urllib.request.urlopen is synchronous; use aiohttp"),
    ("socket.connect(",       "socket.connect is synchronous; use asyncio streams"),
    ("subprocess.run(",       "subprocess.run blocks; use asyncio.create_subprocess_exec"),
    ("subprocess.call(",      "subprocess.call blocks; use asyncio.create_subprocess_exec"),
    ("os.system(",            "os.system blocks; use asyncio.create_subprocess_shell"),
];

// JavaScript blocking calls inside async function
const JS_BLOCKING: &[(&str, &str)] = &[
    ("fs.readFileSync(",   "fs.readFileSync blocks the Node.js event loop; use fs.promises.readFile"),
    ("fs.writeFileSync(",  "fs.writeFileSync blocks; use fs.promises.writeFile"),
    ("fs.readdirSync(",    "fs.readdirSync blocks; use fs.promises.readdir"),
    ("fs.statSync(",       "fs.statSync blocks; use fs.promises.stat"),
    ("fs.existsSync(",     "fs.existsSync blocks; use fs.promises.access"),
    ("execSync(",          "execSync blocks the event loop; use util.promisify(exec) or child_process async"),
    ("spawnSync(",         "spawnSync blocks; use spawn with promise wrapper"),
    ("Atomics.wait(",      "Atomics.wait blocks the event loop in non-worker contexts"),
];
```

Suppression (Rust): if any of the 3 lines above the flagged line contain `spawn_blocking`, skip.

### CWE Mapping

CWE-400 (Uncontrolled Resource Consumption). Also relevant: CWE-821 (Incorrect Synchronization).

### Severity

**Medium** — blocks the runtime under load, does not directly cause data corruption or security vulnerability. Escalate to High if the detector is running in a web server context (heuristic: file path contains `handler`, `route`, `server`, `service`).

### Test Cases

**Positive (should flag):**
```rust
// Rust
async fn handler() {
    let content = std::fs::read_to_string("config.toml").unwrap(); // FINDING
}

async fn delay() {
    std::thread::sleep(Duration::from_secs(1)); // FINDING
}
```

```python
# Python
async def handle_request(request):
    data = requests.get("https://api.example.com")  # FINDING
    return data.json()

async def process():
    time.sleep(2)  # FINDING
    with open("data.txt") as f:  # FINDING
        return f.read()
```

```javascript
// JavaScript
async function loadConfig() {
    const data = fs.readFileSync('/etc/config.json');  // FINDING
    return JSON.parse(data);
}
```

**Negative (should not flag):**
```rust
// Not inside async — sync fn is fine
fn sync_handler() {
    let content = std::fs::read_to_string("config.toml").unwrap();
}

// Properly async
async fn handler() {
    let content = tokio::fs::read_to_string("config.toml").await.unwrap();
}

// spawn_blocking suppresses
async fn cpu_work() {
    let result = tokio::task::spawn_blocking(|| std::fs::read("heavy.bin")).await.unwrap();
}
```

```python
# Not inside async def
def sync_fn():
    time.sleep(1)  # not flagged

# Using async-compatible library
async def handle():
    async with aiohttp.ClientSession() as s:
        async with s.get("https://example.com") as r:
            return await r.json()
```

---

## Detector 2: `swallowed-errors`

### Rationale

Empty exception handlers silently discard errors, making failures invisible in production. Padua et al. (IEEE 2017) found ~5% of catch blocks in production Java are empty. This is CWE-390/391 and the single most common error-handling defect.

### Struct and Fields

```rust
pub struct SwallowedErrorsDetector;
```

### Algorithm

The detector uses a three-phase state machine per file:

**State:** `Idle | InExceptClause | InExceptBody { start_brace_depth, lines_seen, has_content }`

For each line:
1. If `Idle` and line matches a catch/except clause pattern, transition to `InExceptClause`.
2. From `InExceptClause`, find the opening `{` (Rust/JS/Java) or record the indent (Python).
   - Rust: `if let Err` — body is the block following. Also matches `let _ = expr;` as a zero-context swallow.
   - Python: body starts on next non-blank, indented line.
   - JS/Java: body starts after `{`.
3. In `InExceptBody`, scan lines:
   - `has_content` = `false` initially.
   - Any non-empty, non-comment, non-`pass` line → `has_content = true`.
   - Brace depth or indent determines when body ends.
4. When body ends: if `!has_content` → emit finding at the clause start line.

Special Rust case: `let _ = some_result;` is a direct swallow. No block tracking needed. Handled as a separate regex pass before the state machine.

```
fn analyze_source(path, source, lang) -> Vec<Finding>:
    findings = []

    // Rust-specific: direct let _ = ... swallow
    if lang == Rust:
        for (line_idx, line) in lines:
            trimmed = strip_string_literals(line.trim())
            if is_comment(trimmed, Rust): continue
            if in_test_block(source, line_idx): continue
            if RUST_LET_DISCARD_RE.is_match(trimmed):
                // exclude let _ = ... .await (already covered by discarded-async-result)
                if !trimmed.contains(".await"):
                    findings.push(swallow_finding(path, line_idx+1, "let _ = ... discards Result"))

    // All languages: empty catch/except/rescue blocks
    state = Idle
    except_start_line = 0
    except_clause_text = ""
    brace_depth = 0
    body_indent = 0
    body_has_content = false
    body_start_depth = 0

    for (line_idx, line) in lines:
        trimmed = line.trim()
        stripped = strip_string_literals(trimmed)
        if is_comment(trimmed, lang): continue

        match state:
        Idle:
            if matches_except_clause(stripped, lang):
                state = InExceptClause
                except_start_line = line_idx
                except_clause_text = stripped

        InExceptClause:
            // Find opening brace / record indent
            if lang in [Rust, JS, Java]:
                for ch in line.chars():
                    if ch == '{': brace_depth += 1; body_start_depth = brace_depth; state = InExceptBody; body_has_content = false; break
                    if ch == '}': brace_depth -= 1
            else: // Python
                if trimmed is not empty and trimmed != except_clause_text:
                    body_indent = indent_of(line)
                    state = InExceptBody
                    body_has_content = is_meaningful_python_line(trimmed)

        InExceptBody:
            if lang in [Rust, JS, Java]:
                for ch in line.chars():
                    if ch == '{': brace_depth += 1
                    if ch == '}':
                        brace_depth -= 1
                        if brace_depth < body_start_depth:
                            // Body ended
                            if !body_has_content:
                                findings.push(swallow_finding(path, except_start_line+1, except_clause_text))
                            state = Idle; break
                if state == InExceptBody and is_meaningful_line(stripped, lang):
                    body_has_content = true
            else: // Python
                current_indent = indent_of(line)
                if trimmed is empty: continue
                if current_indent <= body_indent - 4:  // dedented out of body
                    if !body_has_content:
                        findings.push(swallow_finding(...))
                    state = Idle
                    // reprocess this line as Idle
                else:
                    if is_meaningful_python_line(trimmed):
                        body_has_content = true

    return findings

fn is_meaningful_python_line(trimmed) -> bool:
    trimmed != "pass" and trimmed != "..." and !is_comment(trimmed, Python)

fn is_meaningful_line(stripped, lang) -> bool:
    // Non-empty after stripping — indicates the body has real code
    !stripped.is_empty()
```

### Pattern Strings

```rust
// Rust: let _ = result (not .await)
static RUST_LET_DISCARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*let\s+_\s*=\s*\S+").unwrap()
});

// Rust: if let Err(_) = ... {} with empty block
static RUST_IF_LET_ERR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bif\s+let\s+Err\s*\(").unwrap()
});

// Python: except clause
static PY_EXCEPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*except(\s+\S+)?(\s+as\s+\w+)?\s*:").unwrap()
});

// JavaScript: catch clause
static JS_CATCH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcatch\s*\(\s*\w*\s*\)\s*\{?").unwrap()
});

// Java/Go: catch clause
static JAVA_CATCH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcatch\s*\(\s*\w[\w\s<>]*\s+\w+\s*\)\s*\{?").unwrap()
});

// Go: if err != nil with empty body
static GO_ERR_NIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bif\s+\w+\s*!=\s*nil\s*\{?").unwrap()
});
```

### Language-Specific Variants

| Language | Detection Pattern | Empty Body Indicator |
|----------|-------------------|----------------------|
| Python | `except:` or `except Exception[as e]:` | Body contains only `pass` or `...` |
| JavaScript/TS | `catch (e) {` | `{}` on same line or next line is `}` |
| Java | `catch (Exception e) {` | Next non-comment line is `}` |
| Go | `if err != nil {` | Next non-comment line is `}` |
| Rust | `if let Err(_) = ...` | `{}` immediately follows; also `let _ = ...` one-liner |

### CWE Mapping

- CWE-390: Detection of Error Condition Without Action
- CWE-391: Unchecked Error Condition

### Severity

**High** — silently discarded errors in payment, authentication, and data-mutation paths can corrupt state without any indication.

### Test Cases

**Positive (should flag):**
```python
try:
    process_payment(order)
except Exception:
    pass  # FINDING

try:
    connect()
except:  # FINDING (bare except with only pass)
    pass
```

```javascript
try {
    parseConfig(data);
} catch (e) {}  // FINDING — empty catch
```

```java
try {
    transfer(from, to, amount);
} catch (Exception e) {  // FINDING — empty
}
```

```rust
let _ = write_to_db(&conn, record);  // FINDING — discards Result
if let Err(_) = validate(input) {}   // FINDING — empty error handler
```

**Negative (should not flag):**
```python
try:
    parse_optional()
except Exception as e:
    logger.debug("optional parse failed: %s", e)  # has content, not swallowed

try:
    do_thing()
except ValueError:
    # intentionally ignored per business rule
    pass  # TODO: consider if we should surface this
    # NOTE: still flagged — comment-only body is empty
```

```javascript
try {
    riskyOp();
} catch (e) {
    console.error("riskyOp failed", e);  // not empty
}
```

```rust
if let Err(e) = validate(input) {
    return Err(e.into());  // not empty
}
```

---

## Detector 3: `broad-exception-catching`

### Rationale

Catching base exception types (`Exception`, `BaseException`, `Throwable`) masks bugs including `OutOfMemoryError`, `StackOverflowError`, and programming errors. CWE-396.

### Struct and Fields

```rust
pub struct BroadExceptionCatchingDetector;
```

### Algorithm

Single-pass line scan. For each line:
1. Strip string literals, check not a comment.
2. Match against `BROAD_CATCH_RE` for the language.
3. Check suppression: Python only — if the except body contains a bare `raise` statement (re-raises original), suppress.
4. Emit finding.

Suppression check for Python requires a look-ahead scan of the body (same block-depth logic as `swallowed-errors`). Track: when a broad `except Exception as e:` clause is found, scan its body. If any line is exactly `raise` (bare re-raise) or `raise e` (same exception), do not emit.

```
fn analyze_source(path, source, lang) -> Vec<Finding>:
    findings = []
    lines = source.lines().enumerate().collect()

    for (i, line) in lines:
        trimmed = strip_string_literals(line.trim())
        if is_comment(trimmed, lang): continue
        if is_test_file(path): continue
        if lang == Rust: if in_test_block(source, i): continue

        if matches_broad_catch(trimmed, lang):
            // Check suppression: Python re-raise
            if lang == Python and has_reraise_in_body(lines, i):
                continue
            findings.push(broad_catch_finding(path, i+1, trimmed, lang))

    return findings

fn has_reraise_in_body(lines, except_line_idx) -> bool:
    except_indent = indent_of(lines[except_line_idx])
    // Scan subsequent lines until dedent
    for (i, line) in lines[except_line_idx+1..]:
        if line.trim().is_empty(): continue
        current_indent = indent_of(line)
        if current_indent <= except_indent: break  // left except body
        trimmed = line.trim()
        if trimmed == "raise" or trimmed.starts_with("raise ") or trimmed.starts_with("raise\t"):
            return true
    return false
```

### Pattern Strings

```rust
// Python broad catches
static PY_BROAD_EXCEPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*except\s*(?:\(?\s*(?:Exception|BaseException|SystemExit|KeyboardInterrupt)\s*\)?)?:"
    ).unwrap()
    // matches: bare `except:`, `except Exception:`, `except (Exception, BaseException):`
});

// Java: catch Throwable or Exception
static JAVA_BROAD_CATCH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcatch\s*\(\s*(?:Throwable|Exception|RuntimeException)\s+\w+").unwrap()
});

// Go: no typed catch; errors are values. N/A.

// JavaScript/TypeScript: all catches are untyped; broad catch = empty re-throw or catch-all handler.
// JS: no typed catch (catch-all by language design), so we detect the pattern of
// `catch (e)` bodies that swallow with `console.log` only (covered by swallowed-errors).
// Broad-exception-catching is N/A for JS — skip this language.
```

Severity escalation: Java `catch (Throwable` is High (catches OOM). Python `except BaseException:` is High. Python `except Exception:` is Medium.

### Language-Specific Variants

| Language | Pattern | Severity |
|----------|---------|---------|
| Python | bare `except:` | High |
| Python | `except BaseException:` | High |
| Python | `except Exception:` | Medium |
| Java | `catch (Throwable` | High |
| Java | `catch (Exception` | Medium |
| JavaScript | N/A (untyped catch) | skip |
| Go | N/A (explicit error values) | skip |
| Rust | N/A (Result-based) | skip |

### CWE Mapping

CWE-396: Declaration of Catch for Generic Exception

### Severity

**High** for `Throwable`/`BaseException`. **Medium** for `Exception`.

### Test Cases

**Positive (should flag):**
```python
except Exception:         # FINDING Medium
    log_and_continue()

except BaseException:     # FINDING High
    pass

except:                   # FINDING High (bare)
    pass
```

```java
catch (Throwable t) {     // FINDING High
    log.error("oops", t);
}

catch (Exception e) {     // FINDING Medium
    // handle
}
```

**Negative (should not flag):**
```python
except Exception as e:
    raise  # re-raises original — suppressed

except ValueError:
    handle_value_error()  # specific exception type

except (TypeError, KeyError):
    handle_type_key()  # specific types
```

---

## Detector 4: `error-context-loss`

### Rationale

Re-raising a new exception inside an error-handling block without linking the original cause destroys the call chain. Debugging a `ValidationError` with no context about the underlying `DatabaseError` is a production nightmare. CWE-755.

### Struct and Fields

```rust
pub struct ErrorContextLossDetector;
```

### Algorithm

Multi-phase scan:

1. **Scope tracking:** detect when we're inside an error-handling block.
   - Python: after `except` line, until dedent.
   - Rust: inside `Err` arm of `match` or after `if let Err(e) =`.
   - JS: inside `catch` body.

2. **Raise detection:** while inside error scope, look for a raise/throw that doesn't chain:
   - Python: `raise X(...)` without `from` → context loss. But `raise` alone (bare re-raise) is fine.
   - Rust: `.map_err(|_| ...)` (underscore discards original) → context loss.
   - JS: `throw new Error(msg)` without wrapping original `e` → context loss.

3. Emit finding at the raise/throw line.

```
fn analyze_source(path, source, lang) -> Vec<Finding>:
    findings = []

    if lang == Python:
        for (i, line) in lines:
            trimmed = strip_string_literals(line.trim())
            if is_comment(trimmed, Python): continue
            if is_in_except_body(source, i):
                if PY_RAISE_WITHOUT_FROM_RE.is_match(trimmed):
                    if !trimmed.starts_with("raise ") or trimmed == "raise":
                        continue  // bare raise is ok
                    findings.push(ctx_loss_finding(path, i+1, "Python raise without 'from e' loses original traceback"))

    if lang == Rust:
        for (i, line) in lines:
            trimmed = strip_string_literals(line.trim())
            if is_comment(trimmed, Rust): continue
            if in_test_block(source, i): continue
            if RUST_MAP_ERR_DISCARD_RE.is_match(trimmed):
                findings.push(ctx_loss_finding(path, i+1, ".map_err(|_| ...) discards original error"))

    if lang in [JavaScript, TypeScript]:
        in_catch = false; catch_brace_depth = 0; catch_err_var = ""
        for (i, line) in lines:
            trimmed = strip_string_literals(line.trim())
            if is_comment(trimmed, JS): continue
            if JS_CATCH_RE.is_match(trimmed):
                in_catch = true
                catch_err_var = extract_catch_var(trimmed)  // e.g. "e" from catch(e)
                catch_brace_depth = brace_depth_before_catch
            if in_catch:
                // track brace depth to know when catch ends
                if brace_depth < catch_brace_depth: in_catch = false; continue
                if JS_THROW_NEW_RE.is_match(trimmed):
                    if catch_err_var not in trimmed:
                        // throw new Error(msg) without wrapping catch_err_var
                        findings.push(ctx_loss_finding(path, i+1, "throw new Error() in catch doesn't wrap original error"))

    return findings
```

### Pattern Strings

```rust
// Python: raise X(...) without `from`
// Matches: raise ValueError("msg") but NOT raise ValueError("msg") from e
static PY_RAISE_WITHOUT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*raise\s+\w[\w.]*\s*\(.*\)\s*$").unwrap()
    // trailing $ ensures no ` from` after the paren — but we also check !contains(" from ")
});

// Rust: .map_err(|_| ...) — discards original error
static RUST_MAP_ERR_DISCARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.map_err\(\s*\|_\|").unwrap()
});

// JavaScript: throw new SomeError(...)
static JS_THROW_NEW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bthrow\s+new\s+\w+Error\s*\(").unwrap()
});
```

Additional Python suppression: if the line contains ` from `, skip (e.g., `raise NewError("msg") from original`).

### Language-Specific Variants

| Language | Pattern | Fix Suggestion |
|----------|---------|---------------|
| Python | `raise X(msg)` inside except without `from e` | Add `from e`: `raise X(msg) from e` |
| Rust | `.map_err(\|_\| ...)` | Use `.map_err(\|e\| MyError::from(e))` or `.context("msg")` with anyhow |
| JS | `throw new Error(msg)` in catch without wrapping `e` | Use `throw new Error(msg, { cause: e })` (ES2022) or wrap: `throw new AppError(msg, e)` |
| Go | `errors.New(msg)` in err-handling without `%w` wrap | Use `fmt.Errorf("context: %w", err)` |

Go pattern (additional):
```rust
static GO_ERRORS_NEW_IN_ERR_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r'\berrors\.New\s*\(').unwrap()
    // Only flagged when inside `if err != nil` block (tracked by brace depth)
});
```

### CWE Mapping

CWE-755: Improper Handling of Exceptional Conditions

### Severity

**Medium** — does not cause immediate crashes but severely impedes debugging and incident response.

### Test Cases

**Positive (should flag):**
```python
try:
    db.execute(query)
except DatabaseError:
    raise ValidationError("bad input")  # FINDING — loses original DatabaseError

try:
    connect()
except ConnectionError:
    raise RuntimeError(str(e))  # FINDING — loses stack trace
```

```rust
result.map_err(|_| MyError::Generic)?;  // FINDING — discards original error
```

```javascript
try {
    parseConfig(data);
} catch (e) {
    throw new Error("config invalid");  // FINDING — e not wrapped
}
```

**Negative (should not flag):**
```python
try:
    db.execute(query)
except DatabaseError as e:
    raise ValidationError("bad input") from e  # OK — chained

try:
    connect()
except ConnectionError:
    raise  # OK — bare re-raise preserves chain
```

```rust
result.map_err(|e| MyError::Database(e))?;  // OK — wraps original
result.context("database operation failed")?;  // OK — anyhow context
```

```javascript
try {
    parseConfig(data);
} catch (e) {
    throw new Error("config invalid", { cause: e });  // OK — ES2022 cause
}
```

---

## Detector 5: `string-concat-in-loop`

### Rationale

String concatenation in a loop produces O(n²) allocations because each `+=` copies the entire accumulated string. For n=10,000 iterations on a 100-char average line, this is 5 billion bytes of allocation. The fix (collect + join, StringBuilder) is trivial. CWE-400.

### Struct and Fields

```rust
pub struct StringConcatInLoopDetector;
```

### Algorithm

Two-phase per file:

**Phase 1 — Detect loops.** Build list of `LoopScope { start_line, end_line }` by:
- Matching `LOOP_START_RE` for the language.
- Tracking brace/indent depth (same as blocking-io-in-async async scope tracking).
- Nested loops are treated as separate scopes; inner-loop findings are reported once.

**Phase 2 — Detect string concatenation inside loops.**

For each line inside a loop scope:
1. Strip string literals and comments.
2. Match string-concat patterns for the language.
3. Check that the variable being concatenated was not declared in the loop initializer (loop-local variables that are later joined are fine — hard to check statically, so we flag and let the user judge).
4. Emit finding.

```
fn analyze_source(path, source, lang) -> Vec<Finding>:
    loop_scopes = find_loop_scopes(source, lang)
    seen_lines = HashSet::new()  // deduplicate (nested loops)
    findings = []

    for (i, line) in lines:
        if not inside_any_loop_scope(loop_scopes, i): continue
        if seen_lines.contains(i): continue
        seen_lines.insert(i)
        trimmed = strip_string_literals(line.trim())
        if is_comment(trimmed, lang): continue
        if lang == Rust and in_test_block(source, i): continue
        if matches_str_concat(trimmed, lang):
            findings.push(str_concat_finding(path, i+1, lang))

    return findings
```

### Pattern Strings

```rust
// Rust loop starts
static RUST_LOOP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:for\s+\w|while\s+|loop\s*\{)").unwrap()
});

// Python loop starts (anchored to start of trimmed line)
static PY_LOOP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:for|while)\s+").unwrap()
});

// JavaScript loop starts
static JS_LOOP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:for\s*\(|while\s*\(|for\s+(?:const|let|var)\s+\w+\s+(?:of|in)\s+)").unwrap()
});

// Java loop starts
static JAVA_LOOP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:for\s*\(|while\s*\()").unwrap()
});

// String concatenation patterns
// Rust: str_var += &... or str_var.push_str(
static RUST_STR_CONCAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\w+\s*\+=\s*&|\w+\.push_str\s*\(|\w+\.push\s*\()").unwrap()
    // push_str and push are both O(n) amortized but repeated += is the issue
    // Only flag += & (which concatenates), not push_str (which is fine! amortized O(1))
    // CORRECTION: += on String is the issue. push_str is fine.
});
// Corrected: only flag += on what looks like a String
static RUST_STR_PLUS_EQ_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\w+\s*\+=\s*").unwrap()
    // Broad but in loop context + String type context is signal enough
    // False positive: numeric += is also caught. Disambiguate by checking for string literal or &str
    // Heuristic: if RHS starts with " or & or a variable (not a number), flag it
});

// Python: str_var += ... or str_var = str_var + ...
static PY_STR_CONCAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\w+\s*\+=\s*[^\d]|\w+\s*=\s*\w+\s*\+\s*)").unwrap()
});

// JavaScript: same as Python
static JS_STR_CONCAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\w+\s*\+=\s*[^\d=]|\w+\s*=\s*\w+\s*\+\s*[^\d=])").unwrap()
});

// Java: String += or String = String + ...
static JAVA_STR_CONCAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\w+\s*\+=\s*|\w+\s*=\s*\w+\s*\+\s*)").unwrap()
    // Context: Java, inside a for loop. Numeric + is also caught but StringBuilder
    // recommendation is still valid for the numeric case (less critical).
    // Suppress if variable was declared as int/long/double earlier in the scope.
});
```

Disambiguation heuristic for Rust: only flag `var += expr` if `expr` starts with `&`, `"`, or a variable name not matching a number literal. This filters `count += 1`.

### Language-Specific Variants

| Language | Flag Pattern | Suggested Fix |
|----------|-------------|---------------|
| Rust | `s += &other_str` in loop | Collect into `Vec<String>` then `s = parts.join("")` |
| Python | `s += chunk` or `s = s + chunk` in loop | Use `parts.append(chunk)` then `"".join(parts)` |
| JavaScript | `s += chunk` or `s = s + chunk` in loop | Use array push + `.join("")` or template literal |
| Java | `str += chunk` in loop | Use `StringBuilder` |
| Go | `s += chunk` in loop | Use `strings.Builder` |

### CWE Mapping

CWE-400: Uncontrolled Resource Consumption

### Severity

**Low** for small/bounded loops. **Medium** when the loop is a handler/request scope (heuristic: function name contains `handle`, `process`, `parse`, `build`). Use Low as default.

### Test Cases

**Positive (should flag):**
```python
result = ""
for item in items:
    result += item + ","  # FINDING

for line in file_lines:
    output = output + line  # FINDING
```

```java
String result = "";
for (Item item : items) {
    result += item.toString() + ",";  // FINDING
}
```

```rust
let mut result = String::new();
for part in parts {
    result += &part;  // FINDING
}
```

**Negative (should not flag):**
```python
parts = []
for item in items:
    parts.append(item)
result = "".join(parts)  # OK — join pattern

count = 0
for item in items:
    count += 1  # numeric, not string
```

```rust
let result: String = parts.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(",");  // OK
```

---

## Detector 6: `regex-in-loop`

### Rationale

Compiling a regex is expensive (involves parsing, NFA/DFA construction). Doing it inside a loop means O(n) compilations that should be O(1). CWE-400. Datadog tracks this specifically for Go; no tool covers all five target languages.

### Struct and Fields

```rust
pub struct RegexInLoopDetector;
```

### Algorithm

Identical loop-scope tracking as `string-concat-in-loop`. After locating loop scopes, scan each interior line for regex compilation calls.

Suppression: if the regex pattern argument is a compile-time constant (quoted string literal with no variable interpolation), the compiler may intern it — but in most languages it still recompiles every iteration at runtime. Only suppress if the language has explicit static compilation (`lazy_static!` / `LazyLock::new` outside the loop, `re.compile()` result cached in a variable declared outside the loop). The heuristic: if the `compile` call is assigned to a variable that was declared _before_ the loop, it's fine. This requires a two-pass look-back, which is approximated by: if the compiled regex is assigned to a `let`/`const`/`var` binding on the _same line_ and that binding name appears as the LHS of a declaration within 20 lines before the loop start, suppress.

Practical implementation: don't attempt the full suppression heuristic in the first pass. Flag all regex compilations inside loops and let the user add a `// apex-ignore: regex-in-loop` suppression comment if they've already hoisted it.

```
fn analyze_source(path, source, lang) -> Vec<Finding>:
    loop_scopes = find_loop_scopes(source, lang)
    findings = []

    for (i, line) in lines:
        if not inside_any_loop_scope(loop_scopes, i): continue
        trimmed = strip_string_literals(line.trim())
        if is_comment(trimmed, lang): continue
        if lang == Rust and in_test_block(source, i): continue

        if matches_regex_compile(trimmed, lang):
            findings.push(regex_loop_finding(path, i+1, lang))

    return findings
```

### Pattern Strings

```rust
// Rust: Regex::new( inside loop
static RUST_REGEX_NEW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bRegex::new\s*\(").unwrap()
});

// Python: re.compile( or re.search/match/findall without pre-compiled pattern (all re.* calls recompile)
// Flag re.compile( (explicit) and also re.search/match/findall (implicit compile each call)
static PY_REGEX_COMPILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bre\.(?:compile|search|match|fullmatch|findall|finditer|sub|subn|split)\s*\(").unwrap()
});

// JavaScript: new RegExp(
static JS_REGEX_NEW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bnew\s+RegExp\s*\(").unwrap()
    // Note: /pattern/flags regex literals in JS are NOT compiled at runtime per iteration — they are
    // compiled once at parse time. So /foo/g inside a loop is fine. Only `new RegExp(...)` is an issue.
});

// Go: regexp.Compile( or regexp.MustCompile( or regexp.Match(
static GO_REGEX_COMPILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bregexp\.(?:Compile|MustCompile|Match|MatchString|MatchReader)\s*\(").unwrap()
});

// Java: Pattern.compile(
static JAVA_PATTERN_COMPILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bPattern\.compile\s*\(").unwrap()
});
```

JavaScript note: `/regex/flags` literals in JS are compile-time constants (interned by V8 and other engines). Only `new RegExp(expr)` is runtime compilation. This is a meaningful distinction — false positive avoidance.

### Language-Specific Variants

| Language | Compile Call | Notes |
|----------|-------------|-------|
| Rust | `Regex::new(` | Use `LazyLock<Regex>` at module scope |
| Python | `re.compile(` and all `re.*` convenience functions | Hoist `re.compile()` result before loop |
| JavaScript | `new RegExp(` only | Regex literals `/foo/` are fine |
| Go | `regexp.Compile(`, `regexp.MustCompile(`, `regexp.Match*` | Use `regexp.MustCompile` at package init |
| Java | `Pattern.compile(` | Use `static final Pattern` |

### CWE Mapping

CWE-400: Uncontrolled Resource Consumption

### Severity

**Low** — performance issue, not a correctness or security bug. In tight inner loops over large datasets (log parsing, data pipelines) this can be severe, but cannot determine that statically.

### Test Cases

**Positive (should flag):**
```python
for line in log_lines:
    match = re.search(r"\d{4}-\d{2}-\d{2}", line)  # FINDING — recompiles each iteration
    m2 = re.compile(r"\w+").match(line)  # FINDING
```

```rust
for line in lines {
    let re = Regex::new(r"\d+").unwrap();  // FINDING
    if re.is_match(line) { ... }
}
```

```javascript
for (const item of items) {
    const re = new RegExp("^\\d+$");  // FINDING
    if (re.test(item.id)) { ... }
}
```

```go
for _, line := range lines {
    matched, _ := regexp.MatchString(`\d+`, line)  // FINDING
}
```

**Negative (should not flag):**
```python
pattern = re.compile(r"\d{4}-\d{2}-\d{2}")  # compiled once, outside loop
for line in log_lines:
    match = pattern.search(line)  # OK — using pre-compiled pattern
```

```rust
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap());
for line in lines {
    if DATE_RE.is_match(line) { ... }  // OK — LazyLock
}
```

```javascript
const re = /^\d+$/;  // JS regex literal — compiled once by engine
for (const item of items) {
    if (re.test(item.id)) { ... }  // OK
}

const re2 = new RegExp("^\\d+$");  // compiled once outside loop
for (const item of items) {
    re2.test(item.id);  // OK
}
```

---

## Shared Implementation Notes

### Loop Scope Tracking (shared by detectors 1, 5, 6)

Extract into `util.rs` as a shared function:

```rust
pub struct Scope {
    pub start_line: usize,  // 0-based
    pub end_line: usize,    // 0-based, inclusive
}

/// Find all async function / loop scopes in the source, depending on the `pattern`
/// regex used to detect the scope opener.
pub fn find_scopes(source: &str, lang: Language, scope_opener: &Regex) -> Vec<Scope>
```

Brace-tracked (Rust/JS/Java/Go) and indent-tracked (Python) variants. This prevents duplicating the scope logic across three detectors.

### Language Dispatch

The `analyze()` method in each detector follows a consistent pattern:

```rust
async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for (path, source) in &ctx.source_cache {
        if is_test_file(path) { continue; }
        match ctx.language {
            Language::Rust => findings.extend(analyze_rust(path, source)),
            Language::Python => findings.extend(analyze_python(path, source)),
            Language::JavaScript | Language::TypeScript => findings.extend(analyze_js(path, source)),
            Language::Java => findings.extend(analyze_java(path, source)),
            Language::Go => findings.extend(analyze_go(path, source)),
            _ => {}
        }
    }
    Ok(findings)
}
```

### Registration in `mod.rs`

Each detector is registered in `crates/apex-detect/src/detectors/mod.rs` by adding:
```rust
pub mod blocking_io_in_async;
pub mod swallowed_errors;
pub mod broad_exception_catching;
pub mod error_context_loss;
pub mod string_concat_in_loop;
pub mod regex_in_loop;
```

And in the `all_detectors()` factory (or equivalent):
```rust
Box::new(BlockingIoInAsyncDetector),
Box::new(SwallowedErrorsDetector),
Box::new(BroadExceptionCatchingDetector),
Box::new(ErrorContextLossDetector),
Box::new(StringConcatInLoopDetector),
Box::new(RegexInLoopDetector),
```

---

## Summary Table

| Detector | Struct Name | CWE | Severity | Languages | Priority |
|----------|------------|-----|----------|-----------|---------|
| `blocking-io-in-async` | `BlockingIoInAsyncDetector` | 400 | Medium | Rust, Python, JS | P0 |
| `swallowed-errors` | `SwallowedErrorsDetector` | 390, 391 | High | Rust, Python, JS, Java, Go | P0 |
| `broad-exception-catching` | `BroadExceptionCatchingDetector` | 396 | High/Medium | Python, Java | P0 |
| `error-context-loss` | `ErrorContextLossDetector` | 755 | Medium | Python, Rust, JS, Go | P1 |
| `string-concat-in-loop` | `StringConcatInLoopDetector` | 400 | Low | Rust, Python, JS, Java, Go | P0 |
| `regex-in-loop` | `RegexInLoopDetector` | 400 | Low | Rust, Python, JS, Go, Java | P0 |

---

## Implementation Order

1. `swallowed-errors` — highest impact, broadest language coverage, simple state machine
2. `broad-exception-catching` — simplest algorithm (single-pass scan), Python/Java only
3. `string-concat-in-loop` — loop scope util needed; builds foundation for detector 6
4. `regex-in-loop` — reuses loop scope util from detector 3
5. `blocking-io-in-async` — requires async scope tracking (similar to loop scope, new util)
6. `error-context-loss` — most complex (error scope + raise analysis + multiple language patterns)

Detectors 3+4 share `find_scopes()` util. Detector 5 shares the same util with a different scope opener regex. Implement the util alongside detector 3.

---

## Open Questions for Dig 3 (Implementation)

1. **Python indent tracking:** The indent-based scope detection needs a robust implementation that handles mixed tabs/spaces and one-liner bodies (`except Exception: pass`). Consider normalizing to 4-space equivalents.

2. **Rust async scope precision:** `async move {}` closures and `async { ... }` blocks are common patterns. The `RUST_ASYNC_FN_RE` must handle these. Consider whether closures assigned to variables should also be tracked.

3. **JavaScript regex literals:** V8 caches `/regex/flags` literals as compile-time constants. The spec says do not flag them. Verify this is also true in SpiderMonkey / JavaScriptCore for completeness.

4. **`swallowed-errors` and `broad-exception-catching` overlap:** A `except Exception: pass` triggers both detectors. Either deduplicate in the registry or accept that both fire (different CWEs, different fix suggestions, both valid).

5. **Suppression comments:** Should `// apex-ignore: <detector-name>` be supported on the preceding line? Existing detectors do not implement this yet. Consider adding to `util.rs` as a general mechanism.
