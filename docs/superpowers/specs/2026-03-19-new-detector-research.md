<!-- status: ACTIVE -->
# Research: Underexplored Static Detection Areas for APEX

**Date:** 2026-03-19
**Scope:** Identify top 20 new detectors across computer science domains not yet covered by APEX

## Current APEX Coverage (Baseline)

Before ranking new detectors, here is what APEX already ships:

**Security (OWASP):** SQL injection, command injection, XSS, SSRF, path traversal, insecure deserialization, broken access control, crypto failures (weak hash, disabled cert validation), hardcoded secrets, secret scanning, session security, subprocess security, bandit-equivalent rules.

**Correctness (Rust-specific):** panic patterns, unsafe reachability, unsafe Send/Sync, partial_cmp unwrap, VecDeque partial eq, discarded async results, process exit in library, mixed bool ops.

**Code Quality:** duplicated functions, data transform spec mining, path normalization.

**Operational:** missing timeouts (Python/JS/Rust), SLO checks, feature flag hygiene (stale/dead flags), config drift detection, trace analysis, memory leak patterns, schema migration checks, dependency audit, license scanning.

**CWEs covered:** 22, 78, 79, 89, 94, 120, 134, 190, 242, 295, 312, 327, 328, 401, 502, 704, 705, 755, 918.

---

## Top 20 New Detectors Ranked by Feasibility and Impact

### Tier 1: High Feasibility, High Impact (Pattern Matching / Simple Data Flow)

---

#### 1. Swallowed Errors / Empty Exception Handlers

**Area:** Error handling anti-patterns
**What it detects:** Empty catch/except/rescue blocks; catch blocks that only log at DEBUG level; bare `except:` or `catch(Exception)` that swallow all errors without re-raising or wrapping.

**Static detectability:** YES -- pure pattern matching on AST. Extremely well-understood.
**False positive rate:** Low (5-10%). Empty catch with a comment like `// intentionally ignored` can be suppressed.
**Languages:** Python, JavaScript/TypeScript, Java, Go (blank `if err != nil {}` blocks), Rust (`let _ = fallible()`)
**Example:**
```python
try:
    process_payment(order)
except Exception:
    pass  # <-- APEX finding: swallowed error in payment path
```
**Existing tools:** CodeQL (`py-empty-except`, `cs-empty-catch-block`), ESLint `no-empty`, PMD, SonarQube. Well-covered but APEX does not have it.
**Priority:** CRITICAL. Studies show ~5% of catch blocks in production Java codebases are empty (Padua et al., IEEE 2017). This is the single most common error-handling bug.
**CWE:** 390 (Detection of Error Condition Without Action), 391 (Unchecked Error Condition)

---

#### 2. Overly Broad Exception Catching

**Area:** Error handling anti-patterns
**What it detects:** `catch(Throwable)`, `catch(Exception)`, bare `except:`, `catch(e: any)` -- catching base exception types that mask bugs.

**Static detectability:** YES -- pattern match on catch clause type.
**False positive rate:** Low-Medium (10-15%). Some framework entry points legitimately catch broadly.
**Languages:** Python, JavaScript/TypeScript, Java, Go (less relevant due to explicit error values)
**Example:**
```java
try {
    transferFunds(from, to, amount);
} catch (Throwable t) {  // <-- catches OutOfMemoryError, StackOverflowError
    log.error("oops", t);
}
```
**Existing tools:** CodeQL (`py-catch-base-exception`), PMD, Checkstyle, Pylint `bare-except`.
**Priority:** HIGH. Masks real bugs; correlated with production incidents.
**CWE:** 396 (Declaration of Catch for Generic Exception)

---

#### 3. Blocking I/O in Async Context

**Area:** Performance anti-patterns / Correctness
**What it detects:** Synchronous file I/O, DNS resolution, sleep(), or database calls inside async functions that will block the event loop / Tokio runtime.

