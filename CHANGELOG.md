# Changelog

All notable changes to APEX will be documented in this file.

## [Unreleased]

### Added
- **`apex perf`** — new CLI subcommand for performance test generation with modes: `--redos` (scan for catastrophic backtracking in regex), `--complexity` (empirical algorithmic complexity estimation), `--slo` (SLO verification against latency/input-size bounds), and default resource-guided fuzzing (PerfFuzz approach)
- **ReDoS detector** (`redos`) — static analysis of regular expressions for nested quantifiers and overlapping alternatives that cause exponential backtracking; emits CWE-1333 / CWE-400 findings with concrete worst-case pump strings
- **Algorithmic complexity detector** (`algorithmic-complexity`) — identifies nested loops over same collection (O(n²)), recursive functions without memoization (O(2^n)), and quadratic list building patterns; emits CWE-400 findings
- **Hash collision risk detector** (`hash-collision-risk`) — flags user-controlled input used as hash map keys in request handlers, enabling hash collision DoS (Crosby & Wallach 2003); emits CWE-400 findings
- **`PerfFuzzStrategy`** — resource-guided fuzzing that maximizes wall-clock time, peak memory, instruction count, or per-edge execution frequency instead of branch coverage (PerfFuzz ISSTA 2018 approach)
- **`PerfFeedback`** — multi-dimensional performance feedback tracker with per-edge execution count maximization
- **`ComplexityEstimator`** — empirical complexity estimation from execution traces using least-squares curve fitting to O(1), O(log n), O(n), O(n log n), O(n²), O(n³), O(2^n) models (Goldsmith et al. 2007)
- **`ResourceMetrics`** type — captures wall-clock time, CPU time, peak memory, allocation count, instruction count, and per-edge execution counts from test execution
- **`ComplexityClass`** / **`ComplexityEstimate`** types — represent asymptotic complexity classifications with confidence scores
- **`FindingCategory::PerformanceRisk`** — new finding category for performance-class defects
- **`Evidence::PerformanceProfile`** — new evidence variant with function, metric, baseline/measured values, and worst-case input description
- **Performance synthesis prompts** — LLM prompt strategies for worst-case input generation, ReDoS proof-of-concept tests, and SLO verification tests with per-language timing instrumentation
- **Regex extraction** — structured multi-language regex pattern extraction from source code (Python, JavaScript, Rust, Go, Java, Ruby) for the ReDoS analysis pipeline
- **Resource measurement in sandboxes** — `ProcessSandbox` now populates `ExecutionResult.resource_metrics` via `getrusage(RUSAGE_CHILDREN)` (wall time, CPU time, peak RSS)

### Fixed
- **Claude Code plugin install** — repo was missing top-level `commands/` directory, so `claude plugin install` found zero slash commands; added `commands/` with all 9 APEX commands (`/apex`, `/apex-run`, `/apex-index`, etc.) and promoted agent `.md` files from `agents/agents/` to `agents/` root for auto-dispatch; added `.claude-plugin/plugin.json` manifest so the plugin registry picks up name, version, and metadata

## [0.5.0] — 2026-03-24

### Added
- **`apex init`** — new CLI subcommand that detects project language, runs `probe_all()`, saves `.apex/environment.json`, and generates `apex.toml` if absent; supports `--lang` override and `--dry-run`
- **Auto-probe on `apex run`** — environment probe is loaded or refreshed (7-day freshness window) automatically before the coverage pipeline runs; result logged as a structured `info!` line
- **`apex_lang::probe_impl`** — new module with `EnvironmentProbe`, `probe_all()`, `save_cache()`, `load_cached()`, and language-specific sub-probes for Python, Rust, Node, Go, and Java
- **`apex doctor` environment section** — shows cached probe summary (Python version, venv, package manager, PEP 668 flag) at the end of doctor output; suggests `apex init` when no cache exists
- **`detect_language()`** — heuristic language detection from project marker files (`Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`, `pom.xml`, `CMakeLists.txt`)

### Fixed
- **CWD bug in analyze pipeline** — `parse_llvm_cov_export` now canonicalizes both the target root and coverage filenames before `strip_prefix`, fixing symlink mismatches (e.g. `/tmp` vs `/private/tmp` on macOS) that caused "0 source files" and "could not find Cargo.toml" when analyzing out-of-tree targets
- **`apex doctor`** — `CommandSpec` working directory changed from `"."` to `std::env::current_dir()` so version checks don't depend on inherited CWD
- **Wasm instrumentor** — fallback working directory for `wasm-opt` changed from `"."` to the target root

