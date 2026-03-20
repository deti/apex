<!-- status: ACTIVE -->

# Agentic Instrumentation Pipeline

**Goal:** Replace the hardcoded `install_deps()` + `instrument()` code path in
`apex analyze` with an agentic pipeline that dispatches a language crew agent to
read the project, install dependencies, run tests with coverage, and handle
errors adaptively -- fixing the 8-out-of-11 real-world failure rate.

**Date:** 2026-03-20
**Captain:** apex

---

## Problem Statement

`run_analyze()` in `crates/apex-cli/src/lib.rs` (line 786) calls two
deterministic functions:

1. `install_deps()` (line 1639) -- a 60-line match on Language that calls
   `runner.install_deps(target)` for each language. Every runner executes a
   fixed sequence of shell commands (pip install, npm install, bundle install,
   etc.) with no ability to adapt when commands fail.

2. `instrument()` (line 1704) -- a 65-line match on Language that calls
   `Instrumentor::instrument()` for each language. Each instrumentor runs a
   fixed coverage tool (coverage.py, istanbul, JaCoCo, cargo-llvm-cov, etc.)
   with no error recovery.

These fail on real-world projects because:

| Project | Language | Failure Mode |
|---------|----------|-------------|
| CPython | Python | venv creation inside Python source tree conflicts |
| Rails | Ruby | mysql2 gem needs system mysql headers |
| Spring Boot | Java | JaCoCo init.gradle fails on multi-module Gradle |
| ktor | Kotlin | Gradle needs specific JDK version |
| Vapor | Swift | swift test has compile errors needing Xcode config |
| TypeScript projects | JS/TS | V8 coverage JSON format varies by Node version |
| .NET projects | C# | dotnet not on PATH or wrong SDK version |
| Linux kernel | C | kernel headers not available, custom build system |

No amount of conditional logic will cover every project. An agent can read the
project's README, CONTRIBUTING.md, Makefile, CI configs, and error messages to
reason about alternatives.

## Architecture

```
apex analyze --target ./project --lang python
  |
  +-- 1. preflight_check()            [code -- fast, deterministic]
  |     Returns: PreflightInfo { build_system, test_framework, tools, warnings }
  |
  +-- 2. IF --agentic (default):
  |     DISPATCH language crew agent   [agent -- reads project, sets up env]
  |     Input:  PreflightInfo + target path + language + coverage_target
  |     Agent reads: README.md, CONTRIBUTING.md, Makefile, CI configs, etc.
  |     Agent does:
  |       - Installs missing tools (creates venvs, runs bundle install, etc.)
  |       - Runs tests with coverage instrumentation
  |       - Handles errors: reads messages, tries alternatives, adjusts
  |       - Produces: coverage data in .apex/coverage/ directory
  |     Output: AgentCoverageResult (JSON on stdout)
  |
  |   ELSE (--no-agent):
  |     install_deps() + instrument()  [code -- deterministic fallback]
  |
  +-- 3. parse_agent_coverage()        [code -- deterministic]
  |     Reads .apex/coverage/ directory, builds CoverageOracle
  |
  +-- 4. detect pipeline               [code -- pattern matching]
  |
  +-- 5. compound analyzers            [code -- deterministic]
  |
  +-- 6. unified report                [code -- format output]
```

## File Map

| Crew | Files | Purpose |
|------|-------|---------|
| **foundation** | `crates/apex-core/src/types.rs` | `AgentCoverageResult` struct |
| **foundation** | `crates/apex-core/src/traits.rs` | `PreflightInfo` serialization |
| **platform** | `crates/apex-cli/src/lib.rs` | `--agentic` flag, dispatch logic, result parsing |
| **platform** | `crates/apex-cli/src/agent_dispatch.rs` | New module: agent prompt builder + subprocess dispatch |
| **agent-ops** | `.claude/agents/apex-coverage-agent.md` | Generic coverage agent system prompt |
| **agent-ops** | `.claude/agents/lang-*.md` | Per-language coverage agent prompts (update existing) |
| **runtime** | `crates/apex-instrument/src/lib.rs` | Export `parse_coverage_json()` for agent output |
| **runtime** | `crates/apex-coverage/src/lib.rs` | `CoverageOracle::from_agent_result()` constructor |

## Structured Agent Protocol

The agent writes a JSON result to stdout (last line, delimited by markers):