**Static detectability:** YES -- identify async function scope, then check for known blocking calls (`std::fs::read`, `time.sleep`, `requests.get`, `fs.readFileSync`).
**False positive rate:** Medium (15-20%). Some blocking calls are intentionally fast (reading small config files).
**Languages:** Python (asyncio), JavaScript/TypeScript (Node.js), Rust (Tokio/async-std)
**Example:**
```python
async def handle_request(request):
    data = requests.get("https://api.example.com/data")  # BLOCKS event loop
    return Response(data.json())
```
```rust
async fn handler() {
    let content = std::fs::read_to_string("config.toml").unwrap(); // blocks Tokio
}
```
**Existing tools:** No mainstream SAST tool detects this well. Tokio has unstable runtime detection. Clippy has no `sync_in_async` lint. Ruff/Pylint do not flag `requests` in async. This is a significant gap.
**Priority:** CRITICAL. One of the most common production performance bugs in async codebases. Causes latency spikes and cascading failures under load.
**CWE:** 400 (Uncontrolled Resource Consumption -- indirect)

---

#### 4. Wall Clock for Duration Measurement

**Area:** Distributed systems correctness
**What it detects:** Using wall-clock time (`time.time()`, `Date.now()`, `SystemTime::now()`, `System.currentTimeMillis()`) to measure elapsed duration instead of monotonic clocks (`time.monotonic()`, `performance.now()`, `Instant::now()`, `System.nanoTime()`).

**Static detectability:** YES -- pattern match: wall-clock call stored, then subtracted later for elapsed time. Simple heuristic: `end - start` where both are wall-clock calls.
**False positive rate:** Low-Medium (10-15%). Some uses of wall clock are correct (logging timestamps, user-facing dates).
**Languages:** Python, JavaScript, Rust, Java, Go
**Example:**
```python
start = time.time()
do_work()
elapsed = time.time() - start  # NTP adjustment could make this negative
```
**Existing tools:** No mainstream tool detects this. Datadog has a Go-specific lint rule for `regexp.Match` in loops but not clock misuse.
**Priority:** HIGH. Causes incorrect metrics, timeout failures, and impossible-to-reproduce bugs when NTP adjusts. Particularly dangerous in distributed systems.
**CWE:** 682 (Incorrect Calculation)

---

#### 5. String Concatenation in Loops

**Area:** Performance anti-patterns
**What it detects:** Repeated string concatenation (`+=`, `+`) inside loops instead of using StringBuilder/join/push_str/format.

**Static detectability:** YES -- pure pattern matching. Track string variable, detect `+=` or `= ... + ...` pattern inside loop body.
**False positive rate:** Low (5-10%). Short loops with few iterations are false positives but easy to suppress.
**Languages:** Python, Java, JavaScript, Go, Rust (less common due to ownership)
**Example:**
```java
String result = "";
for (Item item : items) {
    result += item.toString() + ",";  // O(n^2) allocation
}
```
**Existing tools:** SonarQube detects this for Java. PMD has `InefficientStringBuffering`. Not broadly covered for Python/JS.
**Priority:** HIGH. O(n^2) performance bug. Very common in Python and Java codebases.
**CWE:** 400 (Uncontrolled Resource Consumption)

---

#### 6. Regex Compilation Inside Loops

**Area:** Performance anti-patterns
**What it detects:** `re.compile()`, `Pattern.compile()`, `new RegExp()`, `Regex::new()` called inside a loop body instead of hoisted outside.

**Static detectability:** YES -- pattern match: regex construction inside loop scope.
**False positive rate:** Medium (15-20%). Impact varies by pattern complexity and iteration count. Datadog's Go lint detects `regexp.Match` in loops specifically.
**Languages:** Python, Java, JavaScript, Go, Rust
**Example:**
```python
for line in log_lines:
    match = re.search(r"\d{4}-\d{2}-\d{2}", line)  # recompiles every iteration
```
**Existing tools:** Datadog static analysis has `go-best-practices/loop-regexp-match`. SonarQube partial coverage. No broad multi-language tool.
**Priority:** MEDIUM-HIGH. Common in log parsing, data processing. Real impact depends on loop size but easy fix.
**CWE:** 400 (Uncontrolled Resource Consumption)

---

#### 7. Error Context Loss on Re-raise

**Area:** Error handling anti-patterns
**What it detects:** Re-raising a different exception without wrapping the original cause, losing the stack trace and context.

