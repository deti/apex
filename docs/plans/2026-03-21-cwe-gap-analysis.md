<!-- status: ACTIVE -->
# APEX CWE Gap Analysis vs Industry Standards

**Date:** 2026-03-21
**Scope:** Compare APEX's detector coverage against OWASP Top 10, CWE Top 25, and competitor tools

---

## 1. Current APEX CWE Coverage (37 CWEs)

APEX detectors reference 37 distinct CWE IDs across 63 detectors:

| CWE | Name | Detector(s) |
|-----|------|-------------|
| 16 | Configuration | iac_scan |
| 22 | Path Traversal | path_traversal, multi_path_traversal, js_path_traversal |
| 78 | OS Command Injection | command_injection, multi_command_injection, js_command_injection, bandit |
| 79 | Cross-Site Scripting (XSS) | security_pattern (mark_safe, res.write, res.send) |
| 89 | SQL Injection | sql_injection, multi_sql_injection, js_sql_injection |
| 94 | Code Injection | security_pattern (eval, exec) |
| 119 | Buffer Overflow | mem_check |
| 134 | Format String | security_pattern |
| 183 | Permissive Allow List | substring_security |
| 248 | Uncaught Exception | panic_pattern |
| 295 | Improper Certificate Validation | bandit (verify=False) |
| 327/328 | Broken Crypto / Weak Hash | crypto_failure, multi_crypto_failure, js_crypto_failure |
| 330 | Insufficient Randomness | crypto_failure |
| 377 | Insecure Temp File | bandit |
| 390 | Error Without Action | swallowed_errors |
| 400 | Uncontrolled Resource Consumption | timeout, js_timeout, missing_async_timeout |
| 502 | Deserialization of Untrusted Data | insecure_deserialization, multi_insecure_deser, js_insecure_deser, bandit |
| 611 | XXE | bandit (ElementTree.parse) |
| 670 | Always-Incorrect Control Flow | mixed_bool_ops |
| 676 | Dangerous Function | static_analysis (banned functions) |
| 710 | Improper Coding Standards | broad_exception, error_context_loss |
| 732 | Incorrect Permission Assignment | bandit (os.chmod) |
| 758 | Undefined Behavior | unsafe_reach, unsafe_send_sync |
| 775 | Missing File Descriptor Release | open_without_with |
| 798 | Hardcoded Credentials | hardcoded_secret, secret_scan |
| 833 | Deadlock | mutex_across_await |
| 862 | Missing Authorization | broken_access |
| 918 | SSRF | ssrf, multi_ssrf, js_ssrf |
| 1104 | Unmaintained Components | dep_audit |
| 1327 | Binding to All Interfaces | bandit |

---

## 2. CWE Top 25 (2024) Coverage Matrix

The 2024 CWE Top 25 Most Dangerous Software Weaknesses:

