# Changelog

All notable changes to APEX will be documented in this file.

## [Unreleased]

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
- Homebrew formula: `brew install allexdav2/tap/apex`
- npm wrapper: `npx @apex-coverage/cli run`
- pip wrapper: `pipx install apex-coverage`
- Nix flake: `nix run github:allexdav2/apex`
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