**Static detectability:** YES -- detect `raise X` without `from original` in Python; `throw new Exception(msg)` without `cause` in Java; `Err(new_error)` without `.context()` in Rust anyhow usage.
**False positive rate:** Medium (15-20%). Sometimes the original error is intentionally discarded for security (don't expose internals).
**Languages:** Python (`raise X from e`), Java (`new Exception(msg, cause)`), Rust (anyhow `.context()`), Go (`fmt.Errorf("...%w", err)`)
**Example:**
```python
try:
    db.execute(query)
except DatabaseError:
    raise ValidationError("bad input")  # original traceback lost
```
**Existing tools:** Pylint has no rule. SonarQube detects some cases. Semgrep custom rules possible but not in registry.
**Priority:** HIGH. Makes production debugging nearly impossible. Common in codebases that evolved from "catch and rethrow" patterns.
**CWE:** 755 (Improper Handling of Exceptional Conditions)

---

#### 8. Hardcoded Environment-Specific Values

**Area:** Configuration & environment safety
**What it detects:** Hardcoded `localhost`, `127.0.0.1`, `0.0.0.0`, specific port numbers, staging/dev URLs in non-test production code.

**Static detectability:** YES -- regex pattern matching with file-path filtering (exclude test directories).
**False positive rate:** Medium (15-25%). Defaults in config structs, documentation strings, test fixtures.
**Languages:** All languages
**Example:**
```python
# In production handler code, not config:
API_URL = "http://localhost:8080/api/v1"  # will fail in production
```
**Existing tools:** SonarQube has partial rules. Semgrep custom rules exist in some organizations. Not standardized.
**Priority:** MEDIUM-HIGH. Extremely common cause of "works on my machine" deployment failures.
**CWE:** 547 (Use of Hard-coded, Security-relevant Constants)

---

### Tier 2: Medium Feasibility, High Impact (Data Flow Required)

---

#### 9. N+1 Query Patterns

**Area:** Performance anti-patterns
**What it detects:** ORM lazy-loading attribute access inside a loop body, or issuing a database query per iteration when a batch query should be used.

**Static detectability:** YES with data flow -- identify ORM model access (Django `.objects.get()`, SQLAlchemy relationship access, ActiveRecord) inside loops. Requires knowing which attributes trigger lazy loads.
**False positive rate:** Medium-High (20-30%). Some loops genuinely need per-item queries; prefetch may already be configured elsewhere.
**Languages:** Python (Django, SQLAlchemy), JavaScript (Sequelize, Prisma), Ruby (ActiveRecord), Java (Hibernate)
**Example:**
```python
orders = Order.objects.all()
for order in orders:
    customer = order.customer  # lazy load: 1 query per order
    print(customer.name)
```
**Existing tools:** Django Debug Toolbar (runtime), nplusone (Python runtime), Bullet (Ruby runtime). No mainstream static detector.
**Priority:** CRITICAL. The most common performance bug in web applications. N+1 is responsible for more production slowdowns than perhaps any other single pattern. Runtime tools exist but shift-left detection is lacking.
**CWE:** 400 (Uncontrolled Resource Consumption)

---

#### 10. Timing-Attack-Vulnerable Comparisons

**Area:** Cryptographic misuse
**What it detects:** Using `==` to compare HMAC digests, tokens, or passwords instead of constant-time comparison (`hmac.compare_digest`, `crypto.timingSafeEqual`, `constant_time_eq`).

**Static detectability:** YES -- data flow: track HMAC/hash output, check if compared with `==` instead of constant-time function.
**False positive rate:** Low (5-10%). If the variable is named `digest`, `hmac`, `token`, `signature` and compared with `==`, it is almost certainly a bug.
**Languages:** Python, JavaScript/TypeScript, Go, Rust, Java
**Example:**
```python
expected = hmac.new(key, msg, hashlib.sha256).hexdigest()
if request.headers["X-Signature"] == expected:  # timing side-channel
    process()
```
**Existing tools:** Bandit B303 (partial), semgrep community rules (partial). CryptoGuard covers some Java cases. Most tools miss this.
**Priority:** HIGH. Exploitable in real attacks (forge API signatures via timing oracle). Low false positive rate makes it high-value.
**CWE:** 208 (Observable Timing Discrepancy)

---

#### 11. Missing Error Logging on Error Paths

**Area:** Observability gaps
**What it detects:** Functions that catch errors or return errors but have no logging/tracing call on the error path. Silent failures that make production debugging impossible.

**Static detectability:** YES with basic data flow -- detect catch/except blocks or `if err != nil` blocks that don't contain any logging call (`log.`, `logger.`, `console.error`, `tracing::error`).
**False positive rate:** Medium (15-25%). Some functions intentionally propagate errors upward for the caller to log.
**Languages:** All languages
**Example:**
```go
result, err := doWork()
if err != nil {
    return nil, err  // no logging -- if caller also doesn't log, error is silent
}
```
**Existing tools:** No mainstream tool. SonarQube has a rule for empty catch but not for "catch that returns error without logging."
**Priority:** HIGH. Silent failures are one of the top causes of production incidents going undetected.
**CWE:** 778 (Insufficient Logging)

---

#### 12. Unstructured Log Messages

**Area:** Observability gaps
**What it detects:** Using string interpolation/formatting in log calls instead of structured logging fields.

**Static detectability:** YES -- pattern match: `log.info(f"...")`, `logger.info("User {} did {}", user, action)` instead of structured calls like `log.info("user_action", extra={"user": user, "action": action})` or `tracing::info!(user = %user, action = %action, "user action")`.
**False positive rate:** Medium-High (20-30%). Many teams use unstructured logging intentionally. Best deployed as a "house style" rule rather than a bug detector.
**Languages:** Python, JavaScript, Go, Rust, Java
**Example:**
```python
logging.info(f"User {user_id} purchased {item_id} for ${amount}")
# vs structured:
logging.info("purchase_completed", extra={"user_id": user_id, "item_id": item_id, "amount": amount})
```
**Existing tools:** No mainstream tool. Custom Semgrep rules in some organizations.
**Priority:** MEDIUM. Important for observability maturity but more of a code style issue than a bug.
**CWE:** N/A (code quality)

---

#### 13. Missing Retry with Backoff on Network Calls

**Area:** Distributed systems correctness
**What it detects:** HTTP client calls or RPC invocations that lack retry logic or use naive retry (no exponential backoff, no jitter).

**Static detectability:** PARTIAL -- can detect HTTP calls not wrapped in a retry decorator/library. Cannot determine if retry exists at a higher layer (middleware, service mesh).
**False positive rate:** High (25-35%). Retry may be handled by infrastructure (Envoy, Istio), or the call may be intentionally non-retryable.
**Languages:** Python, JavaScript, Go, Java, Rust
**Example:**
```python
response = requests.post("https://payment-service/charge", json=payload)
# No retry, no timeout, no circuit breaker -- one transient failure = user error
```
**Existing tools:** No mainstream tool. This is almost entirely uncharted for static analysis.
**Priority:** MEDIUM-HIGH. Common in microservice architectures. Transient failures cause cascading outages.
**CWE:** 755 (Improper Handling of Exceptional Conditions)

---

#### 14. Phantom Dependencies

**Area:** Supply chain / build security
**What it detects:** Modules imported in source code that are not declared in the dependency manifest (package.json, Cargo.toml, requirements.txt, go.mod). These "phantom" dependencies work because a transitive dependency provides them, but can break when that transitive dependency is updated.

**Static detectability:** YES -- diff import statements against declared dependencies in manifest files.
**False positive rate:** Low-Medium (10-15%). Standard library imports must be excluded; namespace packages in Python can be tricky.
**Languages:** Python, JavaScript/TypeScript, Go, Rust
**Example:**
```python
# requirements.txt has: flask
import werkzeug  # not in requirements.txt, but works via flask
```
**Existing tools:** `deptry` (Python-specific), `depcheck` (JavaScript). No multi-language tool.
**Priority:** MEDIUM-HIGH. Causes cryptic build failures. Supply chain attack vector (attacker registers the phantom package name on public registry).
**CWE:** 1357 (Reliance on Insufficiently Trustworthy Component)

---

#### 15. Dependency Confusion Risk

**Area:** Supply chain / build security
**What it detects:** Private package names that could collide with public registry names. Checks if a package declared in internal config also exists (or could exist) on PyPI/npm/crates.io.

**Static detectability:** YES -- compare internal package names against public registries (requires network call or cached index).
**False positive rate:** Low (5-10%). If the name exists on a public registry and is not owned by the organization, it is a real risk.
**Languages:** Python, JavaScript/TypeScript, Rust, Go
**Example:**
```
# internal requirements.txt
my-company-auth==1.2.3
# If an attacker publishes `my-company-auth` on PyPI at version 99.0.0...
```
**Existing tools:** Snyk, Socket.dev, Aikido. Most require SaaS integration. No open-source CLI tool does this well.
**Priority:** HIGH. 30% of 2025 breaches involved supply chain (Verizon DBIR). This is a concrete, exploitable attack vector.
**CWE:** 427 (Uncontrolled Search Path Element)

---

### Tier 3: Medium Feasibility, Medium Impact (More Complex Analysis)

---

#### 16. Integer Overflow on User-Controlled Input

**Area:** Type system exploitation
**What it detects:** Arithmetic operations on values derived from user input (HTTP parameters, deserialized data) without overflow checks. Particularly dangerous in C, Go, and languages without checked arithmetic.

**Static detectability:** PARTIAL -- requires taint tracking from user input sources to arithmetic operations. False negatives when the data flows through many layers.
**False positive rate:** Medium (15-25%). Many arithmetic operations are on bounded values.
**Languages:** C/C++, Go, Rust (in `wrapping` mode), Java (less critical due to no UB, but still logical bugs)
**Example:**
```go
quantity := r.FormValue("qty")      // user input
q, _ := strconv.Atoi(quantity)
total := q * pricePerUnit            // overflow if qty is INT_MAX
```
**Existing tools:** ELAID (research), Coverity (enterprise), CodeQL (partial). TrustInSoft for C. Good coverage for C/C++, poor for Go/Python.
**Priority:** MEDIUM-HIGH. Critical in financial calculations, memory allocation sizes. CWE-190 is in OWASP Top 25.
**CWE:** 190 (Integer Overflow or Wraparound)

---

#### 17. State Machine Stuck States

**Area:** State machine correctness
**What it detects:** Enum-based state machines where certain states have no outgoing transitions (dead-end states that are not terminal) or states that are defined but unreachable from the initial state.

**Static detectability:** PARTIAL -- requires recognizing state machine patterns in code (match/switch on enum with state transitions). Can build a transition graph and analyze reachability.
**False positive rate:** Medium (15-20%). Requires heuristics to identify which enums represent state machines vs. plain data.
**Languages:** Rust (strong enum patterns), Java, TypeScript, Go
**Example:**
```rust
enum ConnState { Connecting, Open, Closing, Closed, Error }
fn transition(state: ConnState, event: Event) -> ConnState {
    match (state, event) {
        (Connecting, Connected) => Open,
        (Open, Close) => Closing,
        (Closing, Closed) => Closed,
        // Error state has no outgoing transitions -- stuck!
        _ => state,
    }
}
```
**Existing tools:** TLA+, Alloy (formal methods, not integrated into CI). Blue Pearl (hardware FSM lint). No SAST tool does this for application code.
**Priority:** MEDIUM. High value for protocol implementations, connection managers, workflow engines. Niche but high-signal when it fires.
**CWE:** 691 (Insufficient Control Flow Management)

---

#### 18. Error Messages Leaking Internal State

**Area:** Error handling / Information disclosure
**What it detects:** Error responses that include stack traces, internal file paths, database table names, or SQL query fragments sent to end users.

**Static detectability:** YES with data flow -- track exception/error objects flowing into HTTP response bodies. Pattern match for `traceback`, `stack`, `__file__`, SQL fragments in response construction.
**False positive rate:** Medium (15-20%). Development-mode error pages are intentional; need to distinguish from production code paths.
**Languages:** Python (Django DEBUG=True), JavaScript (Express default error handler), Java (Spring), Go
**Example:**
```python
@app.errorhandler(500)
def handle_error(e):
    return {"error": str(e), "trace": traceback.format_exc()}, 500  # leaks internals
```
**Existing tools:** SonarQube (partial), Semgrep community rules (partial for Django DEBUG). Not comprehensive.
**Priority:** MEDIUM-HIGH. Information disclosure enables more targeted attacks. OWASP Top 10 component (Security Misconfiguration).
**CWE:** 209 (Generation of Error Message Containing Sensitive Information)

---

#### 19. Unbounded Collection Growth

**Area:** Performance / Resource safety
**What it detects:** Collections (lists, maps, sets) that grow inside loops or event handlers without any size bound, cap, or eviction policy. Includes unbounded caches, growing log buffers, and append-only in-memory stores.

**Static detectability:** PARTIAL -- detect `.append()`, `.push()`, `.add()`, `.insert()` inside loops or callbacks without corresponding `.pop()`, `.remove()`, size checks, or LRU wrappers.
**False positive rate:** Medium-High (20-30%). Many collections legitimately grow during processing and are discarded after.
**Languages:** All languages
**Example:**
```python
cache = {}
def handle_request(request):
    key = request.session_id
    cache[key] = process(request)  # grows forever, no eviction
```
**Existing tools:** APEX `mem_check` has partial coverage for Python patterns. No tool does this comprehensively cross-language.
**Priority:** MEDIUM. Causes OOM in long-running services. Hard to reproduce in testing.
**CWE:** 400 (Uncontrolled Resource Consumption), 770 (Allocation of Resources Without Limits)

---

#### 20. Non-Idempotent HTTP Handlers

**Area:** Distributed systems correctness
**What it detects:** HTTP POST/PUT handlers that perform side effects (database writes, external API calls) without idempotency protection (no idempotency key check, no upsert pattern, no deduplication).

**Static detectability:** PARTIAL -- identify HTTP handler functions (framework-specific: Flask route, Express handler, Axum handler), check if they perform writes without checking for duplicate request identifiers.
**False positive rate:** High (25-40%). Many POST endpoints are inherently non-idempotent by design (search, read-only queries over POST). Requires understanding of the business logic.
**Languages:** Python, JavaScript/TypeScript, Go, Rust, Java
**Example:**
```python
@app.route("/api/charge", methods=["POST"])
def charge():
    amount = request.json["amount"]
    db.execute("INSERT INTO charges (amount) VALUES (?)", (amount,))
    payment_gateway.charge(amount)
    return {"status": "ok"}
    # Retry = double charge. No idempotency key.
```
**Existing tools:** None. This is entirely novel for static analysis.
**Priority:** MEDIUM. High impact when it matters (payments, state changes), but high false positive rate limits deployability. Best as an advisory/audit rule rather than blocking.
**CWE:** N/A (design pattern)

---

## Honorable Mentions (Not in Top 20)

These are valuable but either too niche, too high in false positives, or already partially covered:

| Detector | Why Not Top 20 |
|----------|---------------|
| Nonce reuse in encryption | Very niche; CryptoGuard covers Java; low occurrence in application code |
| Key derivation without salt | Already partially in APEX security_pattern (CWE 327/328) |
| Missing pagination in list endpoints | Very high false positive rate; framework-specific |
| Missing health check endpoints | Configuration concern, not code pattern |
| Abandoned dependency detection | Requires external data (git history of deps); `deptry`/Snyk do this |
| Pre/post install scripts in deps | npm audit / Socket.dev cover this well |
| Feature flag interactions (A+B) | Combinatorial explosion; no practical static approach exists |
| Missing distributed tracing propagation | Framework-specific; service meshes handle this |
| Implicit type coercion (JS == vs ===) | ESLint `eqeqeq` covers this perfectly |
| Enum exhaustiveness | Rust compiler handles this; TypeScript ESLint has it |
| Quadratic algorithms on user input | Requires algorithmic complexity analysis; very hard statically |
| Missing rate limiting | Infrastructure concern; not reliably detectable in code |
| Pinned vs floating dependency versions | Trivial regex; many tools cover this (Renovate, Dependabot) |
| Certificate validation disabled | Already in APEX security_pattern (CWE 295) |

---

## Implementation Priority Matrix

```
                    HIGH IMPACT                         LOW IMPACT
              +---------------------------+---------------------------+
   EASY       | 1. Swallowed errors       | 5. String concat in loops |
   (pattern   | 2. Overly broad catch     | 6. Regex in loops         |
    match)    | 4. Wall clock misuse      | 8. Hardcoded env values   |
              | 7. Error context loss     | 12. Unstructured logging  |
              +---------------------------+---------------------------+
   MEDIUM     | 3. Blocking I/O in async  | 14. Phantom dependencies  |
   (data      | 9. N+1 query patterns     | 15. Dep confusion risk    |
    flow)     | 10. Timing-attack compare | 19. Unbounded collections |
              | 11. Missing error logging |                           |
              +---------------------------+---------------------------+
   HARD       | 16. Integer overflow/user | 17. State machine stuck   |
   (taint/    | 18. Error msg leaking     | 20. Non-idempotent HTTP   |
    complex)  | 13. Missing retry+backoff |                           |
              +---------------------------+---------------------------+
```

## Recommended Implementation Order

**Phase 1 (Quick Wins -- pattern matching only, 1-2 days each):**
1. Swallowed errors (CWE-390)
2. Overly broad exception catching (CWE-396)
3. Wall clock for duration measurement (CWE-682)
4. String concatenation in loops (CWE-400)
5. Regex compilation in loops (CWE-400)
6. Error context loss on re-raise (CWE-755)
7. Hardcoded environment values (CWE-547)

**Phase 2 (Data Flow -- 2-4 days each):**
8. Blocking I/O in async context (CWE-400)
9. Timing-attack-vulnerable comparisons (CWE-208)
10. Missing error logging on error paths (CWE-778)
11. Unstructured log messages (code quality)

**Phase 3 (Cross-File / External Data -- 3-5 days each):**
12. N+1 query patterns (CWE-400)
13. Phantom dependencies (CWE-1357)
14. Dependency confusion risk (CWE-427)
15. Error messages leaking internal state (CWE-209)

**Phase 4 (Complex Analysis -- 5+ days each):**
16. Integer overflow on user input (CWE-190) -- APEX already has CWE-190 patterns; extend with taint tracking
17. State machine stuck states (CWE-691)
18. Unbounded collection growth (CWE-770)
19. Missing retry with backoff (CWE-755)
20. Non-idempotent HTTP handlers (advisory)

---

## Competitive Differentiation

Tools that cover some of these:
- **SonarQube:** Swallowed errors, broad catch, string concat (Java-focused)
- **CodeQL:** Empty catch, broad catch, some crypto misuse (requires database build)
- **Semgrep:** Custom rules possible but few pre-built for these patterns
- **CryptoGuard:** Java crypto misuse only
- **PMD/Checkstyle:** Java-only error handling
- **ESLint:** JS-only, eqeqeq/no-empty

**The gap APEX can fill:** No single tool covers all 20 patterns across Python, JavaScript/TypeScript, Rust, Go, and Java. The multi-language, zero-config approach is the differentiator. Particularly:
- Blocking I/O in async (no tool does this well)
- Wall clock misuse (no tool does this)
- N+1 query detection statically (all existing tools are runtime)
- Timing-attack comparisons (fragmented coverage)
- Error context loss (no tool does this cross-language)

---

## Sources

- [Studying the Prevalence of Exception Handling Anti-Patterns (IEEE 2017)](https://ieeexplore.ieee.org/document/7961532)
- [CryptoGuard: High Precision Detection of Cryptographic Vulnerabilities](https://dl.acm.org/doi/10.1145/3319535.3345659)
- [CodeQL: Empty except detection](https://codeql.github.com/codeql-query-help/python/py-empty-except/)
- [CodeQL: Catch base exception](https://codeql.github.com/codeql-query-help/python/py-catch-base-exception/)
- [Datadog: Avoid regexp.Match in a loop (Go)](https://docs.datadoghq.com/security/code_security/static_analysis/static_analysis_rules/go-best-practices/loop-regexp-match/)
- [The Trouble with Timestamps (Aphyr)](https://aphyr.com/posts/299-the-trouble-with-timestamps)
- [ELAID: Integer-Overflow-to-Buffer-Overflow Detection](https://link.springer.com/article/10.1186/s42400-020-00058-2)
- [Distributed Systems Patterns and Anti-Patterns (WJAETS 2025)](https://wjaets.com/content/distributed-systems-patterns-and-anti-patterns-comprehensive-framework-scalable-and)
- [Top 12 Software Supply Chain Security Tools (Aikido 2026)](https://www.aikido.dev/blog/top-software-supply-chain-security-tools)
- [Dependency Confusion Attacks (GitGuardian)](https://blog.gitguardian.com/dependency-confusion-attacks/)
- [The Hidden Bottleneck: Blocking in Async Rust](https://cong-or.xyz/blocking-async-rust)
- [Blue Pearl: State Machine Design and Analysis](https://bluepearlsoftware.com/issue-5-state-machine-design-analysis/)
- [Roseau: API Breaking Change Analysis in Java](https://arxiv.org/abs/2507.17369)
- [Loop Performance Anti-Patterns: 40-Repository Study](https://stackinsight.dev/blog/loop-performance-empirical-study)
- [Studying Exception Handling Anti-Pattern Evolution](https://link.springer.com/article/10.1186/s13173-019-0095-5)