| Rank | CWE | Name | APEX? | Semgrep | CodeQL | SonarQube | Bearer |
|------|-----|------|-------|---------|--------|-----------|--------|
| 1 | 79 | Cross-Site Scripting | PARTIAL | Yes | Yes | Yes | Yes |
| 2 | 787 | Out-of-bounds Write | NO | Limited | Yes | Yes | No |
| 3 | 89 | SQL Injection | YES | Yes | Yes | Yes | Yes |
| 4 | 352 | Cross-Site Request Forgery | NO | Yes | Yes | Yes | Yes |
| 5 | 22 | Path Traversal | YES | Yes | Yes | Yes | Yes |
| 6 | 125 | Out-of-bounds Read | NO | Limited | Yes | Yes | No |
| 7 | 78 | OS Command Injection | YES | Yes | Yes | Yes | Yes |
| 8 | 416 | Use After Free | NO | No | Yes | Yes | No |
| 9 | 862 | Missing Authorization | YES | Yes | Yes | Yes | Yes |
| 10 | 434 | Unrestricted Upload | NO | Yes | Limited | Yes | Yes |
| 11 | 94 | Code Injection | YES | Yes | Yes | Yes | Yes |
| 12 | 20 | Improper Input Validation | NO | Yes | Yes | Yes | Yes |
| 13 | 77 | Command Injection (gen.) | YES* | Yes | Yes | Yes | Yes |
| 14 | 287 | Improper Authentication | NO | Yes | Yes | Yes | Yes |
| 15 | 269 | Improper Privilege Mgmt | NO | Yes | Limited | Yes | No |
| 16 | 502 | Insecure Deserialization | YES | Yes | Yes | Yes | Yes |
| 17 | 200 | Exposure of Sensitive Info | NO | Yes | Yes | Yes | Yes |
| 18 | 863 | Incorrect Authorization | NO | Yes | Yes | Yes | Yes |
| 19 | 918 | SSRF | YES | Yes | Yes | Yes | Yes |
| 20 | 119 | Buffer Overflow | PARTIAL | Limited | Yes | Yes | No |
| 21 | 476 | NULL Pointer Dereference | NO | No | Yes | Yes | No |
| 22 | 798 | Hardcoded Credentials | YES | Yes | Yes | Yes | Yes |
| 23 | 190 | Integer Overflow | NO | No | Yes | Yes | No |
| 24 | 400 | Uncontrolled Resource Consumption | YES | Yes | Yes | Yes | No |
| 25 | 306 | Missing Critical Auth Step | NO | Yes | Limited | Yes | No |

**Score: 11/25 fully covered, 2 partial = 13/25 (52%)**

---

## 3. OWASP Top 10 (2021) Coverage

| # | Category | CWEs | APEX Coverage | Gap |
|---|----------|------|---------------|-----|
| A01 | Broken Access Control | 22, 862, 863, 352, 200, 269, 434 | 22, 862 covered | Missing CSRF (352), info exposure (200), incorrect authz (863), privilege mgmt (269), file upload (434) |
| A02 | Cryptographic Failures | 327, 328, 330, 295, 798 | All covered | Good coverage |
| A03 | Injection | 78, 79, 89, 94, 502 | All covered | XSS only partial (pattern match, no taint) |
| A04 | Insecure Design | 20 | Not covered | Missing input validation (20) |
| A05 | Security Misconfiguration | 16, 611, 732 | Covered via bandit/iac | Adequate for current scope |
| A06 | Vulnerable Components | 1104 | dep_audit | Covered |
| A07 | Auth Failures | 287, 306, 384, 613 | Not covered | Missing auth bypass (287), session fixation (384), session expiry (613) |
| A08 | Software/Data Integrity | 502, 829, 494 | 502 covered | Missing integrity check failures (829, 494) |
| A09 | Logging/Monitoring Failures | 778, 223 | Not covered | Missing insufficient logging (778) |
| A10 | SSRF | 918 | Covered | Good coverage |

---

## 4. Competitor Coverage Comparison

### Semgrep (~3000 rules)
- Covers all OWASP Top 10 and CWE Top 25 for web languages
- Deep framework-specific rules (Django, Flask, Spring, Express, Rails)
- Taint tracking via `pattern-sources` / `pattern-sinks`
- APEX advantage: multi-language single binary, concurrency detectors, Rust-specific bugs

### CodeQL (~300+ queries)
- AST + data flow + taint analysis for compiled languages
- Strong on memory safety (UAF, OOB, null deref, integer overflow)
- APEX disadvantage: no compiled-language data flow
- APEX advantage: faster, no build required, broader language support per binary

### SonarQube (~5000 rules)
- Broadest rule set, includes code quality + security
- Covers all CWE Top 25
- APEX advantage: no server required, faster feedback loop, better Rust/Go support

### Bearer (Rust CLI, open source)
- Most similar to APEX architecturally (Rust, static analysis, multi-lang)
- Strong on data flow for Ruby/JS/Python/Java
- ~120 security rules covering OWASP Top 10
- Covers: CSRF, auth failures, XSS with taint, info exposure, file upload
- APEX has: concurrency bugs, Rust-specific detectors, CPG, fuzz/symbolic
- **Bearer is the closest competitor and their rule set is the best benchmark**

