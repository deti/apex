# APEX — Autonomous Path EXploration

[![CI](https://github.com/allexdav2/apex/actions/workflows/ci.yml/badge.svg)](https://github.com/allexdav2/apex/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

APEX is a **Claude Code-first** coverage exploration and SDLC intelligence
platform. It drives any repository toward 100% branch coverage by combining
instrumentation, fuzzing, concolic execution, symbolic solving, and AI-guided
test synthesis — then uses the resulting per-test branch data to power 16
intelligence commands across testing, security, documentation, and CI/CD.

While APEX includes a standalone CLI, the primary interface is Claude Code.
The agents analyze your codebase, identify coverage gaps, write tests,
select strategies, and iterate autonomously inside your editor.

## Using APEX with Claude Code

### Install

Clone the repo and install the agents into your project:

```bash
git clone https://github.com/allexdav2/apex.git
cd apex
cargo build --release

# Install agents and commands into your project's .claude/ directory
./agents/install.sh
```

### Prerequisites

```bash
cargo install cargo-llvm-cov

# On macOS with Homebrew LLVM:
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
```

### Slash Commands

Run these directly in Claude Code:

| Command | What it does |
|---------|-------------|
| `/apex-run` | Full autonomous coverage loop — measures gaps, writes tests, selects strategies, re-measures |
| `/apex-run /path/to/project python 5 0.95` | Run against a specific project with language, rounds, and target |
| `/apex-status` | Show coverage table for the workspace |
| `/apex-status apex-fuzz` | Coverage for a specific crate |
| `/apex-gaps` | List the top uncovered regions with explanations and suggested tests |
| `/apex-gaps apex-coverage` | Gaps in a specific crate |
| `/apex-generate apex-fuzz` | Generate tests targeting uncovered branches in a crate |
| `/apex-ci 0.8` | CI coverage gate — fails if below threshold |
| `/apex-index` | Build per-test branch index for intelligence commands |
| `/apex-intel` | Full SDLC intelligence report — test quality, risk, dead code, hotpaths, contracts |
| `/apex-deploy` | Deployment readiness check — aggregate confidence score with GO/CAUTION/BLOCK recommendation |

### Agents (auto-invoked)

These agents are triggered automatically by Claude Code when your message matches:

| Agent | Trigger examples |
|-------|-----------------|
| **apex-coverage-analyst** | "what's our coverage?", "which parts are uncovered?" |
| **apex-test-writer** | "write tests for X", "improve coverage in Y" |
| **apex-runner** | "run apex against Z", "run apex on itself" |
| **apex-sdlc-analyst** | "what's our deploy score?", "find flaky tests", "show hot paths" |

### Typical Workflow

```
You:    /apex-run
APEX:   Round 1/5 — Coverage: 62% -> 71% (+9%)
        +142 branches covered | 203 remaining | 8 tests written

You:    /apex-index --target . --lang python
APEX:   Index built: 312 tests, 1847 branches, 89.2% coverage
        Saved to .apex/index.json

You:    /apex-intel
APEX:   Test Optimization: 312 -> 94 tests (3.3x speedup)
        Dead Code: 23 branches in 4 files never executed
        Flaky Tests: 2 tests show nondeterministic branch paths
        Hot Paths: src/auth.py:45 accounts for 12.3% of all branch hits
        Contracts: 8 invariants discovered (always-taken branches)
        Risk: LOW — 95% coverage of changed files

You:    /apex-deploy
APEX:   Deploy Score: 87/100 — GO
        Coverage: 89.2% -> 27/30
        Test quality: 68% unique coverage -> 17/25
        Detectors: 0 findings -> 25/25
        Stability: index present -> 20/20
```

### Strategy Selection

The `/apex-run` agent loop automatically selects the best strategy per gap:

| Target | Primary strategy | Fallback |
|--------|-----------------|----------|
| Rust workspace | Source-level tests | fuzz (if binary harness exists) |
| Python project | Source-level tests | concolic (for constraint paths) |
| C/Rust binary | fuzz | driller (when fuzz stalls) |
| JavaScript | Source-level tests | — |

## Standalone CLI

APEX has 20 subcommands organized in 6 packs:

### Core Commands

```bash
# Run against a Python project
apex run --target ./my-project --lang python

# CI coverage gate (fails if coverage < 80%)
apex ratchet --target ./my-project --lang python --min-coverage 0.8

# Check tool dependencies
apex doctor

# Security audit
apex audit --target ./my-project --lang python
```

### Pack A: Foundation — Per-Test Branch Index

```bash
# Build the index (required before intelligence commands)
apex index --target ./my-project --lang python --parallel 8
```

Runs each test individually under coverage instrumentation, building a persistent
map of which tests exercise which branches. Stored in `.apex/index.json`.

### Pack B: Test Intelligence

```bash
# Find minimal test subset maintaining coverage
apex test-optimize --target .

# Order tests by relevance to changed files
apex test-prioritize --target . --changed-files src/auth.py,src/api.py

# Detect flaky tests via execution path divergence
apex flaky-detect --target . --lang python --runs 5
```

### Pack C: Source Intelligence

```bash
# Find semantically dead code
apex dead-code --target .

# Runtime-prioritized lint findings
apex lint --target . --lang python

# Exercised vs static complexity per function
apex complexity --target .
```

### Pack D: Behavioral Analysis & CI/CD

```bash
# Behavioral diff between current and base branch
apex diff --target . --lang python --base main

# CI gate for unexpected behavioral changes
apex regression-check --target . --lang python --base main --allow flaky_test

# Assess risk of changed files
apex risk --target . --changed-files src/auth.py

# Rank branches by execution frequency
apex hotpaths --target . --top 20

# Discover invariants from branch execution patterns
apex contracts --target .

# Aggregate deployment confidence score (0-100)
apex deploy-score --target . --detector-findings 0 --critical-findings 0
```

### Pack E: Documentation

```bash
# Generate behavioral documentation from execution traces
apex docs --target . --output docs/behavioral.md
```

### Pack F: Security Analysis

```bash
# Map attack surface from entry-point reachability
apex attack-surface --target . --lang python --entry-pattern test_api

# Verify all entry-point paths pass through auth checks
apex verify-boundaries --target . --lang python \
  --entry-pattern test_api --auth-checks check_auth --strict
```

## Features

- **Multi-language** — Python, JavaScript, Java, Rust, C, WebAssembly
- **Coverage-guided fuzzing** — MOpt-mutator scheduling with corpus management
- **Concolic execution** — concrete + symbolic hybrid exploration
- **Symbolic constraint solving** — SMT-LIB2 solver with optional Z3 backend
- **AI agent orchestration** — LLM-driven test generation and refinement
- **Bug detection** — panic patterns, security auditing, hardcoded secrets
- **SDLC intelligence** — 16 commands powered by per-test branch indexing
- **CI integration** — `ratchet`, `regression-check`, `deploy-score` for CI gates
- **Sandboxed execution** — process isolation, shared-memory bitmaps, optional Firecracker microVMs

## Architecture

APEX is a Rust workspace with 15 crates:

| Crate | Description |
|-------|-------------|
| **apex-core** | Shared types, traits, configuration, error handling |
| **apex-coverage** | Coverage oracle, bitmap management, delta tracking |
| **apex-instrument** | Multi-language instrumentation (Python, JS, Java, Rust, LLVM, WASM) |
| **apex-lang** | Language-specific test runners |
| **apex-sandbox** | Process/WASM/Firecracker sandbox execution, shared-memory bitmaps |
| **apex-agent** | AI agent orchestration, report generation, refinement loops |
| **apex-synth** | Test synthesis via Tera templates (pytest, Jest, JUnit, cargo-test) |
| **apex-symbolic** | Symbolic constraint solving (SMT-LIB2, optional Z3, Kani) |
| **apex-concolic** | Concolic execution engine (optional pyo3 tracer) |
| **apex-fuzz** | Coverage-guided fuzzing with MOpt scheduling (optional libafl) |
| **apex-detect** | Bug detection pipeline (panic patterns, security checks) |
| **apex-index** | Per-test branch indexing, SDLC intelligence analysis |
| **apex-rpc** | gRPC distributed coordination (tonic) |
| **apex-mir** | Mid-level IR parsing and control-flow analysis |
| **apex-cli** | CLI binary — 20 subcommands across 6 intelligence packs |

```
                    +----------+
                    | apex-cli |
                    +----+-----+
         +-------+-------+-------+--------+--------+
         v       v       v       v        v        v
    apex-agent  apex-fuzz  apex-concolic  apex-detect  apex-index
         |       |       |                              |
         v       v       v                              v
    apex-synth  apex-coverage  apex-symbolic      (analysis engine)
         |       |       |
         v       v       v
    apex-instrument  apex-sandbox  apex-mir
         |       |
         v       v
    apex-lang  apex-rpc
         |
         v
      apex-core
```

## Configuration

Copy `apex.example.toml` to your project root as `apex.toml`:

```toml
[coverage]
target = 1.0            # Coverage target (0.0-1.0)
min_ratchet = 0.8       # Minimum for CI gate

[fuzz]
corpus_max = 10000
mutations_per_input = 8
stall_iterations = 50

[agent]
max_rounds = 3

[sandbox]
process_timeout_ms = 10000
```

See [`apex.example.toml`](apex.example.toml) for all options.

## Optional Feature Flags

Heavy dependencies are behind feature flags and not compiled by default:

| Feature | Crate | Enables |
|---------|-------|---------|
| `llvm-instrument` | apex-instrument | LLVM-based instrumentation via inkwell |
| `wasm-instrument` | apex-instrument, apex-lang | WebAssembly instrumentation |
| `z3-solver` | apex-symbolic | Z3 SMT solver integration |
| `kani-prover` | apex-symbolic | Kani bounded model checking |
| `pyo3-tracer` | apex-concolic | Python concolic tracer extension |
| `libafl-backend` | apex-fuzz | LibAFL fuzzer backend |
| `firecracker` | apex-sandbox | Firecracker microVM isolation |

Enable with:

```bash
cargo build --release --features "apex-symbolic/z3-solver,apex-fuzz/libafl-backend"
```

## Development

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p apex-core

# Check formatting and lints
cargo fmt --check
cargo clippy --workspace -- -D warnings

# Build docs
cargo doc --workspace --no-deps --open
```

## License

[MIT](LICENSE)