## [0.4.0] — 2026-03-21

### Added
- **`--output-format sarif`** — GitHub Security tab integration; SARIF 2.1.0 output for `apex audit` and `apex analyze` commands
- **`--output-format markdown`** — PR comment generation with severity summary tables and findings details
- **`--output-format lcov`** — Coverage Gutters / Codecov integration; LCOV export from branch coverage data
- **`--changed-files`** — incremental CI analysis for `test-prioritize`, `risk`, and security commands
- **`apex badge`** — SVG coverage badge generation with color tiers (red/orange/yellow/green/brightgreen)
- **`apex ci-report`** — base vs head finding comparison with new/resolved/unchanged classification
- **`apex wrap`** — inject coverage into user's test command (10 languages)
- **GitHub Action scaffold** (`.github/actions/apex/action.yml`)
- **Tree-sitter Python CPG builder** (feature-gated)
- **Inter-procedural taint analysis** via TaintSummary
- **O(1) CPG graph lookup** (HashMap storage)
- **Compound Coverage Oracle** (Bayesian signal combination)
- **LLM concolic solver** (ConcoLLMic, third solver in portfolio)
- **Mutation-guided test generation** (Meta ACH approach)
- **Type-state analysis** (CWE-416/675/404)
- **Coverage-informed severity re-scoring**
- **CSRF detector** (CWE-352)
- **XSS detector upgrade** (CWE-79 — template injection, DOM XSS)
- **File upload detector** (CWE-434)
- **Information exposure detector** (CWE-200)
- **Frida binary coverage stub** (feature-gated)
- **Multi-language CPG dispatch** — `apex-cli` now dispatches `JsCpgBuilder` and `GoCpgBuilder` for JavaScript and Go targets alongside the existing Python builder; all 5 CPG build sites refactored through a new `build_cpg_for_lang` helper using the `CpgBuilder` trait
- **`JsCpgBuilder` and `GoCpgBuilder`** — new line-based CPG builders in `apex-cpg` for JavaScript and Go; exported from the crate alongside `PythonCpgBuilder`
- **`Cpg` now derives `Clone`** — enables Arc unwrap-or-clone in the data-flow command
- **E2E taint detection test** — `test_audit_detects_taint_flow` in `integration_harness.rs` verifies CWE-78 taint flow from `sys.argv` through `subprocess.call(shell=True)` is detected and not suppressed as noisy
- **`tainted.py` fixture** — `tests/fixtures/tiny-python/tainted.py` provides a concrete CWE-78 taint source for E2E verification
- **v0.4.0 integration test suite** — 11 tests covering SARIF output, markdown output, badge SVG, changed-files filtering, CI report comparison, and LCOV export

## [0.3.1] — 2026-03-20

## [0.3.0] — 2026-03-20

## [0.3.0] — 2026-03-18