```json
APEX_COVERAGE_RESULT_BEGIN
{
  "success": true,
  "coverage_dir": "/path/to/project/.apex/coverage",
  "coverage_format": "lcov",
  "total_branches": 1247,
  "covered_branches": 891,
  "coverage_pct": 71.4,
  "test_output_path": "/path/to/project/.apex/test-output.log",
  "test_count": 342,
  "test_pass": 340,
  "test_fail": 2,
  "test_skip": 0,
  "errors_encountered": [
    "pip install failed: externally-managed-environment",
    "retried with: python -m venv .apex/venv && .apex/venv/bin/pip install -e ."
  ],
  "tools_used": ["python3.12", "coverage.py 7.4", "pytest 8.1"],
  "duration_secs": 45
}
APEX_COVERAGE_RESULT_END
```

Supported `coverage_format` values: `lcov`, `cobertura`, `jacoco`, `istanbul`,
`v8`, `llvm-cov-json`, `go-cover`, `simplecov`, `coverlet`, `llvm-profdata`.

The CLI parses whichever format the agent produced using existing parsers in
apex-instrument (which already handle all of these).

---

## Wave 1: Foundation -- Core types and serialization (no dependencies)

### Task 1.1 -- foundation crew
**Files:** `crates/apex-core/src/types.rs`
- [ ] Define `AgentCoverageResult` struct with all fields from the protocol above
- [ ] Derive `Serialize, Deserialize, Debug, Clone` on it
- [ ] Add `AgentCoverageError` variant to the result for failure cases
- [ ] Write unit test: round-trip serialize/deserialize of AgentCoverageResult
- [ ] Run `cargo test -p apex-core` -- confirm pass
- [ ] Commit: "feat(core): add AgentCoverageResult type for agentic pipeline"

### Task 1.2 -- foundation crew
**Files:** `crates/apex-core/src/traits.rs`
- [ ] Add `#[derive(serde::Serialize)]` to `PreflightInfo`
- [ ] Add `pub fn to_json(&self) -> String` method on PreflightInfo
- [ ] Write unit test: serialize PreflightInfo with populated fields
- [ ] Run `cargo test -p apex-core` -- confirm pass
- [ ] Commit: "feat(core): make PreflightInfo serializable for agent dispatch"

### Task 1.3 -- runtime crew
**Files:** `crates/apex-coverage/src/lib.rs`
- [ ] Add `pub fn from_agent_result(result: &AgentCoverageResult) -> Self` on CoverageOracle
- [ ] This constructor reads the coverage file at `coverage_dir` using the declared format
- [ ] Dispatch to existing format parsers based on `coverage_format` field
- [ ] Write unit test with a mock AgentCoverageResult pointing to a fixture lcov file
- [ ] Run `cargo test -p apex-coverage` -- confirm pass
- [ ] Commit: "feat(coverage): CoverageOracle::from_agent_result constructor"

---

## Wave 2: Platform -- CLI dispatch mechanism (depends on Wave 1)

### Task 2.1 -- platform crew
**Files:** `crates/apex-cli/src/lib.rs`
- [ ] Add `--agentic` flag to `AnalyzeArgs` (default: true)
- [ ] Add `--no-agent` flag as `--agentic=false` alias
- [ ] In `run_analyze()`, branch on `args.agentic`:
  - true: call `agent_dispatch::run_coverage_agent()` (new module)
  - false: call existing `install_deps()` + `instrument()` (unchanged)
- [ ] After agent returns, call `CoverageOracle::from_agent_result()` to populate oracle
- [ ] Continue with existing exploration/detect/compound pipeline unchanged
- [ ] Run `cargo check -p apex-cli` -- confirm compiles
- [ ] Commit: "feat(cli): add --agentic flag to apex analyze"

### Task 2.2 -- platform crew
**Files:** `crates/apex-cli/src/agent_dispatch.rs` (new file)
- [ ] Create module with `pub async fn run_coverage_agent()` function
- [ ] Function signature: `(lang: Language, target: &Path, preflight: &PreflightInfo, coverage_target: f64) -> Result<AgentCoverageResult>`
- [ ] Build structured prompt from template + preflight JSON + target path
- [ ] Dispatch via `tokio::process::Command` running `claude` subprocess:
  - `claude --print --system-prompt <prompt> --message <task>`
  - Parse stdout for `APEX_COVERAGE_RESULT_BEGIN...END` markers
  - Extract JSON, deserialize to `AgentCoverageResult`
- [ ] Implement timeout (configurable, default 10 minutes)
- [ ] Implement fallback: if agent dispatch fails, return error (caller falls back to deterministic)
- [ ] Write unit test: parse a mock agent stdout with embedded JSON result
- [ ] Run `cargo test -p apex-cli` -- confirm pass
- [ ] Commit: "feat(cli): agent dispatch module for coverage pipeline"