---

## 5. TOP 10 Missing Detectors to Add Next

Ranked by: CWE Top 25 position, OWASP relevance, feasibility, and competitive gap.

### 1. CWE-352: Cross-Site Request Forgery (CSRF)
- **CWE Top 25 rank:** #4
- **OWASP:** A01 (Broken Access Control)
- **Static analysis?** Yes -- check for POST/PUT/DELETE handlers missing CSRF tokens
- **Needs taint?** No
- **Needs CPG?** No
- **Effort:** LOW (2-3 days)
- **Approach:** Detect form handlers / state-changing endpoints without CSRF middleware or tokens. Check for `csrf_exempt` decorators (Django), missing `csurf` middleware (Express), absent `@csrf_protect` (Flask-WTF). Pattern-based, similar to existing `broken_access` detector.
- **All competitors cover this.**

### 2. CWE-287: Improper Authentication
- **CWE Top 25 rank:** #14
- **OWASP:** A07 (Authentication Failures)
- **Static analysis?** Yes -- detect missing/weak auth patterns
- **Needs taint?** No
- **Needs CPG?** No
- **Effort:** MEDIUM (3-4 days)
- **Approach:** Detect: (a) custom auth implementations instead of framework auth, (b) password comparison with `==` instead of constant-time compare, (c) missing rate limiting on login endpoints, (d) session tokens in URLs. Extend existing `broken_access` and `session_security` detectors.
- **Semgrep, SonarQube, Bearer all cover this.**

### 3. CWE-200: Exposure of Sensitive Information
- **CWE Top 25 rank:** #17
- **OWASP:** A01 (Broken Access Control)
- **Static analysis?** Yes -- detect stack traces, debug modes, verbose errors in production
- **Needs taint?** Partial (for data-flow-based exposure)
- **Needs CPG?** No (basic), Yes (advanced)
- **Effort:** LOW-MEDIUM (2-3 days for basic)
- **Approach:** Detect: (a) `DEBUG = True` in production config, (b) stack traces in HTTP responses (`traceback.format_exc()` in response bodies), (c) sensitive fields in API responses / logs (password, ssn, credit_card in serializers), (d) verbose error messages returned to clients. Pattern-based for most cases.
- **All competitors cover this. Bearer is particularly strong here.**

### 4. CWE-863: Incorrect Authorization
- **CWE Top 25 rank:** #18
- **OWASP:** A01 (Broken Access Control)
- **Static analysis?** Yes -- detect IDOR patterns
- **Needs taint?** Partial
- **Needs CPG?** Helpful but not required
- **Effort:** MEDIUM (3-4 days)
- **Approach:** Detect: (a) database queries using user-supplied IDs without ownership checks (IDOR), (b) role checks that are advisory only (no enforcement), (c) authorization checks after data access rather than before. Extends existing `broken_access` detector which already checks for `objects.get(request.*)` without permission checks. Needs more patterns for JS/Go/Java.
- **Semgrep and SonarQube have extensive rules here.**

### 5. CWE-434: Unrestricted File Upload
- **CWE Top 25 rank:** #10
- **OWASP:** A01 (Broken Access Control)
- **Static analysis?** Yes
- **Needs taint?** No
- **Needs CPG?** No
- **Effort:** LOW (2 days)
- **Approach:** Detect: (a) file upload handlers without extension/MIME validation, (b) uploaded files stored in web-accessible directories, (c) missing file size limits, (d) using user-supplied filename directly (`request.files[].filename` without `secure_filename()`). Pattern-based across Python (Flask/Django), JS (multer/formidable), Go (multipart).
- **Bearer and Semgrep cover this well.**