### Added
- **Concurrent subprocess detectors** — `DetectorPipeline::run_all` now runs subprocess detectors (e.g. dep-audit) concurrently with a semaphore (max 4) instead of sequentially, reducing audit wall time when multiple subprocess detectors are enabled
- **HUNT+INTEL integration (`hunt_hints` module)** — `apex-detect` now exposes `HuntHints`, `HuntHintConfig`, and `AnalysisReport::hunt_hints()` to convert security findings into priority boosts for the hunt phase; uncovered branches within a configurable line window of a finding receive a severity-scaled boost so the orchestrator explores security-adjacent code first
- **`apex integrate` subcommand** — auto-writes MCP server config for Claude Code, Cursor, and Windsurf with --editor, --global, --dry-run flags and config merging
- **33 total MCP tools** — full CLI coverage via MCP protocol (was 6, added 27: complexity, dead-code, risk, hotpaths, test-optimize, test-prioritize, blast-radius, secret-scan, data-flow, diff, regression-check, lint, flaky-detect, contracts, attack-surface, verify-boundaries, features, index, docs, license-scan, flag-hygiene, api-diff, compliance-export, api-coverage, service-map, schema-check, test-data)
- **Scripted test harness** — `ScriptedSandbox` and `ScriptedStrategy` mocks for deterministic orchestrator loop testing
- **7 orchestrator loop tests** covering all exit conditions
- **UDS RPC test infrastructure** — worker tests converted from TCP to Unix domain sockets
- **Fixture project integration tests** — tiny Python project for end-to-end CLI pipeline tests
- **Dependency audit for C# (.NET)** — `dotnet list package --vulnerable --include-transitive` parser with tabular output support; graceful fallback when `dotnet` is absent
- **Dependency audit for Ruby** — `bundler-audit check` parser with block-format (Name/Version/Advisory/Criticality/Title/Solution); graceful fallback when `bundler-audit` is absent
- **Dependency audit for Swift** — `swift-audit check` stub; reports Info finding when tool is not installed
- **Dependency audit for C/C++** — `osv-scanner` integration for lockfile scanning; reports Info finding when tool is not installed
- **Exhaustive language match** in `DependencyAuditDetector::analyze()` — replaces wildcard arm with explicit arms for all 12 languages; remaining unsupported languages (Java, Kotlin, Go, Wasm) return empty
- **+28 tests** for new dep audit parsers and language dispatch
- **Test synthesis for 8 languages** — Go (`go test`), C++ (Google Test), C (`assert.h`), C# (xUnit), Swift (XCTest), Kotlin (JUnit5), Ruby (RSpec), WASM (Jest wrapper); all via `TestSynthesizer` trait with dedup, chunking, and hash-named output files
- **+58 tests** for test synthesis expansion
- **Fuzzing harness generators** — C# (SharpFuzz `Fuzzer.Run`) and Swift (libFuzzer `@_cdecl("LLVMFuzzerTestOneInput")`) harness code generation
- **+12 tests** for fuzz harness generators
- **Ruby test sandbox** — `RubyTestSandbox` with SimpleCov JSON parser for coverage-guided execution
- **Kotlin per-test indexer** — JaCoCo-based per-test branch indexing via Gradle
- **+25 tests** for Ruby sandbox and Kotlin indexer
- **Concolic condition parsers for 7 languages** — Rust (`if`/`match`/`if let`/`.is_some()`), Go (`err != nil`/`switch`/`len()`), Java/Kotlin (`instanceof`/`.equals()`), C# (`is Type`/`?.`/`??`), Swift (`if let`/`guard let`), C/C++ (`ptr != NULL`/`flags & MASK`), Ruby (`.nil?`/`unless`/`case/when`); all parse into shared `ConditionTree` IR
- **Boundary seed generator** — `boundary_values(ConditionTree)` generates concrete values near decision boundaries for concolic execution
- **`StaticConcolicStrategy`** — language-agnostic `Strategy` impl that accepts pluggable condition parsers
- **+53 tests** for concolic expansion
- **README marketing redesign** — new headline, hero SVG (18s, no scroll), Quick Start, comparison table, language support matrix, CI/CD example

## [0.2.1] — 2026-03-16

### Fixed
- **CWE-88 argument injection** — validate git refs in `apex diff --base` to reject flag-like values starting with `-` or containing `..`
- **Path traversal in MCP handlers** — canonicalize `params.target` in all MCP tool endpoints before subprocess dispatch
- **Output path validation** — canonicalize `--output` paths in `audit`, `docs`, and `compliance` subcommands
- **Secret-scan false positives** — suppress high-entropy string matches in instrumentation templates, detector source files, and `const` string declarations
- **Dependency-audit graceful fallback** — return info-level finding instead of error when `cargo-audit`/`pip-audit`/`npm audit` are not installed

### Added
- **+132 hunt tests** across 5 crates (apex-detect, apex-index, apex-instrument, apex-agent, apex-cli) raising line coverage 92.7% → 93.0%

### Bugs Found
- `schema_check.rs` — `safe_count` field permanently 0 (dead field, never populated)
- `dep_graph.rs` — DFS cycle detection misses cycles via previously-visited nodes
- `csharp.rs` indexer — compact/minified XML silently drops all branches
- `csharp.rs` instrumenter — class context leak on same-line `</class>` tags

## [0.2.0] — 2026-03-16

### Added

#### SDLC Intelligence Platform
- 10 intelligence subcommands: `deploy-score`, `test-optimize`, `dead-code`, `hotpaths`, `risk`, `test-prioritize`, `test-impact`, `contracts`, `regression-check`, `verify-boundaries`
- `apex features` — per-language feature matrix showing instrumentation, concolic, and analysis support
- Rust per-test branch indexer — APEX can now index itself with `apex index`
- `/apex` unified dashboard command showing deploy score, coverage, and recommended actions

