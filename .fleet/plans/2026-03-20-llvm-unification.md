<!-- status: ACTIVE -->

# Unified LLVM Coverage Backend

**Date:** 2026-03-20
**Priority:** Critical (core value proposition)
**Scope:** crates/apex-instrument, crates/apex-cli

## Problem

Three separate instrumentors independently implement the same LLVM source-based coverage pipeline:

1. **rust_cov.rs** -- `cargo llvm-cov --json` then `parse_llvm_json()` (segments with 6-field filter: has_count AND is_region_entry AND NOT is_gap)
2. **c_coverage.rs** -- `clang -fprofile-instr-generate` then `llvm-cov export` then `parse_llvm_cov_json()` (segments with 5-field filter: has_count AND is_region_entry, using `json_truthy()` for int/bool compat)
3. **swift.rs** -- `swift test --enable-code-coverage` then `parse_llvm_cov_json()` (segments with 3-field minimum, no region_entry filter, creates both direction=0 and direction=1 branches per segment)

All three consume `llvm-cov export --format=json` output, which uses the **same JSON schema** regardless of source language. But they parse it with three different parsers that have **divergent semantics**:

| Parser | Min fields | Checks has_count | Checks is_region_entry | Checks is_gap | Bool/int compat | Direction model |
|--------|-----------|-----------------|----------------------|---------------|----------------|----------------|
| rust_cov `parse_llvm_json` | 6 | yes | yes | yes | no | direction=0 only |
| c_coverage `parse_llvm_cov_json` | 5 | yes | yes | no | yes (`json_truthy`) | direction=0 only |
| swift `parse_llvm_cov_json` | 3 | no | no | no | no | direction=0 AND direction=1 per segment |

The Swift parser is the most divergent -- it creates 2 BranchIds per segment (covered direction + uncovered direction) and treats every segment as a coverable unit regardless of has_count or is_region_entry. This means Swift reports 2x the branches and counts uncovered lines as "executed in the uncovered direction."

## Architecture

### Tier 1: Unified LLVM backend (C, C++, Rust, Swift)

One `LlvmCoverageBackend` struct handles the 4-step pipeline for all compiled languages:

```
compile with coverage flags -> run tests -> llvm-profdata merge -> llvm-cov export --format=json
```

One `parse_llvm_cov_export()` function parses the output. Language-specific differences are limited to:
- How to invoke the compiler (cargo vs clang vs swiftc)
- How to invoke tests (cargo test vs make test vs swift test)
- Where to find llvm-profdata/llvm-cov (PATH vs xcrun)

### Tier 2: Native coverage (interpreted + JVM + Go)

No changes. Python, JavaScript, Ruby, Java/Kotlin, Go, C#, WASM keep their existing instrumentors. These use fundamentally different coverage tools (coverage.py, istanbul, JaCoCo, etc.) that do not produce LLVM-cov JSON.

### Tier 3: SHM-based (fuzzing feedback)

No changes. `shim.rs` (LD_PRELOAD `__sanitizer_cov_trace_pc_guard`) stays for real-time branch feedback during fuzzing. Tier 1 LLVM coverage is the slower but more detailed source-based coverage used for gap reporting.

## File Map

| Crew | Files | Action |
|------|-------|--------|
| foundation | `crates/apex-instrument/src/llvm_coverage.rs` (NEW) | Create unified backend |
| foundation | `crates/apex-instrument/src/lib.rs` | Add `pub mod llvm_coverage` |
| lang-rust | `crates/apex-instrument/src/rust_cov.rs` | Delegate to LlvmCoverageBackend |
| lang-c-cpp | `crates/apex-instrument/src/c_coverage.rs` | Delegate LLVM path to LlvmCoverageBackend |
| lang-swift | `crates/apex-instrument/src/swift.rs` | Delegate to LlvmCoverageBackend |
| platform | `crates/apex-cli/src/lib.rs` | Simplify instrument() dispatch |

## Detailed Design