### 6. CWE-20: Improper Input Validation
- **CWE Top 25 rank:** #12
- **OWASP:** A04 (Insecure Design)
- **Static analysis?** Yes (heuristic)
- **Needs taint?** Helpful
- **Needs CPG?** No
- **Effort:** MEDIUM (3-4 days)
- **Approach:** Detect: (a) request parameters used without validation/parsing (type coercion), (b) missing length/range checks on numeric inputs, (c) regex-based validation without anchoring (`^...$`), (d) allowlist vs denylist patterns. This is broad -- focus on high-signal patterns: unanchored regex validators, type-coerced inputs used in security decisions, missing schema validation on API endpoints.
- **Hardest to get right without false positives. Start narrow.**

### 7. CWE-79: XSS -- Upgrade from PARTIAL to FULL
- **CWE Top 25 rank:** #1
- **OWASP:** A03 (Injection)
- **Static analysis?** Yes
- **Needs taint?** Yes (for real coverage)
- **Needs CPG?** Beneficial
- **Effort:** MEDIUM-HIGH (4-5 days)
- **Current state:** APEX has XSS patterns in `security_pattern.rs` for Django `mark_safe()`, Node `res.write()`/`res.send()`. These are basic sink checks without source-to-sink tracking.
- **Approach:** (a) Add template injection detection (Jinja2 `|safe`, `{% autoescape false %}`), (b) DOM XSS patterns (innerHTML, document.write, eval with location/referrer/cookie), (c) React `dangerouslySetInnerHTML`, (d) use existing `taint_reaches_sink` utility for intra-function flow. This is the #1 CWE -- partial coverage is a competitive liability.
- **Every competitor has deep XSS coverage.**

### 8. CWE-306: Missing Authentication for Critical Function
- **CWE Top 25 rank:** #25
- **OWASP:** A07 (Authentication Failures)
- **Static analysis?** Yes
- **Needs taint?** No
- **Needs CPG?** No
- **Effort:** LOW (1-2 days, extends broken_access)
- **Approach:** Detect admin/management endpoints (patterns: `/admin`, `/manage`, `/internal`, `/debug`) without authentication middleware. Detect database mutation operations (DELETE, UPDATE) in handlers without auth checks. This is a natural extension of the existing `broken_access` detector.

### 9. CWE-190: Integer Overflow/Wraparound
- **CWE Top 25 rank:** #23
- **Static analysis?** Yes (for unchecked arithmetic)
- **Needs taint?** No
- **Needs CPG?** Helpful
- **Effort:** MEDIUM (3 days)
- **Approach:** (a) Python: less relevant (arbitrary precision). (b) Go: detect unchecked `int` arithmetic from user input used in `make()` slice allocation, array indexing. (c) Rust: detect `as` casts that truncate (u64 as u32, i64 as i32), unchecked arithmetic in unsafe blocks. (d) JS: detect parseInt without radix, bitwise ops on large numbers. Go and Rust are highest value.
- **CodeQL and SonarQube cover this; Semgrep/Bearer do not.**

### 10. CWE-778/CWE-223: Insufficient Logging
- **CWE Top 25 rank:** Not in Top 25
- **OWASP:** A09 (Logging/Monitoring Failures)
- **Static analysis?** Yes
- **Needs taint?** No
- **Needs CPG?** No
- **Effort:** LOW-MEDIUM (2-3 days)
- **Approach:** Detect: (a) authentication endpoints without logging (login/logout/register handlers missing log statements), (b) exception handlers that swallow errors without logging (partially covered by `swallowed_errors`), (c) security-relevant operations (password change, permission grant, data deletion) without audit logging. Complements APEX's existing `swallowed_errors` detector.
- **Important for compliance (SOC2, PCI-DSS). SonarQube covers this.**

---

## 6. Implementation Priority Matrix