#### Security Analysis
- Security-pattern detector with Rust + Python patterns and CWE ID mapping
- Hardcoded-secret detector with regex patterns and false-positive filtering
- Secret scan, license scan, feature-flag hygiene, and API diff detectors
- SARIF output format with CWE mapping for all finding categories
- CVSS scoring for security findings
- SBOM generation, SCA dependency audit (Python pip, JS npm, Rust cargo)
- Session security, missing-timeout, and broken-access detectors
- STRIDE, ASVS, and SSDF framework integration into audit pipeline
- Threat-model-aware detection to suppress false positives based on trust boundaries
- `/apex-threat-model` interactive wizard for configuring trust classification

#### Analyzers (30 new)
- `dep-graph` — dependency graph with cycle detection and fan-in/fan-out metrics
- `doc-coverage` — documentation coverage measurement
- `runbook-check` — operational runbook validation
- `slo-check` — SLO/SLA compliance verification
- `perf-diff` — performance regression detection
- `a11y-scan` — accessibility compliance checking
- `bench-diff` — benchmark comparison analysis
- `config-drift` — configuration drift detection
- `iac-scan` — infrastructure-as-code security scanning
- `container-scan` — container image vulnerability scanning
- `mem-check` — memory usage analysis
- `resource-profile` — resource utilization profiling
- `i18n-check` — internationalization completeness checking
- `trace-analysis` — distributed trace analysis
- `cost-estimate` — cloud cost estimation
- `incident-match` — incident pattern matching
- `migration-check` — migration safety validation
- `api-coverage` — API endpoint coverage measurement
- `service-map` — service dependency mapping
- `schema-check` — schema validation
- `test-data` — test data quality analysis
- `data-flow` — data flow tracking
- `blast-radius` — change impact analysis
- `compliance-export` — compliance report generation
- `data-transform` — data transformation validation
- 8 Rust self-analysis detectors (64 tests)
- Bandit-compatible rule detector for Python

#### Concolic Execution
- JS/TS condition parser for concolic execution with `ConditionTree` shared IR
- Extended Python concolic parser: string ops, `isinstance`, `in`, `len` comparisons
- Bun runtime detection in JS runner

#### Code Property Graph
- New `apex-cpg` crate with taint analysis (inspired by Joern)
- Sanitizer-aware taint propagation with proper blocking

#### Reverse Path Analysis
- New `apex-reach` crate — traces from uncovered regions back to entry points
- Python and JavaScript extractors for import/export graphs

#### Solver Upgrades
- Gradient descent constraint solver (from Angora research)
- Continuous branch distance heuristics (from EvoMaster/Korel research)
- Priority-based target selection with solver cache
- Portfolio solver wires gradient descent as first backend

#### Test Synthesis
- LLM-guided test refinement with CoverUp-style closed loop

#### JS/TS Support
- 5-stage JS instrumentor pipeline with V8 + Istanbul tool selection
- V8 coverage parser with `OffsetIndex` for precise mapping
- Source map remapping for TypeScript coverage
- JS/TS index support — Istanbul + V8 coverage parsing (36 tests)
- JS environment detection in `apex-lang`

#### CLI
- `apex secret-scan`, `license-scan`, `flag-hygiene`, `api-diff` subcommands
- `apex data-flow`, `blast-radius`, `compliance-export` subcommands
- `apex api-coverage`, `service-map`, `schema-check`, `test-data` subcommands

#### Distribution
- GitHub Releases with cross-compilation for 4 targets (linux/mac x amd64/arm64)
- Homebrew formula: `brew install sahajamoth/tap/apex`
- npm wrapper: `npx @apex-coverage/cli run`
- pip wrapper: `pipx install apex-coverage`
- Nix flake: `nix run github:sahajamoth/apex`
- curl installer: `curl -sSL .../install.sh | sh`

#### Infrastructure
- Fleet meta-agent system with 6 crew agents and 5 officers
- `AgentCluster` orchestrator wired into `apex run` as unified entrypoint
- `FixtureRunner` for deterministic integration testing
- Portable agents/commands via `$APEX_HOME`

### Changed
- `apex run` shifted from coverage-chasing to bug-hunting strategy
- Workspace expanded from 14 to 16 crates (added `apex-cpg`, `apex-reach`)
- CPG build integrated into `run_audit` pipeline
- Test file exclusion from branch coverage measurement

