# APEX Detectors & Methodologies Reference

APEX ships 63 static analysis detectors across 10 categories, covering 40+ CWEs
and 11 programming languages. All detectors run by default. Findings from
noisy detectors are tagged `noisy: true` so consumers can filter them without
losing data.

## Security Detectors — Multi-Language (All 11 languages)

| Detector | CWE | What it finds |
|----------|-----|---------------|
| `multi-command-injection` | CWE-78 | `subprocess.run`, `exec.Command`, `Runtime.exec`, `system()`, `Process.Start` |
| `multi-sql-injection` | CWE-89 | String-concatenated SQL queries vs parameterized |
| `multi-crypto-failure` | CWE-327/328/330 | MD5, SHA1, DES, `Math.random()`, non-secure random |
| `multi-insecure-deser` | CWE-502 | `pickle.loads`, `readObject`, `Marshal.load`, `yaml.load` |
| `multi-ssrf` | CWE-918 | HTTP requests to user-controlled URLs |
| `multi-path-traversal` | CWE-22 | File operations with unsanitized path components |
| `security-pattern` | Various | Per-language security patterns (14 Python, 12 JS, 11 C#, 10 C++, 9 Java/Ruby, 7 Go/Swift, 4 Kotlin, 2 Rust) |
| `hardcoded-secret` | CWE-798 | Password assignments, API key constants, PEM keys in source |
| `secret-scan` | CWE-798 | Entropy-based secret detection (threshold 5.0 bits/char) |
| `path-normalize` | CWE-22 | File operations missing path canonicalization |
| `dep-audit` | CWE-1395 | Vulnerable dependencies via cargo-audit, pip-audit, npm audit, govulncheck, bundler-audit, dotnet, osv-scanner, swift-audit |

## Code Quality Detectors

| Detector | Languages | What it finds |
|----------|-----------|---------------|
| `panic-pattern` | C, Java, JS, Python, Ruby, Rust | `unwrap()`, `panic!`, `assert!`, bare `raise` |
| `blocking-io-in-async` | Rust, Python, JS | Synchronous I/O in async functions |
| `broad-exception` | Python, Java, JS | `except Exception`, `catch (Exception e)` |
| `swallowed-errors` | Python, Java, JS, Go | Empty catch blocks, ignored error returns |
| `error-context-loss` | Rust, Python, JS | `?` without `.context()`, bare `raise` |
| `mixed-bool-ops` | Rust, Python, JS, Java | `a and b or c` without parentheses |
| `string-concat-in-loop` | Rust, Python, JS | String building via `+=` in loops |
| `regex-in-loop` | Rust, Python, JS | `re.compile` / `Regex::new` inside loops |
| `discarded-async-result` | Rust, Python, JS | Awaitable called but result not awaited |
| `duplicated-fn` | Rust, Python, JS, Java | Same function name in multiple files |
| `process-exit-in-lib` | Rust, Python, JS, Java, Go | `sys.exit()` / `process::exit()` in library code |

## Concurrency Detectors

| Detector | Languages | What it finds |
|----------|-----------|---------------|
| `mutex-across-await` | Rust | `MutexGuard` held across `.await` point |
| `ffi-panic` | Rust | `panic!` in FFI-exported function |
| `unbounded-queue` | Rust, Python, Go | Channel/queue without capacity bound |
| `missing-async-timeout` | Rust, JS | Async operation without timeout |
| `zombie-subprocess` | Rust, Python | Spawned process without `wait()` |
| `missing-shutdown-handler` | Rust, Python | Server without graceful shutdown |
| `poisoned-mutex-recovery` | Rust | Lock recovery after poisoned mutex |
| `relaxed-atomics` | Rust | `Ordering::Relaxed` on shared state |

## Advanced / Research Detectors

| Detector | What it does |
|----------|-------------|
| `cegar` | Counter-Example Guided Abstraction Refinement |
| `dual-encoder` | Semantic similarity for code clone detection |
| `hagnn` | Graph Neural Network vulnerability detection |
| `spec-miner` | API specification mining from usage patterns |
| `data-transform-spec` | Data transformation specification validation |

## Compound Analyzers

| Analyzer | What it analyzes |
|----------|-----------------|
| `service-map` | HTTP dependencies, database connections |
| `dep-graph` | Dependency cycles, orphan nodes |
| `container-scan` | Dockerfile security |
| `config-drift` | .env differences across environments |
| `schema-check` | Dangerous SQL migrations |
| `mem-check` | Memory safety patterns |
| `cost-estimate` | Cloud cost drivers |
| `license-scan` | Dependency license compatibility |
| `blast-radius` | Change impact analysis |

## Architecture

All detectors implement `Detector` trait in `crates/apex-detect/src/detectors/`. Pure detectors run concurrently; subprocess detectors (dep-audit, static-analysis) use a semaphore (max 4).

See [CONTRIBUTING.md](../CONTRIBUTING.md) for how to add new detectors.