### Task 2.3 -- platform crew
**Files:** `crates/apex-cli/src/agent_dispatch.rs`
- [ ] Add MCP dispatch path as alternative to subprocess
- [ ] If APEX is running as an MCP server (detectable via env var), use tool call instead
- [ ] Add `AgentDispatchConfig` struct to `apex-core/config.rs`:
  - `agent_binary: String` (default: "claude")
  - `agent_timeout_secs: u64` (default: 600)
  - `agent_model: Option<String>` (override model)
- [ ] Wire config through from `ApexConfig`
- [ ] Run `cargo check -p apex-cli` -- confirm compiles
- [ ] Commit: "feat(cli): MCP dispatch path and agent config"

---

## Wave 3: Agent prompts -- Language crew agent definitions (depends on Wave 2)

### Task 3.1 -- agent-ops crew
**Files:** `.claude/agents/apex-coverage-agent.md` (new file)
- [ ] Write the base system prompt for the coverage agent:
  - Role: "You are the APEX coverage agent. Your job is to set up the environment and run tests with coverage instrumentation for a target project."
  - Protocol: explain the APEX_COVERAGE_RESULT JSON output format
  - Strategy: "Read the project first (README, CI configs, build files). Understand how it builds and tests. Then execute."
  - Error handling: "If a command fails, read the error message. Try an alternative approach. Common alternatives: create venv, use different package manager, install system deps, adjust JDK version."
  - Constraints: "Do not modify the project's source code. Only install tools and run tests."
  - Security: "Do not execute arbitrary code from the target project outside of its test suite."
- [ ] Review prompt for completeness against the 8 known failure modes
- [ ] Commit: "feat(agents): base coverage agent system prompt"

### Task 3.2 -- agent-ops crew
**Files:** `.claude/agents/lang-python-coverage.md`, `.claude/agents/lang-jvm-coverage.md`, `.claude/agents/lang-js-coverage.md` (new files)
- [ ] Python agent: knows about venv, uv, pip, coverage.py, pytest-cov, tox, nox
  - Specific guidance for: externally-managed-environment, PEP 668, source-tree venvs
- [ ] JVM agent: knows about Gradle, Maven, JaCoCo, Kover, JUnit, TestNG
  - Specific guidance for: multi-module Gradle, init.gradle injection, JDK version detection
- [ ] JS/TS agent: knows about npm, yarn, pnpm, bun, jest, vitest, c8, istanbul, V8
  - Specific guidance for: V8 coverage format variations, monorepo workspaces
- [ ] Commit: "feat(agents): language-specific coverage agent prompts"

### Task 3.3 -- agent-ops crew
**Files:** `.claude/agents/lang-ruby-coverage.md`, `.claude/agents/lang-go-coverage.md`, `.claude/agents/lang-swift-coverage.md`, `.claude/agents/lang-dotnet-coverage.md`, `.claude/agents/lang-rust-coverage.md`, `.claude/agents/lang-c-coverage.md` (new files)
- [ ] Ruby agent: simplecov, bundler, rspec/minitest, system gem deps
- [ ] Go agent: go test -coverprofile, go tool cover
- [ ] Swift agent: swift test --enable-code-coverage, xcresulttool, llvm-profdata
- [ ] .NET agent: coverlet, dotnet test --collect, reportgenerator
- [ ] Rust agent: cargo-llvm-cov, cargo-nextest, RUSTFLAGS for coverage
- [ ] C/C++ agent: gcov, llvm-cov, compile flags, cmake/make/xmake detection
- [ ] Commit: "feat(agents): remaining language coverage agent prompts"

---

## Wave 4: Integration wiring (depends on Wave 3)

### Task 4.1 -- platform crew
**Files:** `crates/apex-cli/src/agent_dispatch.rs`
- [ ] Implement prompt template selection based on Language enum
- [ ] For each language, load the corresponding `.claude/agents/lang-*-coverage.md` as system prompt
- [ ] Embed the base `apex-coverage-agent.md` as a prefix to all language prompts
- [ ] If agent prompt file not found, fall back to base prompt only (with language hint)
- [ ] Write integration test: verify prompt construction for Python, Java, JS
- [ ] Run `cargo test -p apex-cli` -- confirm pass
- [ ] Commit: "feat(cli): language-aware prompt template selection"