### New: `llvm_coverage.rs`

```rust
/// Unified LLVM source-based coverage backend.
///
/// All compiled languages that use LLVM (C, C++, Rust, Swift) produce the
/// same `llvm-cov export --format=json` output. This module provides:
/// 1. A single JSON parser (`parse_llvm_cov_export`)
/// 2. Tool resolution (`resolve_llvm_tools`)
/// 3. The 4-step pipeline (`LlvmCoverageBackend::run`)

pub struct LlvmCoverageBackend {
    language: Language,
    target_root: PathBuf,

    // Tool paths (resolved at construction)
    profdata_cmd: Vec<String>,  // ["llvm-profdata"] or ["xcrun", "llvm-profdata"]
    llvm_cov_cmd: Vec<String>,  // ["llvm-cov"] or ["xcrun", "llvm-cov"]

    // Language-specific pipeline
    pipeline: Box<dyn LlvmPipeline>,

    // Output paths
    profraw_dir: PathBuf,
    profdata_path: PathBuf,
}

/// Language-specific parts of the LLVM coverage pipeline.
/// Each language implements this to handle compilation and test execution.
#[async_trait]
pub trait LlvmPipeline: Send + Sync {
    /// Compile with coverage flags and run tests.
    /// Returns the path to the instrumented binary (needed for llvm-cov export)
    /// and the directory containing .profraw files.
    async fn compile_and_test(&self, target: &Target) -> Result<PipelineOutput>;

    /// Whether this pipeline handles profdata merge + export internally
    /// (e.g., cargo-llvm-cov does everything in one command).
    fn self_contained(&self) -> bool { false }

    /// For self-contained pipelines, return the JSON directly.
    async fn export_json(&self, target: &Target) -> Result<Vec<u8>> {
        Err(ApexError::Instrumentation("not self-contained".into()))
    }
}

pub struct PipelineOutput {
    pub binary_path: PathBuf,
    pub profraw_dir: PathBuf,
}
```

### Unified JSON parser

```rust
/// Parse `llvm-cov export --format=json` output.
///
/// The JSON schema is the same for C, C++, Rust, and Swift:
/// ```json
/// { "data": [{ "files": [{ "filename": "...", "segments": [...] }] }] }
/// ```
///
/// Each segment is `[line, col, count, has_count, is_region_entry, is_gap_region]`.
/// We filter to segments where has_count=true AND is_region_entry=true AND is_gap=false.
/// This matches the Rust parser semantics (the most correct of the three).
///
/// Boolean fields may be encoded as true/false OR 0/1 depending on LLVM version.
pub fn parse_llvm_cov_export(
    bytes: &[u8],
    target_root: &Path,
    filter: &FileFilter,
) -> Result<ParsedCoverage> { ... }

pub struct ParsedCoverage {
    pub branch_ids: Vec<BranchId>,
    pub executed_branch_ids: Vec<BranchId>,
    pub file_paths: HashMap<u64, PathBuf>,
}

