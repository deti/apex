# Changelog

All notable changes to APEX will be documented in this file.

## [Unreleased]

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