### Fixed
- 35 bugs found and fixed across 2 bug-hunting rounds (16 + 19)
- Boundary seed overflow: `val + 1` → `saturating_add(1)` in concolic engine
- `>=`/`<=` operators generated wrong boundary values in concolic seeds
- Concolic errors silently swallowed as `Ok(vec![])` — now propagated as `Err`
- Empty module/func generated invalid Python in concolic test synthesis
- Mutex poison panics across 4 crates → `unwrap_or_else(|e| e.into_inner())`
- Heroku UUID regex matched all UUIDs in secret scanner — added context prefix
- `PATTERN_META[idx]` potential panic → safe `.get(idx)` with bounds check
- SQL injection regexes recompiled per call → `static LazyLock<Regex>`
- Circular `$ref` stack overflow in API diff → visited-set cycle detection
- `branch_key` sentinel collision: `None` vs `Some(255)` → string-based keys
- `build_profiles` double-counted tests → `HashSet` deduplication
- `extract_functions` false positives for Java/JS/Python → tighter patterns
- Generic type params included in function names → truncate at `<`
- Istanbul `i as u8` wraps for >255 arms → overflow guard
- Source map `sourceRoot` double-joined with token paths
- Inline base64 source map newline corruption
- `.mjs` sidecar source map path resolution
- Arrow function `=>` misidentified as comparison operator in JS parser
- Self-loop cycle detection in dependency graph
- Fan-in/fan-out missed zero-degree nodes
- SPDX license expression case normalization (`OR`/`Or`/`or`)
- BOM stripping in license file parsing
- Float count parsing and non-bool condition filtering in Rust indexer
- 41 regexes converted from per-call `Regex::new` to `static LazyLock` across 7 crates
- Dead detectors (`MissingTimeoutDetector`, `SessionSecurityDetector`) wired into pipeline
- Z3 solver timeout added to prevent hangs

### Security
- Hardcoded secret detection skips `#[cfg(test)]` blocks to reduce false positives
- Threat model suppression integrated into `SecurityPatternDetector`
- Trust classification tables for internal/external/admin boundaries

## [0.1.0] — 2026-03-12

Initial release.

### Core Infrastructure
- Workspace with 14 crates and shared dependency management
- `ApexConfig` with TOML configuration loading and defaults
- Unified error handling with `ApexError` and `thiserror`
- Async trait-based `Instrumentor` and `LanguageRunner` abstractions

### Coverage
- `CoverageOracle` with bitmap-based edge coverage tracking
- Delta coverage computation for incremental analysis
- Shared-memory bitmap support for cross-process coverage

### Instrumentation
- Python: AST-based branch probe injection
- JavaScript: Istanbul-compatible instrumentation
- Java: bytecode instrumentation
- Rust: `cargo-llvm-cov` integration + `sancov` runtime
- Optional LLVM IR instrumentation (feature: `llvm-instrument`)
- Optional WebAssembly instrumentation (feature: `wasm-instrument`)

### Language Runners
- Test runners for Python (pytest), JavaScript (Jest/Node), Java (JUnit), Rust (cargo test), C (gcc), WebAssembly

### Sandbox
- Process-based sandbox with timeout and resource limits
- Shared-memory bitmap for coverage collection
- Optional Firecracker microVM isolation (feature: `firecracker`)

### Fuzzing
- Coverage-guided fuzzer with MOpt mutator scheduling
- Corpus management with LRU eviction
- Grammar-aware mutation, CmpLog feedback, directed fuzzing
- Optional LibAFL backend (feature: `libafl-backend`)

### Symbolic & Concolic
- SMT-LIB2 constraint solver with caching
- Portfolio solver strategy with bounded model checking
- Optional Z3 integration (feature: `z3-solver`)
- Optional Kani prover (feature: `kani-prover`)
- Python concolic execution engine with taint tracking
- Optional pyo3 tracer extension (feature: `pyo3-tracer`)

### AI Agent
- Multi-agent orchestration with ensemble strategies
- Source-context-aware test generation and refinement
- Bug ledger for tracking discovered issues
- Driller integration for coverage-guided exploration

### Test Synthesis
- Tera template-based test generation
- Synthesizers for pytest, Jest, JUnit, cargo-test

### Bug Detection
- Panic pattern detector for Rust code
- Security audit pipeline with configurable detectors
- Finding categorization and severity classification

### CLI
- `apex run` — full autonomous coverage pipeline
- `apex ratchet` — CI coverage gate with configurable threshold
- `apex doctor` — external tool dependency checker
- `apex audit` — security and bug detection analysis

### Infrastructure
- gRPC distributed coordination service (tonic/prost)
- MIR parsing and control-flow graph analysis