pub struct FileFilter {
    /// Skip files outside target root (stdlib, deps)
    pub require_under_root: bool,
    /// Skip test files (tests/, *_test.rs, *_tests.rs)
    pub skip_test_files: bool,
    /// Custom skip patterns
    pub skip_patterns: Vec<String>,
}
```

Key decisions:
- **6-field segments with full filtering** (has_count AND is_region_entry AND NOT is_gap) -- matches rust_cov.rs, the most precise
- **json_truthy()** from c_coverage.rs -- handles LLVM version differences where bools are encoded as 0/1
- **Direction=0 only** -- the Swift dual-direction model is incorrect (it inflates branch counts by 2x and marks uncovered lines as "executed in the uncovered direction")
- **FileFilter** is configurable per language (Rust skips test files, C does not need to)
- **Deduplication** -- sort + dedup as in rust_cov.rs

### Language pipelines

**RustLlvmPipeline** (self-contained):
- `cargo llvm-cov --json --output-path <path>` handles compile + test + merge + export
- `self_contained() = true`, `export_json()` returns the JSON file
- Workspace detection + `--exclude apex-rpc` logic preserved from rust_cov.rs
- PATH propagation preserved

**ClangLlvmPipeline**:
- Compile: `clang -fprofile-instr-generate -fcoverage-mapping -g -O0`
- Test: run binary with `LLVM_PROFILE_FILE=<profraw_dir>/default.profraw`
- Uses standard profdata merge + llvm-cov export
- Falls back to GccGcovPipeline when clang is not available (not LLVM, separate path)

**SwiftLlvmPipeline** (self-contained):
- `swift test --enable-code-coverage` handles compile + test
- `swift test --show-codecov-path` returns the JSON path
- `self_contained() = true`, `export_json()` reads the file at that path
- SWIFTPM_CACHE_DIR wiring preserved

### GCC fallback

GCC uses a fundamentally different pipeline (--coverage + gcov + .gcov text files). It stays in `c_coverage.rs` as the fallback when clang is unavailable. The `compile_and_run_gcc_gcov()` function and `parse_gcov_output()` are unchanged. Only the `compile_and_run_llvm_cov()` function in c_coverage.rs delegates to the new backend.

### Tool resolution

```rust
pub struct LlvmToolchain {
    pub profdata: Vec<String>,  // ["llvm-profdata"] or ["xcrun", "llvm-profdata"]
    pub llvm_cov: Vec<String>,  // ["llvm-cov"] or ["xcrun", "llvm-cov"]
}