| Priority | CWE | Effort | Impact | Dependencies |
|----------|-----|--------|--------|--------------|
| P0 | 352 (CSRF) | LOW | HIGH | None -- standalone detector |
| P0 | 79 (XSS upgrade) | MED-HIGH | CRITICAL | taint_reaches_sink exists |
| P1 | 434 (File Upload) | LOW | HIGH | None |
| P1 | 200 (Info Exposure) | LOW-MED | HIGH | None |
| P1 | 306 (Missing Auth) | LOW | MED | Extends broken_access |
| P2 | 287 (Auth Failures) | MED | HIGH | Extends session_security |
| P2 | 863 (Incorrect Authz) | MED | HIGH | Extends broken_access |
| P2 | 20 (Input Validation) | MED | MED | High false-positive risk |
| P3 | 190 (Integer Overflow) | MED | MED | Language-specific |
| P3 | 778 (Logging Failures) | LOW-MED | MED | Compliance-driven |

**Recommended execution order:**
1. Wave 1 (P0): CWE-352, CWE-79 upgrade -- 1 week
2. Wave 2 (P1): CWE-434, CWE-200, CWE-306 -- 1 week
3. Wave 3 (P2): CWE-287, CWE-863, CWE-20 -- 1-2 weeks
4. Wave 4 (P3): CWE-190, CWE-778 -- 1 week

Total estimated effort: 4-5 weeks to go from 52% to 92% CWE Top 25 coverage.

---

## 7. Honorable Mentions (Not Top 10 But Worth Tracking)

| CWE | Name | Why Deferred |
|-----|------|--------------|
| 787 | Out-of-bounds Write | Rank #2 but mostly C/C++ -- APEX doesn't target those languages |
| 125 | Out-of-bounds Read | Rank #6, same as above |
| 416 | Use After Free | Rank #8, memory-safety bug mostly caught by Rust's borrow checker |
| 476 | NULL Pointer Deref | Rank #21, relevant for Go (nil pointer) but low severity |
| 269 | Improper Privilege Mgmt | Rank #15, mostly runtime/config issue, hard to detect statically |
| 384 | Session Fixation | OWASP A07, framework-specific, low prevalence in modern frameworks |
| 829 | Untrusted Resource Inclusion | OWASP A08, CDN/subresource integrity -- very narrow scope |
| 494 | Download Without Integrity | OWASP A08, CI/CD pipeline concern, covered by dep_audit partially |

---

## 8. Strategic Observations

### APEX's Competitive Moat
APEX already covers areas that competitors miss:
- **Concurrency bugs:** mutex_across_await, blocking_io_in_async, unbounded_queue, poisoned_mutex_recovery
- **Rust-specific:** unsafe_reach, unsafe_send_sync, ffi_panic, partial_cmp_unwrap
- **Operational bugs:** zombie_subprocess, missing_shutdown_handler, connection_in_loop
- **Code quality security:** wall_clock_misuse, relaxed_atomics, discarded_async_result

These are not in any CWE Top 25 list but represent real production risk. Do not deprioritize them.

### Where APEX Is Weakest
1. **Web application security** -- CSRF, XSS depth, file upload, auth flows. This is where Bearer and Semgrep dominate.
2. **Information exposure** -- No detector for sensitive data in responses/logs.
3. **Authentication/Authorization depth** -- `broken_access` is basic pattern matching; competitors have framework-aware auth flow analysis.

### Bearer as Primary Benchmark
Bearer is the most architecturally similar tool (Rust CLI, multi-language, static analysis). Their detector set should be the primary competitive benchmark. They cover: CSRF, file upload, XSS with taint, auth bypass, info exposure, cookie security, CORS misconfiguration, open redirect, LDAP injection, header injection, regex DoS. Each of these is a concrete gap.

### Path to "Industry Standard" Coverage
- **52% today** (CWE Top 25)
- **72% after Wave 1+2** (add CSRF, XSS upgrade, file upload, info exposure, missing auth)
- **88% after Wave 3** (add auth failures, incorrect authz, input validation)
- **92% after Wave 4** (add integer overflow, logging failures)
- **Remaining 8%** are memory safety CWEs (787, 125, 416) requiring compiled-language analysis -- not in APEX's target scope.