### Task 4.2 -- runtime crew
**Files:** `crates/apex-instrument/src/lib.rs`, `crates/apex-coverage/src/lib.rs`
- [ ] Add `pub fn parse_coverage_file(path: &Path, format: &str) -> Result<Vec<BranchCoverage>>` to apex-instrument
- [ ] Route to existing parsers: lcov -> python.rs, cobertura -> java.rs, istanbul -> javascript.rs, etc.
- [ ] Wire into `CoverageOracle::from_agent_result()` so it uses format-aware parsing
- [ ] Write unit test: parse fixture files in each format
- [ ] Run `cargo test -p apex-instrument -p apex-coverage` -- confirm pass
- [ ] Commit: "feat(instrument): unified format-aware coverage file parser"

### Task 4.3 -- platform crew
**Files:** `crates/apex-cli/src/lib.rs`, `crates/apex-cli/src/mcp.rs`
- [ ] Add `agentic` parameter to MCP `analyze` tool
- [ ] When agentic=true in MCP mode, dispatch agent via tool call chain
- [ ] Update MCP schema to include `agentic` and `no_agent` parameters
- [ ] Run `cargo check -p apex-cli` -- confirm compiles
- [ ] Commit: "feat(mcp): expose agentic flag in MCP analyze tool"

---

## Wave 5: Validation (depends on Wave 4)

### Task 5.1 -- platform crew
**Files:** tests (no new production code)
- [ ] Test agentic pipeline on APEX itself (Rust): `apex analyze --target . --lang rust`
- [ ] Verify: agent produces cargo-llvm-cov output, CLI parses it, detect pipeline runs
- [ ] Verify: `--no-agent` flag still works with deterministic path
- [ ] Document results in `.fleet/plans/2026-03-20-agentic-instrumentation.md`

### Task 5.2 -- platform crew
**Files:** tests (no new production code)
- [ ] Test on a Python project (e.g., zettel or a fixture project)
- [ ] Verify: agent creates venv, installs deps, runs pytest-cov, produces lcov
- [ ] Verify: coverage numbers are reasonable (not 0%, not 100% on a real project)
- [ ] Test error recovery: delete venv mid-run, verify agent retries

### Task 5.3 -- platform crew
**Files:** tests (no new production code)
- [ ] Test on 3+ validation repos from different languages
- [ ] For each: verify the agent succeeds where the deterministic path failed
- [ ] Capture timing data: how long does agent dispatch add vs deterministic?
- [ ] Write summary of pass/fail rates

---

## Risk Analysis

| Risk | Mitigation |
|------|-----------|
| Agent hallucination -- reports fake coverage | QA officer verifies coverage file exists and contains valid data |
| Agent takes too long | Configurable timeout (default 10min), deterministic fallback |
| Agent modifies target source code | System prompt explicitly forbids it; security officer checks git status after |
| No `claude` binary on PATH | Fallback to deterministic pipeline with warning |
| Agent costs (token usage) | Log token count from agent response; add `--agent-budget` flag later |
| Coverage format mismatch | Agent reports format explicitly; parser validates before Oracle construction |
| Circular dependency (APEX analyzing itself) | Agent runs in subprocess, not in-process; no shared state |

## Decision Gates

- **Gate 1 (after Wave 1):** Do the foundation types compile? Does PreflightInfo serialize correctly? Proceed only if cargo test passes.
- **Gate 2 (after Wave 2):** Does the dispatch mechanism work end-to-end with a mock agent? Can it parse the structured JSON response?
- **Gate 3 (after Wave 3):** Review agent prompts for completeness against all 8 known failure modes. Each prompt must address at least 3 alternative approaches for its language.
- **Gate 4 (after Wave 4):** Full integration: dispatch real agent, parse real coverage, run detect pipeline. Must produce valid results on at least 1 project.
- **Gate 5 (after Wave 5):** Validation across 5+ projects. Agentic path must succeed on at least 3 projects where deterministic path fails.

## Crew Assignment Summary

| Crew | Tasks | Wave(s) |
|------|-------|---------|
| foundation | 1.1, 1.2 | 1 |
| runtime | 1.3, 4.2 | 1, 4 |
| platform | 2.1, 2.2, 2.3, 4.1, 4.3, 5.1, 5.2, 5.3 | 2, 4, 5 |
| agent-ops | 3.1, 3.2, 3.3 | 3 |

## Officer Review Points

- **QA officer** after Wave 2: verify dispatch + parsing logic has edge case handling
- **Security officer** after Wave 3: review agent prompts for sandbox escape risks
- **Architect officer** after Wave 4: review the overall integration for clean separation of concerns
- **QA officer** after Wave 5: verify validation results and coverage data integrity
