# APEX Detectors & Methodologies Reference

APEX ships 54 static analysis detectors across 9 categories, covering 32 CWEs
and 12 programming languages. All detectors run by default. Findings from
noisy detectors are tagged `noisy: true` so consumers can filter them without
losing data.

## How Detection Works

### Three-Layer Architecture

```
Layer 1: Pattern Matching (all languages, instant)
    Regex scan over source lines — catches ~80% of issues.
    Every detector starts here.

Layer 2: Scope Analysis (Rust/Python/JS/Go/Java)
    Tracks brace/indent depth to determine if a pattern
    is inside an async fn, a loop, an error handler, etc.
    Uses find_scopes() / in_async_fn() / in_loop_body().

Layer 3: Taint Analysis via CPG (Python/JS/Go)
    Code Property Graph traces data flow from sources
    (user input) to sinks (dangerous calls).
    Confirms or downgrades pattern-match findings.
```

### Code Property Graph (CPG)

A CPG combines three views of source code into one queryable graph:

- **AST** (Abstract Syntax Tree) — code structure (assignments, calls, control flow)
- **CFG** (Control Flow Graph) — execution order (which line runs after which)
- **Reaching Definitions** — data flow (where does this variable's value come from?)

When overlaid, you can answer: *"Does user input on line 3 reach the database
query on line 15 without passing through a sanitizer?"*

```python
# WITHOUT CPG — pattern matching flags both:
subprocess.call("ls")              # False positive (hardcoded string)
subprocess.call(user_input)        # True positive (tainted)

# WITH CPG — taint analysis distinguishes:
# "ls" → string literal → not a taint source → downgrade to noisy
# user_input → traces back to request.args → confirmed vulnerability
```

**CPG support by language:**

| Language | CPG Builder | Taint Analysis | Fallback |
|----------|-------------|----------------|----------|
| Python | Line-based parser | Sources, sinks, sanitizers | Pattern matching |
| JavaScript | Line-based parser | Express/Node patterns | Pattern matching |
| Go | Line-based parser | net/http patterns | Pattern matching |
| Rust | Not yet | — | Pattern matching |
| Java, C, C++, etc. | Not yet | — | Pattern matching |

### Threat Model Awareness

Detectors read `apex.toml [threat_model]` to adjust severity:

| Project Type | Command Injection | Path Traversal | Panic |
|-------------|-------------------|----------------|-------|
| `cli-tool` / `console-tool` | noisy + Low | noisy + Low | noisy |
| `web-service` | High | High | High |
| `library` | High | High | High |
| `ci-pipeline` | noisy + Low | noisy + Low | noisy |
| (not set) | High | High | normal |

### Noisy Tagging

Findings from noisy detectors have `"noisy": true` in JSON output. These are
valid patterns but high-volume in certain contexts (CLI tools, test code).

Noisy detectors: `panic-pattern`, `mixed-bool-ops`, `static-analysis`,
`duplicated-fn`, `process-exit-in-lib`, `string-concat-in-loop`,
`regex-in-loop`, `hardcoded-env-values`, `wall-clock-misuse`,
`error-context-loss`, `poisoned-mutex-recovery`.

---

## Detector Catalog

### Security — Injection (10 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `security-pattern` | 78, 89, 94, 502 | Python, Rust | `eval()`, `exec()`, `pickle.loads()`, `Command::new()` with user input indicators. Taint-aware when CPG available. |
| `multi-command-injection` | 78 | All | `os.system()`, `subprocess(shell=True)`, `exec.Command()`, `Runtime.exec()` across all languages. Taint-aware. |
| `multi-sql-injection` | 89 | All | f-string/concat SQL: `f"SELECT {x}"`, `"SELECT " + x`, template literals. Taint-aware. |
| `multi-ssrf` | 918 | All | HTTP calls with user-controlled URL: `requests.get(url)`, `fetch(url)`, `http.Get(url)`. Taint-aware. |
| `multi-path-traversal` | 22 | All | `open(user_path)`, `fs.readFile(user_path)`, `os.Open(user_path)`. Threat-model-aware. |
| `multi-insecure-deser` | 502 | All | `pickle.loads()`, `JSON.parse()` from untrusted source, `ObjectInputStream`, `yaml.load()`. |
| `js-command-injection` | 78 | JS/TS | `exec(`, `spawn(shell:true)`, `child_process` patterns. |
| `js-sql-injection` | 89 | JS/TS | Template literal SQL, string concat SQL. |
| `js-ssrf` | 918 | JS/TS | `fetch`/`axios` with user-controlled URL. |
| `js-path-traversal` | 22 | JS/TS | `path.join(req.*)`, `fs.readFile(userInput)`. |

### Security — Secrets & Crypto (6 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `hardcoded-secret` | 798 | All | AWS keys, private keys, GitHub tokens, API keys — 8 regex patterns. |
| `secret-scan` | 798 | All | 15+ patterns: AWS, GitHub PAT/OAuth, Stripe, JWT, SendGrid, Twilio. Shannon entropy check (configurable threshold, default 5.0). |
| `multi-crypto-failure` | 327, 328 | All | MD5, SHA1, DES, RC4, ECB mode, `random.random()` in security context. |
| `js-crypto-failure` | 327, 328 | JS/TS | `crypto.createHash('md5')`, `Math.random()` for tokens. |
| `session-security` | 798 | Python, JS | Hardcoded `SECRET_KEY` in Flask/Django, inline Express session secrets. |
| `js-insecure-deser` | 502 | JS/TS | `JSON.parse` from untrusted source, `eval` on JSON data. |

### Security — Access & Config (4 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `path-normalize` | 22 | All | File operations without path canonicalization. Threat-model-aware. |
| `broken-access` | 862 | Python | Route handlers without auth decorators (`@login_required`). |
| `unsafe-reachability` | 676 | Rust | `cargo-geiger` output: unsafe fn/expr count per crate. |
| `dependency-audit` | — | Rust, Python, JS, C#, Ruby, Swift, C | `cargo audit`, `pip-audit`, `npm audit`, `bundler-audit`, `osv-scanner`. Graceful fallback when tools absent. |

### Concurrency & Safety (8 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `mutex-across-await` | 833 | Rust | `std::sync::Mutex` guard held past `.await` — deadlock risk. The #1 async Rust footgun. |
| `ffi-panic` | 248 | Rust | `panic!`, `unwrap()`, `expect()` inside `extern "C" fn` — undefined behavior. Severity: Critical. |
| `unbounded-queue` | 400, 770 | Rust, Python | `tokio::sync::mpsc::unbounded_channel()`, `Queue()` without `maxsize` — memory exhaustion. |
| `relaxed-atomics` | 362 | Rust | `Ordering::Relaxed` on shared/static atomic state — stale reads on ARM. Skips test code and local variables. |
| `zombie-subprocess` | 772 | Rust | `Command::output()` in `timeout()` without `kill_on_drop(true)` — child process leaks. |
| `missing-async-timeout` | 400 | Rust | Async `TcpStream::connect`, `reqwest` calls without `tokio::time::timeout` wrapper. |
| `missing-shutdown-handler` | 772 | Rust | `#[tokio::main]` without `tokio::signal` import — no graceful shutdown on SIGTERM. |
| `poisoned-mutex-recovery` | 362 | Rust | `unwrap_or_else(\|e\| e.into_inner())` on Mutex — silently uses potentially corrupted state. |

### Error Handling (3 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `swallowed-errors` | 390 | Python, JS, Java, Go, Rust | Empty `except: pass`, `catch(e) {}`, `if err != nil {}` — silent error suppression. |
| `broad-exception-catching` | 396 | Python, Java | `except Exception:`, `except:`, `catch(Throwable)` — masks OOM, stack overflow. Suppressed when body re-raises. |
| `error-context-loss` | 755 | Python, Rust, JS, Go | `raise X()` without `from e`, `.map_err(\|_\| ...)`, `throw new Error()` without wrapping — loses debug context. |

### Performance (5 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `blocking-io-in-async` | 400 | Rust, Python, JS | `std::fs::read_to_string` in `async fn`, `time.sleep()` in `async def`, `fs.readFileSync` in `async function`. Blocks the executor thread. |
| `string-concat-in-loop` | 400 | Rust, Python, JS, Java | `push_str()` / `+=` on strings inside `for`/`while` loops — O(n^2) allocation. |
| `regex-in-loop` | 400 | Rust, Python, JS, Go | `Regex::new()` / `re.compile()` / `new RegExp()` inside loops. Suppressed when wrapped in `LazyLock` or cache. |
| `connection-in-loop` | 400 | Python, JS, Rust | Database `connect()` inside loop body — connection pool exhaustion. |
| `timeout` | 400 | Python | Python `requests.*` / `httpx.*` / `urlopen` without `timeout=` parameter. |

### Resource Safety (2 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `open-without-with` | 775 | Python | `f = open(...)` without `with` context manager — file descriptor leak. |
| `wall-clock-misuse` | 682 | Rust, Python, JS | `SystemTime::now()` / `time.time()` / `Date.now()` for duration measurement — should use monotonic clock. |

### Environment (1 detector)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `hardcoded-env-values` | 547 | All | `localhost`, `127.0.0.1`, `0.0.0.0` in non-test code. Hardcoded ports in `bind()` calls. |

### Code Quality (10 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `panic-pattern` | 248 | Rust, Python, JS, C, Ruby | `unwrap()`, `expect()`, `panic!()`, `sys.exit()`, `process.exit()`, `abort()`. Noisy for CLI tools. |
| `discarded-async-result` | — | Rust, JS | `let _ = foo.await` — silently drops async Result/Error. |
| `mixed-bool-ops` | — | Rust, C, JS, Python | `a \|\| b && c` without clarifying parentheses. |
| `vecdeque-partial` | 682 | Rust | `VecDeque::as_slices().0` — silently discards wrapped-around data. |
| `substring-security` | 183 | All | `.contains()` / `.starts_with()` as sole check in auth/trust functions. |
| `partial-cmp-unwrap` | 754 | Rust | `.partial_cmp().unwrap()` — panics on NaN. |
| `process-exit-in-lib` | — | Rust, Python, JS | `process::exit()` / `sys.exit()` in library code (not main). |
| `duplicated-fn` | — | All | Same function name defined in multiple files. |
| `unsafe-send-sync` | 362 | Rust | `unsafe impl Send/Sync` without `// SAFETY:` comment. |
| `static-analysis` | — | Rust | Re-surfaces `cargo clippy` JSON output as APEX findings. |

### Compliance & Supply Chain (3 detectors)

| Detector | CWE | Languages | What It Detects |
|----------|-----|-----------|-----------------|
| `license-scan` | — | All | GPL/AGPL/proprietary license violations against configured policy (Permissive or Enterprise). |
| `flag-hygiene` | — | Python, JS, Rust | Stale or always-on feature flags via pattern detection. |
| `bandit` | Various | Python | 15 Bandit-style rules: B102 exec, B301 pickle, B501 verify=False, B602 subprocess shell, etc. |

### ML-Assisted (3 detectors, opt-in)

| Detector | What It Does |
|----------|-------------|
| `dual-encoder` | Semantic similarity for clone/bug detection via dual-encoder embeddings. |
| `hagnn` | HA-GNN graph neural network for taint prediction on CPG. |
| `data-transform-spec` | Temporal property mining from execution traces. |

---

## Scope Utilities

Detectors use shared scope-tracking utilities for context-aware analysis:

| Utility | What It Does |
|---------|-------------|
| `find_scopes(source, lang, opener)` | Finds brace/indent-tracked scopes matching a regex opener. |
| `in_async_fn(source, lang, line)` | Is this line inside an `async fn` / `async def` / `async function`? |
| `in_loop_body(source, lang, line)` | Is this line inside a `for` / `while` / `loop`? |
| `in_except_body(source, lang, line)` | Is this line inside a `catch` / `except` / `if let Err`? |
| `in_test_block(source, line)` | Is this line inside `#[cfg(test)]` or a test function? |
| `is_test_file(path)` | Does the path contain `test`, `.spec.`, `_test.`? |
| `strip_string_literals(line)` | Remove quoted strings to avoid false matches in string content. |
| `taint_reaches_sink(ctx, file, line, indicators)` | Does CPG taint analysis confirm a flow? Returns `Some(true/false)` or `None` (no CPG). |

---

## Configuration

All detectors are enabled by default. Customize in `apex.toml`:

```toml
[detect]
# Disable specific detectors
enabled = ["security", "panic", "timeout"]  # only these run

# Tune thresholds
entropy_threshold = 5.0          # secret-scan Shannon entropy
max_subprocess_concurrency = 4   # parallel subprocess detectors
context_window = 3               # lines of context in findings
```

### Threat Model

```toml
[threat_model]
type = "web-service"  # cli-tool | console-tool | web-service | library | ci-pipeline
```

This single field changes severity for ~1,000+ findings by suppressing patterns
that are expected behavior for the project type.

---

## Statistics

- **54 detectors** (45 enabled by default, 6 multi-language, 3 ML opt-in)
- **32 CWEs** covered
- **12 languages** supported (Python, JS/TS, Rust, Go, Java, Kotlin, C, C++, C#, Swift, Ruby, Wasm)
- **~1,700 tests** in apex-detect
- **3 detection layers**: pattern matching → scope analysis → CPG taint
