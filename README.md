# APEX — Autonomous Path EXploration

[![CI](https://github.com/allexdav2/apex/actions/workflows/ci.yml/badge.svg)](https://github.com/allexdav2/apex/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

APEX is a **Claude Code-first** coverage exploration system. It drives any
repository toward 100% branch coverage by combining instrumentation, fuzzing,
concolic execution, symbolic solving, and AI-guided test synthesis — all
orchestrated through Claude Code agents and slash commands.

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

### Agents (auto-invoked)

These agents are triggered automatically by Claude Code when your message matches:

| Agent | Trigger examples |
|-------|-----------------|
| **apex-coverage-analyst** | "what's our coverage?", "which parts are uncovered?" |
| **apex-test-writer** | "write tests for X", "improve coverage in Y" |
| **apex-runner** | "run apex against Z", "run apex on itself" |

### Typical Workflow

```
You:    /apex-run
APEX:   Round 1/5 — Coverage: 62% → 71% (+9%)
        ████████████████████████████████░░░░░░░░░░░░ 71%
        +142 branches covered | 203 remaining | 8 tests written

        Round 2/5 — Coverage: 71% → 78% (+7%)
        ...

You:    /apex-gaps apex-fuzz
APEX:   15 uncovered regions in apex-fuzz:
        1. mutators.rs:45-52 — havoc mutation with empty input
           → Test: feed empty Vec<u8> to HavocMutator::mutate()
        ...
        Writing 8 tests could bring coverage from 78% to ~85%

You:    /apex-generate apex-fuzz
APEX:   Generated 8 tests in crates/apex-fuzz/src/mutators.rs
        cargo test -p apex-fuzz: 8 passed
        Coverage: 78% → 84%
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

APEX also works as a standalone CLI without Claude Code:

```bash
# Run against a Python project
apex run --target ./my-project --lang python

# CI coverage gate (fails if coverage < 80%)
apex ratchet --target ./my-project --lang python --threshold 0.8

# Check tool dependencies
apex doctor

# Security audit
apex audit --target ./my-project --lang python
```

## Features

- **Multi-language** — Python, JavaScript, Java, Rust, C, WebAssembly
- **Coverage-guided fuzzing** — MOpt-mutator scheduling with corpus management
- **Concolic execution** — concrete + symbolic hybrid exploration
- **Symbolic constraint solving** — SMT-LIB2 solver with optional Z3 backend
- **AI agent orchestration** — LLM-driven test generation and refinement
- **Bug detection** — panic pattern analysis, security auditing
- **CI integration** — `apex ratchet` fails builds when coverage drops
- **Sandboxed execution** — process isolation, shared-memory bitmaps, optional Firecracker microVMs

## Architecture

APEX is a Rust workspace with 14 crates:

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
| **apex-rpc** | gRPC distributed coordination (tonic) |
| **apex-mir** | Mid-level IR parsing and control-flow analysis |
| **apex-cli** | CLI binary — `run`, `ratchet`, `doctor`, `audit` subcommands |

```
                    ┌──────────┐
                    │ apex-cli │
                    └────┬─────┘
         ┌───────┬───────┼───────┬────────┐
         v       v       v       v        v
    apex-agent  apex-fuzz  apex-concolic  apex-detect
         │       │       │
         v       v       v
    apex-synth  apex-coverage  apex-symbolic
         │       │       │
         v       v       v
    apex-instrument  apex-sandbox  apex-mir
         │       │
         v       v
    apex-lang  apex-rpc
         │
         v
      apex-core
```

## Configuration

Copy `apex.example.toml` to your project root as `apex.toml`:

```toml
[coverage]
target = 1.0            # Coverage target (0.0–1.0)
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