/// Resolve LLVM tool paths, checking:
/// 1. LLVM_PROFDATA / LLVM_COV env vars (explicit override)
/// 2. llvm-profdata / llvm-cov on PATH
/// 3. xcrun llvm-profdata / xcrun llvm-cov (macOS Xcode)
pub fn resolve_llvm_tools() -> Result<LlvmToolchain> { ... }
```

## Wave 1: Foundation -- Unified LLVM backend (no dependencies)

### Task 1.1 -- Create llvm_coverage.rs skeleton
**Crew:** foundation
**Files:** `crates/apex-instrument/src/llvm_coverage.rs` (NEW), `crates/apex-instrument/src/lib.rs`
- [ ] Create `LlvmCoverageBackend` struct with fields listed above
- [ ] Define `LlvmPipeline` trait
- [ ] Define `PipelineOutput`, `ParsedCoverage`, `FileFilter` structs
- [ ] Add `pub mod llvm_coverage` to lib.rs
- [ ] Write failing test: `LlvmCoverageBackend::new()` compiles
- [ ] Implement constructors
- [ ] Run test, confirm pass
- [ ] Commit

### Task 1.2 -- Implement resolve_llvm_tools()
**Crew:** foundation
**Files:** `crates/apex-instrument/src/llvm_coverage.rs`
- [ ] Write failing test: `resolve_llvm_tools()` returns toolchain on macOS (xcrun path)
- [ ] Write test: env var override takes precedence
- [ ] Write test: returns error when no tools found
- [ ] Implement `resolve_llvm_tools()` checking env vars, PATH, xcrun
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 1.3 -- Implement unified parse_llvm_cov_export()
**Crew:** foundation
**Files:** `crates/apex-instrument/src/llvm_coverage.rs`
- [ ] Copy `parse_llvm_json()` from rust_cov.rs as baseline (it is the most correct parser)
- [ ] Add `json_truthy()` from c_coverage.rs for LLVM version compat
- [ ] Add `FileFilter` parameter for configurable file skipping
- [ ] Write tests: basic parsing, deduplication, external file skip, test file skip, gap region skip, integer booleans, short segments, empty data
- [ ] Port all relevant tests from rust_cov.rs and c_coverage.rs test suites
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 1.4 -- Implement LlvmCoverageBackend::run()
**Crew:** foundation
**Files:** `crates/apex-instrument/src/llvm_coverage.rs`
- [ ] Implement `run()` method: calls pipeline.compile_and_test() or pipeline.export_json(), then profdata merge, then llvm-cov export, then parse
- [ ] For self-contained pipelines, skip the merge+export steps
- [ ] Wire InstrumentedTarget construction
- [ ] Write mock-based tests using FakePipeline
- [ ] Run tests, confirm pass
- [ ] Commit

## Wave 2: Language crews -- per-language pipelines (depends on Wave 1)

### Task 2.1 -- RustLlvmPipeline
**Crew:** lang-rust
**Files:** `crates/apex-instrument/src/rust_cov.rs`
- [ ] Implement `RustLlvmPipeline` struct implementing `LlvmPipeline`
- [ ] Move workspace detection + --exclude logic into `export_json()`
- [ ] `self_contained() = true` -- cargo-llvm-cov handles everything
- [ ] Keep `RustCovInstrumentor` as the public API, but delegate to `LlvmCoverageBackend` internally
- [ ] Keep `run_coverage_for_test()` and `has_llvm_cov()` public APIs unchanged
- [ ] Deprecate `parse_llvm_json()` -- re-export `parse_llvm_cov_export()` for backward compat
- [ ] Run existing 30+ rust_cov tests, confirm all pass
- [ ] Commit

### Task 2.2 -- ClangLlvmPipeline
**Crew:** lang-c-cpp
**Files:** `crates/apex-instrument/src/c_coverage.rs`
- [ ] Implement `ClangLlvmPipeline` struct implementing `LlvmPipeline`
- [ ] Move clang compile + run logic from `compile_and_run_llvm_cov()` into `compile_and_test()`
- [ ] Keep GCC/gcov fallback path unchanged (it is not LLVM)
- [ ] Keep `CCoverageInstrumentor` as the public API; when clang is available, delegate to `LlvmCoverageBackend`
- [ ] Delete `parse_llvm_cov_json()` from c_coverage.rs (replaced by unified parser)
- [ ] Delete `json_truthy()` from c_coverage.rs (moved to llvm_coverage.rs)
- [ ] Run existing c_coverage tests, confirm all pass
- [ ] Commit

### Task 2.3 -- SwiftLlvmPipeline
**Crew:** lang-swift
**Files:** `crates/apex-instrument/src/swift.rs`
- [ ] Implement `SwiftLlvmPipeline` struct implementing `LlvmPipeline`
- [ ] `self_contained() = true` -- swift test + show-codecov-path handles everything
- [ ] Keep `SwiftInstrumentor` as the public API; delegate to `LlvmCoverageBackend`
- [ ] Delete `parse_llvm_cov_json()` from swift.rs (replaced by unified parser)
- [ ] Delete `derive_relative_path()` from swift.rs (handled by unified parser's path normalization)
- [ ] **Breaking semantic change:** Swift will now report ~50% fewer branches (correct -- the old dual-direction model was wrong)
- [ ] Update Swift tests to match new semantics (1 branch per segment, not 2)
- [ ] Run tests, confirm pass
- [ ] Commit

## Wave 3: Platform -- CLI wiring (depends on Wave 2)

### Task 3.1 -- Simplify instrument() dispatch
**Crew:** platform
**Files:** `crates/apex-cli/src/lib.rs`
- [ ] For Language::Rust, Language::C, Language::Cpp, Language::Swift: construct LlvmCoverageBackend with appropriate pipeline
- [ ] Remove the Language::C fallback chain (LlvmInstrumentor -> CCoverageInstrumentor) -- LlvmCoverageBackend handles clang vs gcc internally
- [ ] Keep Language::C gcc fallback inside CCoverageInstrumentor (transparent to CLI)
- [ ] Remove LlvmInstrumentor from imports (it is for SanitizerCoverage, a different use case)
- [ ] Run CLI smoke tests
- [ ] Commit

### Task 3.2 -- Remove dead code
**Crew:** platform
**Files:** `crates/apex-instrument/src/lib.rs`
- [ ] Audit: is `LlvmInstrumentor` (llvm.rs) still needed? It serves SanitizerCoverage, not source-based coverage. Keep if used for fuzzing.
- [ ] Remove any dead re-exports
- [ ] Run `cargo clippy --workspace -- -D warnings`
- [ ] Commit

## Wave 4: Verification (depends on Wave 3)

### Task 4.1 -- Unit test parity
**Crew:** foundation
**Files:** `crates/apex-instrument/src/llvm_coverage.rs`
- [ ] Verify all tests from rust_cov.rs parse_llvm_json() have equivalents in unified parser
- [ ] Verify all tests from c_coverage.rs parse_llvm_cov_json() have equivalents
- [ ] Verify all tests from swift.rs parse_llvm_cov_json() have equivalents (with updated semantics)
- [ ] Run `cargo nextest run -p apex-instrument`
- [ ] Commit

### Task 4.2 -- Integration test: APEX self-coverage (Rust)
**Crew:** platform
**Files:** N/A (manual verification)
- [ ] Run `apex run --target . --lang rust` on APEX itself
- [ ] Verify coverage percentage is within 1% of pre-change baseline
- [ ] If regression, compare parse output before/after

### Task 4.3 -- Integration test: C project with clang
**Crew:** lang-c-cpp
**Files:** N/A (manual verification)
- [ ] Run `apex run --target tests/fixtures/c-project --lang c` (if fixture exists)
- [ ] Verify clang path produces coverage data via unified backend
- [ ] Verify gcc fallback still works on Linux

### Task 4.4 -- Integration test: Swift project
**Crew:** lang-swift
**Files:** N/A (manual verification)
- [ ] Run `apex run --target <swift-project> --lang swift`
- [ ] Verify coverage data uses unified parser (branch counts ~50% of old values due to direction fix)
- [ ] Verify no regression in file path resolution

## Risk Analysis

### High risk: Swift semantic change
The Swift instrumentor currently creates 2 BranchIds per segment (direction=0 for covered, direction=1 for uncovered). The unified parser creates 1 BranchId per segment (direction=0 only, like Rust and C). This halves Swift's reported branch count.

**Mitigation:** This is a bug fix. The old behavior double-counted every code region and treated "not executed" as "executed in the uncovered direction," which is semantically wrong. Document the change in CHANGELOG.md.

### Medium risk: LLVM version differences
Different LLVM versions encode booleans differently (true/false vs 0/1). The c_coverage.rs `json_truthy()` function handles this but the Rust parser does not.

**Mitigation:** The unified parser adopts `json_truthy()`. All three language paths benefit.

### Low risk: Backward compatibility
`parse_llvm_json()` in rust_cov.rs is public and used by `run_coverage_for_test()`. The `RustTestSandbox` calls it for delta coverage.

**Mitigation:** Keep `parse_llvm_json()` as a thin wrapper around `parse_llvm_cov_export()` during the transition. Deprecate after one release.

## Lines of code estimate

| Component | Estimate |
|-----------|----------|
| llvm_coverage.rs (new) | ~300 lines implementation + ~400 lines tests |
| rust_cov.rs changes | ~-50 lines (delegate to backend) |
| c_coverage.rs changes | ~-80 lines (remove duplicated LLVM path) |
| swift.rs changes | ~-60 lines (remove duplicated parser) |
| apex-cli/src/lib.rs changes | ~-10 lines (simplify dispatch) |
| **Net** | **~100 lines added, ~200 lines removed** |

## Success Criteria

1. `cargo nextest run -p apex-instrument` passes with no regressions
2. `cargo clippy --workspace -- -D warnings` clean
3. Zero duplicated LLVM JSON parsers (currently 3, target 1)
4. All compiled languages (C, C++, Rust, Swift) use the same `parse_llvm_cov_export()`
5. LLVM version bool/int compat handled uniformly
6. Swift branch counts corrected (no dual-direction inflation)
