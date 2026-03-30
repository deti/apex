# APEX — Autonomous Path EXploration

[![CI](https://github.com/sahajamoth/apex/actions/workflows/ci.yml/badge.svg)](https://github.com/sahajamoth/apex/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/sahajamoth/apex?label=release)](https://github.com/sahajamoth/apex/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![0 crashes](https://img.shields.io/badge/crashes-0_across_10_repos-green)](docs/real-world-validation-summary.md)
[![Validated](https://img.shields.io/badge/validated-Linux_%7C_K8s_%7C_CPython-blue)](docs/real-world-validation-summary.md)
[![6600+ tests](https://img.shields.io/badge/tests-6600%2B_passing-green)](https://github.com/sahajamoth/apex/actions/workflows/ci.yml)
[![63 detectors](https://img.shields.io/badge/detectors-63-blue)](docs/DETECTORS.md)
[![Claude Code Plugin](https://img.shields.io/badge/Claude_Code-plugin-blueviolet)](https://claude.com/claude-code)

**Find vulnerabilities. Fix coverage gaps. Automatically.**

APEX is a Claude Code plugin that scans your codebase for security gaps, dead code,
and untested branches — then writes the tests to fix them. 63 detectors, 11 languages,
zero config. Works as both a CLI tool and a set of AI agents inside Claude Code.

> **Validated against:** Linux kernel · Kubernetes · CPython · TypeScript compiler ·
> ripgrep · Spring Boot · .NET Runtime · Vapor · Rails · ktor
>
> Found a hardcoded EC private key in Kubernetes (CWE-798).
> Scanned the Linux kernel in 4 minutes. 0 crashes across 12,656 findings.

<p align="center">
  <img src="docs/assets/real-world-validation.svg" alt="APEX real-world validation results" width="780">
</p>

[Full validation report →](docs/real-world-validation-summary.md)

---

## Quick Start

### 1. Install the binary

```bash
# macOS / Linux (auto-detects platform, no sudo needed)
curl -LsSf https://github.com/sahajamoth/apex/releases/latest/download/apex-cli-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sahajamoth/apex/releases/latest/download/apex-cli-installer.ps1 | iex"
```

<details>
<summary><strong>Alternative install methods</strong></summary>

```bash
# From source (needs Rust toolchain, ~5 min)
cargo install --git https://github.com/sahajamoth/apex

# Nix
nix run github:sahajamoth/apex
```

Package registry publishing (npm, Homebrew, pip) coming soon.

</details>

### 2. Install the Claude Code plugin

```bash
# Add the APEX marketplace (GitHub repo)
claude plugins marketplace add sahajamoth/apex

# Install the APEX plugin from it
claude plugins install apex@apex
```

Or from a local clone:

```bash
git clone https://github.com/sahajamoth/apex.git
claude plugins marketplace add ./apex
claude plugins install apex@apex
```

### 3. Run

```
# In Claude Code:
/apex init      # Auto-detect language, venv, toolchain
/apex           # Full analysis: coverage + security + intelligence
/apex detect    # Security scan (63 detectors, 40+ CWEs)
/apex hunt      # Bug hunting in uncovered code
/apex deploy    # Deploy readiness score
```

APEX agents detect your environment, install missing tools
(via uv, bun, mise), run coverage, write tests, and produce reports.

> **Not using Claude Code?** See [Standalone Installation](docs/STANDALONE.md)
> for CLI binary, GitHub Actions, and CI/CD setup.

---

## What APEX Finds in Real Projects

> The output below is from Claude Code running the `/apex` command.
> APEX agents orchestrate the full analysis cycle automatically.

```
> /apex

  ╭──────────────────────────────────────────────────╮
  │  APEX — Autonomous Path EXploration              │
  │  Target: ./your-project  (Python, 847 branches)  │
  ╰──────────────────────────────────────────────────╯

  Round 1/5 ─────────────────────────────────────────

  Coverage: 62% → 71% (+9%)
  +142 branches covered | 203 remaining | 8 tests written
```

```
  Round 5/5 ─────────────────────────────────────────

  Coverage: 71% → 94% (+23%)
  Final: 798/847 branches covered
  Tests written: 31 new tests across 6 files
```

Then ask Claude for intelligence:

```
> /apex intel

  ┌─ Test Optimization ──────────────────────────────┐
  │  312 tests → 94 minimal set (3.3× speedup)       │
  │  218 tests are redundant — same branch coverage   │
  └──────────────────────────────────────────────────┘

  ┌─ Dead Code ──────────────────────────────────────┐
  │  23 branches in 4 files — never executed by any   │
  │  test or production path                          │
  │                                                   │
  │  src/billing.py:89   unreachable after refactor   │
  │  src/export.py:34    legacy XML path, 0 callers   │
  │  src/api.py:201      dead error handler           │
  └──────────────────────────────────────────────────┘

  ┌─ Flaky Tests ────────────────────────────────────┐
  │  2 tests show nondeterministic branch paths       │
  │                                                   │
  │  test_concurrent_upload — race in file locking    │
  │  test_session_timeout  — depends on wall clock    │
  └──────────────────────────────────────────────────┘

  ┌─ Security ───────────────────────────────────────┐
  │  src/auth.py:67  — auth bypass: no token check    │
  │  on admin endpoint (reachable from test_api)      │
  │                                                   │
  │  src/config.py:12 — hardcoded secret:             │
  │  AWS_KEY = "AKIA..." (not from env)               │
  └──────────────────────────────────────────────────┘

  ┌─ Hot Paths ──────────────────────────────────────┐
  │  src/auth.py:45  — 12.3% of all branch hits      │
  │  src/db.py:112   — 8.7% of all branch hits       │
  │  These functions need the most test coverage.     │
  └──────────────────────────────────────────────────┘

  Deploy Score: 87/100 — GO
```

---

## Why APEX?

| | APEX | Semgrep | CodeQL | Snyk | coverage.py |
|---|:---:|:---:|:---:|:---:|:---:|
| Claude Code integration | **native** | — | — | — | — |
| AI agents (hunt, plan, fix) | ✓ | — | — | — | — |
| Auto-writes tests | ✓ | — | — | — | — |
| 63 detectors, 40+ CWEs | ✓ | ✓ | ✓ | ✓ | — |
| Branch-level coverage | ✓ | — | — | — | line only |
| CPG taint analysis | ✓ | ✓ | ✓ | — | — |
| Security + coverage unified | ✓ | security | security | security | coverage |
| MCP server (33 tools) | ✓ | — | — | — | — |
| Deploy readiness score | ✓ | — | — | — | — |
| Single binary, zero deps | ✓ | ✓ | cloud | cloud | pip |
| 11 languages | ✓ | ✓ | ✓ | ✓ | Python |

---

## Installation

### Claude Code Plugin (Recommended)

```bash
# From GitHub
claude plugins marketplace add sahajamoth/apex
claude plugins install apex@apex

# Or from a local clone
git clone https://github.com/sahajamoth/apex.git
claude plugins marketplace add ./apex
claude plugins install apex@apex
```

This installs slash commands, 20+ AI agents, and 33 MCP tools.

### Verify

In Claude Code:
```
/apex init      # Should detect your project and generate apex.toml
apex doctor     # Should show all green checks
```

<details>
<summary><strong>What gets installed</strong></summary>

| Component | Description |
|-----------|-------------|
| `apex` binary | CLI tool with 35+ subcommands |
| 33 MCP tools | `apex_run`, `apex_audit`, `apex_complexity`, etc. — callable by Claude |
| `/apex` slash commands | `/apex`, `/apex detect`, `/apex hunt`, `/apex deploy`, `/apex intel` |
| 20+ AI agents | `apex`, `apex-hunter`, `apex-captain`, per-language crew agents |
| `apex.toml` generator | Auto-config via `apex init` |

</details>

> **Standalone CLI, GitHub Actions, CI/CD:** See [docs/STANDALONE.md](docs/STANDALONE.md)

---

## Commands Reference

> All commands work both as Claude Code slash commands (`/apex detect`)
> and as standalone CLI (`apex audit --target . --lang python`).
> Full standalone docs: [docs/STANDALONE.md](docs/STANDALONE.md)

<details>
<summary><strong>Core</strong></summary>

```bash
apex run --target ./project --lang python      # Coverage gap report
apex ratchet --target ./project --min-cov 0.8  # CI gate
apex doctor                                     # Check dependencies
apex audit --target ./project --lang python     # Security audit
```

</details>

<details>
<summary><strong>Pack A: Per-Test Branch Index</strong></summary>

```bash
apex index --target ./project --lang python --parallel 8
```

Runs each test individually under coverage, builds a map of test→branches.
Stored in `.apex/index.json`. Required before intelligence commands.

</details>

<details>
<summary><strong>Pack B: Test Intelligence</strong></summary>

```bash
apex test-optimize --target .                  # Minimal test subset
apex test-prioritize --target . --changed-files src/auth.py
apex flaky-detect --target . --lang python --runs 5
```

</details>

<details>
<summary><strong>Pack C: Source Intelligence</strong></summary>

```bash
apex dead-code --target .                      # Semantically dead code
apex lint --target . --lang python             # Runtime-prioritized lints
apex complexity --target .                     # Exercised vs static complexity
```

</details>

<details>
<summary><strong>Pack D: Behavioral Analysis & CI/CD</strong></summary>

```bash
apex diff --target . --base main               # Behavioral diff
apex regression-check --target . --base main   # CI gate for behavior changes
apex risk --target . --changed-files src/auth.py
apex hotpaths --target . --top 20
apex contracts --target .                      # Discover invariants
apex deploy-score --target .                   # Aggregate confidence 0-100
```

</details>

<details>
<summary><strong>Pack E: Documentation</strong></summary>

```bash
apex docs --target . --output docs/behavioral.md
```

</details>

<details>
<summary><strong>Pack F: Security</strong></summary>

```bash
apex attack-surface --target . --lang python --entry-pattern test_api
apex verify-boundaries --target . --lang python \
  --entry-pattern test_api --auth-checks check_auth --strict
```

</details>

<details>
<summary><strong>Pack G: Supply Chain Security</strong></summary>

```bash
# Full transitive dependency tree snapshot (Cargo, npm, Go, PyPI, +4 more)
apex enterprise supply-chain-snapshot

# Diff two snapshots — detect what changed deep in the dependency tree
apex enterprise supply-chain-diff --from 1 --to 0

# Risk scoring with 9 signal types (checksum mutation, coordinated updates, etc.)
apex enterprise supply-chain-audit --threshold 5.0
```

Detects transitive dependency poisoning: A uses B, B uses C, C uses D — attacker
compromises D, and months later D propagates into A through natural update cycles.
Tree snapshots capture the full resolved dependency graph with depth, propagation
paths, checksums, and provenance per node.

</details>

---

## Claude Code Integration

APEX integrates natively with Claude Code for an AI-enhanced workflow.
The standalone CLI works without any AI tooling — Claude Code adds
slash commands and auto-triggered agents on top.

### Slash Commands

| Command | What it does |
|---------|-------------|
| `/apex` | **Dashboard** — deploy score, key findings, recommended next actions |
| `/apex-run` | **Autonomous loop** — measures gaps, writes tests, re-measures, repeats |
| `/apex-index` | Build per-test branch index for intelligence commands |
| `/apex-intel` | Full SDLC intelligence — test quality, risk, dead code, hotpaths, contracts |
| `/apex-deploy` | Deployment readiness — GO / CAUTION / BLOCK with confidence score |
| `/apex-status` | Coverage table for the workspace |
| `/apex-gaps` | Top uncovered regions with explanations and suggested tests |
| `/apex-generate` | Generate tests targeting uncovered branches in a crate |
| `/apex-ci 0.8` | CI gate — fails if below threshold |

### Auto-triggered Agents

These fire automatically when Claude Code detects a matching intent:

| Agent | Trigger examples |
|-------|-----------------|
| **apex-coverage-analyst** | "what's our coverage?", "which parts are uncovered?" |
| **apex-test-writer** | "write tests for X", "improve coverage in Y" |
| **apex-runner** | "run apex against Z", "run apex on itself" |
| **apex-sdlc-analyst** | "what's our deploy score?", "find flaky tests" |

### Strategy Selection

The `/apex-run` loop automatically picks the best strategy per gap:

| Target | Primary | Fallback |
|--------|---------|----------|
| Rust workspace | Source-level tests | fuzz harness |
| Python project | Source-level tests | concolic execution |
| C/Rust binary | fuzz | driller (when fuzz stalls) |
| JavaScript | Source-level tests | — |

---

## Architecture

Rust workspace, 16 crates. Heavy dependencies (Z3, LibAFL, PyO3, Inkwell,
Firecracker) are behind feature flags — not compiled by default.

| Crate | Role |
|-------|------|
| `apex-core` | Shared types, traits, config |
| `apex-coverage` | Coverage oracle, bitmap tracking, continuous branch distance heuristics |
| `apex-instrument` | Multi-language instrumentation (Python, JS, Java, Rust, LLVM, WASM) |
| `apex-lang` | Language-specific test runners |
| `apex-sandbox` | Process / WASM / Firecracker isolation |
| `apex-agent` | AI-driven test generation, priority scheduler, solver cache |
| `apex-synth` | Test synthesis via Tera templates + LLM-guided refinement loop |
| `apex-symbolic` | SMT-LIB2 constraint solving, gradient descent solver (optional Z3) |
| `apex-concolic` | Concolic execution (optional PyO3 tracer) |
| `apex-fuzz` | Coverage-guided fuzzing with MOpt (optional LibAFL) |
| `apex-detect` | Security patterns, hardcoded secrets, CWE-mapped findings |
| `apex-cpg` | Code Property Graph — taint analysis via reaching definitions |
| `apex-index` | Per-test branch indexing, SDLC analysis |
| `apex-rpc` | gRPC distributed coordination |
| `apex-mir` | MIR parsing, control-flow analysis |
| `apex-cli` | CLI binary — 20 subcommands |

### Analysis Mechanisms

APEX integrates fundamental mechanisms from established tools
(see [docs/INSPIRATION.md](docs/INSPIRATION.md) for details):

| Mechanism | From | APEX Crate |
|-----------|------|------------|
| Continuous branch distance (Korel fitness) | EvoMaster | `apex-coverage` |
| Gradient descent constraint solving | Angora | `apex-symbolic` |
| Code Property Graph + taint analysis | Joern | `apex-cpg` |
| LLM-guided test refinement (closed loop) | CoverUp | `apex-synth` |
| Priority-based exploration scheduler | Owi + EvoMaster | `apex-agent` |
| Solver caching with negation inference | Owi | `apex-agent` |

<details>
<summary>Optional feature flags</summary>

| Feature | Crate | Enables |
|---------|-------|---------|
| `llvm-instrument` | apex-instrument | LLVM-based instrumentation via inkwell |
| `wasm-instrument` | apex-instrument | WebAssembly instrumentation |
| `z3-solver` | apex-symbolic | Z3 SMT solver |
| `kani-prover` | apex-symbolic | Kani bounded model checking |
| `pyo3-tracer` | apex-concolic | Python concolic tracer |
| `libafl-backend` | apex-fuzz | LibAFL fuzzer backend |
| `firecracker` | apex-sandbox | Firecracker microVM isolation |

```bash
cargo build --release --features "apex-symbolic/z3-solver,apex-fuzz/libafl-backend"
```

</details>

---

## Configuration

```toml
# apex.toml
[coverage]
target = 1.0
min_ratchet = 0.8

[fuzz]
corpus_max = 10000
stall_iterations = 50

[agent]
max_rounds = 3

[sandbox]
process_timeout_ms = 10000
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

Bug reports and feature requests: [GitHub Issues](https://github.com/sahajamoth/apex/issues).

## License

[MIT](LICENSE)
