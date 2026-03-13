# APEX TypeScript/JavaScript Support Design

**Date:** 2026-03-14
**Status:** Draft
**Scope:** TS sub-mode, V8 coverage, ESM/Bun/monorepo, source maps, JS/TS concolic

## Overview

Extend APEX's existing JavaScript support with TypeScript sub-mode detection, V8 coverage format parsing, ESM/Bun/monorepo support, source map remapping, and AST-based concolic execution. `Language::JavaScript` remains the single enum variant; TypeScript is handled as an internal sub-mode.

## Architecture: Layered Pipeline

`JsInstrumentor::instrument()` decomposes into five explicit stages:

```
1. detect_environment() → JsEnvironment
2. select_coverage_tool(env) → CoverageToolConfig
3. run_under_coverage(config) → RawCoverageOutput
4. parse_coverage(raw) → Vec<BranchId>
5. remap_source_maps(branches, target) → (Vec<BranchId>, HashMap<u64, PathBuf>)
```

Each stage is independently testable. Monorepo support wraps stages 1-5 in a per-package loop with merged output.

The existing `Instrumentor` trait takes `&self`. The current `JavaScriptInstrumentor::instrument()` uses an inner mutable state pattern — the five-stage pipeline continues this pattern, with each stage as a pure function taking inputs and returning outputs (no `&mut self` needed).

## Prerequisite: Extract `fnv1a_hash` to apex-core

`fnv1a_hash` is currently duplicated across apex-instrument (python.rs, javascript.rs) and apex-sandbox (javascript.rs). Before adding more call sites (V8 parser, source map remapper), extract to `apex-core::hash::fnv1a_hash()` as shared infrastructure. All existing call sites updated to use the shared version.

## Stage 1: Environment Detection

```rust
struct JsEnvironment {
    runtime: JsRuntime,              // Node, Bun
    pkg_manager: PkgManager,         // Npm, Yarn, Pnpm, Bun
    test_runner: JsTestRunner,       // Jest, Mocha, Vitest, BunTest, NpmScript
    module_system: ModuleSystem,     // CommonJS, ESM, Mixed
    is_typescript: bool,
    source_maps: bool,
    monorepo: Option<MonorepoKind>,  // NpmWorkspaces, Yarn, Pnpm, Turborepo, Nx
}
```

Detection rules:

- **TypeScript**: any `tsconfig*.json` at target root or in package subdirectories (covers `tsconfig.json`, `tsconfig.build.json`, `tsconfig.app.json`). Also detect `.ts`/`.tsx` files in src directories as fallback.
- **Module system**: `"type": "module"` in package.json, `.mjs`/`.cjs` file presence
- **Runtime**: `bun.lockb` or `bunfig.toml` → Bun, else Node
- **Monorepo**: `"workspaces"` in package.json (npm/yarn globs), `pnpm-workspace.yaml` (YAML `packages:` list), `turbo.json`, `nx.json`
- **Test runner / pkg manager**: existing detection logic extracted to a shared `js_env` module in `apex-lang` and used by both `JsRunner` and `JsInstrumentor` (apex-instrument depends on apex-lang). Note: `apex-detect` is for security/bug detection — not the right home for project environment detection.

**`Language::FromStr` update**: Add `"ts"` and `"typescript"` as aliases that resolve to `Language::JavaScript`. No special hint mechanism needed — `JsEnvironment::detect()` re-discovers TypeScript from the filesystem (`tsconfig*.json`, `.ts` files) regardless of how the language was specified.

**Error handling**: If `package.json` is missing or unparseable, return `ApexError::Instrumentation("project not detected: expected package.json ...")`. All error types in this spec use `ApexError::Instrumentation(String)` or `ApexError::LanguageRunner(String)` — the existing error enum, not new sub-enums.

## Stage 2: Coverage Tool Selection

| Environment              | Tool                    | Format   |
|--------------------------|-------------------------|----------|
| Bun                      | `bun test --coverage`   | V8       |
| Vitest                   | `vitest --coverage`     | V8       |
| ESM + Node               | `c8`                    | V8       |
| CJS + Node (nyc present) | `nyc`                   | Istanbul |
| CJS + Node (no nyc)      | `c8`                    | V8       |

```rust
struct CoverageToolConfig {
    tool: CoverageTool,            // Nyc, C8, Vitest, Bun
    command: Vec<String>,
    output_path: CoverageOutput,   // FilePath(PathBuf) | Stdout
    output_format: CoverageFormat, // V8, Istanbul
}
```

V8 is the primary format. Istanbul kept for legacy nyc projects only.

**Vitest coverage provider**: Check for `@vitest/coverage-v8` in node_modules before using Vitest as coverage tool. If missing, fall back to c8 wrapping the Vitest command.

**Error handling**: If the selected tool (c8, nyc) is not installed, return `ApexError::Instrumentation` with the tool name and `npm install` hint.

## Stage 3: Execution

Run the configured command, capture coverage output. Bun emits to stdout; all others write to disk. No structural changes from current approach — just dispatching the right command.

**Error handling**: If the command exits non-zero, distinguish between test failures (exit code 1 with coverage output still produced) and tool failures (no coverage output). Test failures with partial coverage are still useful — parse what's available.

## Stage 4: Coverage Parsing

### V8 Parser (new)

V8 coverage JSON structure:

```json
{
  "result": [{
    "scriptId": "42",
    "url": "file:///abs/path/to/file.js",
    "functions": [{
      "functionName": "myFunc",
      "ranges": [
        { "startOffset": 100, "endOffset": 200, "count": 5 },
        { "startOffset": 120, "endOffset": 150, "count": 0 }
      ]
    }]
  }]
}
```

Parsing steps:

1. For each script entry, convert `file://` URL to repo-relative path, `apex_core::hash::fnv1a_hash()` → `file_id`
2. Build `OffsetIndex` from source text (single pass storing newline byte offsets)
3. For each function's ranges, flatten the nested structure into branch points. V8 ranges are nested: the outermost range is the function body, inner ranges represent conditional sub-paths.
4. Convert `startOffset`/`endOffset` → line/col via `OffsetIndex`
5. Emit `BranchId` entries per branch point

**V8 ranges → `BranchId.direction` mapping:**

V8 ranges are not binary true/false branches. They are nested coverage ranges where sibling ranges at the same nesting level represent alternative paths. The mapping:

- Group sibling ranges (same parent, same nesting depth) by their parent range
- Within each sibling group, assign `direction` as a sequential index (0, 1, 2, ...) — same semantics as Istanbul's branch arm indices
- A range with `count: 0` produces an uncovered `BranchId`; `count > 0` produces a covered one
- Single-child ranges (no branching) are skipped — they represent sequential code, not branches

This aligns with how Istanbul arms already use `direction` as an index rather than a boolean. The doc comment on `BranchId.direction` in `types.rs` currently says "0 = taken, 1 = not-taken" but the existing Istanbul parser already uses it as a sequential index — update the doc comment to reflect actual semantics: "arm index within branch point (0, 1, 2, ...)".

**`BranchId.direction` overflow**: `direction` is `u8` (max 255). Switch statements with >255 arms are theoretically possible in generated/minified code. Strategy: saturate at `u8::MAX` — same approach as col overflow. In practice, source-level switch statements rarely exceed this.

**`BranchId.col` overflow for minified files:**

`BranchId.col` is `u16` (max 65535). Minified JS files can have lines exceeding this. Strategy: saturate at `u16::MAX`. In practice, source map remapping (Stage 5) resolves to original source columns which are always small. For non-source-mapped minified files, the saturated column still identifies the correct line — col is used for disambiguation, not navigation.

### Istanbul Parser (existing)

No changes. Parses `branchMap` and `b` hit counts.

### Shared Infrastructure

- `OffsetIndex` — byte offset → (line, col) mapping. Reused by V8 parser and source map remapper.
- `repo_relative_path()` — normalizes V8 `file://` URLs and Istanbul absolute paths.

**Error handling**: If coverage JSON is missing or malformed, return `ApexError::Instrumentation` with path, format, and detail in the message string.

## Stage 5: Source Map Remapping

Activates when `env.is_typescript || env.source_maps`.

```rust
fn remap_source_maps(
    branches: Vec<BranchId>,
    file_paths: &HashMap<u64, PathBuf>,
    target: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>)
```

Logic:

1. For each unique `file_id`, find `.map` sidecar or inline `sourceMappingURL`
2. Parse source map using the `sourcemap` crate (Source Map v3, VLQ-encoded)
3. For each `BranchId`, look up `(line, col)` → nearest mapped segment → original file/line/col
4. Produce new `BranchId` with FNV-1a of original TS path, original line/col
5. Update `file_paths` map accordingly
6. Branches with no source mapping (generated code) → dropped

Edge cases:

- Multiple JS files mapping to same TS file → merges naturally via `file_id` hash
- `sourceRoot` field in source maps — prepend to relative paths before hashing
- Missing source maps → skip remapping, use JS locations as-is (degraded but functional)
- Index maps (concatenated bundles) → the `sourcemap` crate handles these natively
- `.tsx`/`.jsx` files → treated identically to `.ts`/`.js` for remapping purposes

**Error handling**: Corrupt or missing `.map` files → log warning, skip remapping for that file (don't fail the whole run).

## Monorepo Support

1. Resolve workspace packages:
   - **npm/yarn**: parse `"workspaces"` globs from `package.json`, expand via glob matching
   - **pnpm**: parse `pnpm-workspace.yaml`, read `packages:` list
   - **Turborepo/Nx**: read the underlying workspace config (they delegate to npm/yarn/pnpm workspaces)
2. Per package: run stages 1-5, collect `Vec<BranchId>` with paths relative to **repo root**
3. Merge all results into single vector for oracle
4. Special case: root `jest.config.js` with `projects: [...]` → run once at root instead of per-package
5. Packages with no tests → skip silently
6. Mixed JS/TS packages → each gets its own environment detection

**Error handling**: Individual package failures don't fail the whole run. Log the error, skip the package, continue with others.

## Bun Runtime

- **Detection**: `bun.lockb` / `bunfig.toml`
- **Test**: `bun test` (Jest-compatible API)
- **Coverage**: `bun test --coverage` → V8 format, captured from stdout
- **Deps**: `bun install`
- **Source maps**: Bun's built-in TS transpiler generates them, same remap pipeline

## Concolic Execution for JS/TS

New `js_conditions.rs` in apex-concolic, parallel to existing Python condition parsing.

**Prerequisite**: The Python concolic module currently uses `BranchTrace` structs with raw condition strings — there is no shared `ConditionTree` IR yet. Phase 5 must first introduce a `ConditionTree` enum in apex-concolic as shared IR, then refactor the Python condition parser to emit it, then implement the JS/TS parser targeting the same IR. This is a three-step sub-phase.

**Pipeline:** JS/TS source → parse AST → extract branch conditions → `ConditionTree` IR → Z3 constraints → solve → new inputs

### Condition Mapping

| JS/TS Condition              | Z3 Mapping                      |
|------------------------------|----------------------------------|
| `x === 0`, `x !== null`     | Equality/disequality             |
| `x > 5 && x < 10`           | Conjunction of arithmetic        |
| `typeof x === "string"`     | Type tag enum constraint         |
| `x instanceof Error`        | Prototype chain → type tag       |
| `arr.length > 0`            | Integer property constraint      |
| `"key" in obj`              | Set membership                   |
| `x?.y`                      | Null-check + property access     |
| `switch(x) { case 1: ... }` | Disjunction of equalities       |

Operates on **original source** (TS or JS), not emitted JS. Branch conditions reference original line/col so `BranchId`s match remapped coverage data.

### Deferred (tracked in TODO.md)

- `eval()` / `new Function()` — not statically analyzable, needs runtime tracing
- Proxy/Reflect metaprogramming — intercepted property access creates invisible branches
- Async control flow constraints — Promise branching, `await` paths, race conditions

## Feature Matrix Update

`Language::JavaScript` row updates:

- `instrumentation`: Full (now covers TS, ESM, V8 format, Bun)
- `concolic`: Full (was Missing)
- All others remain Full

## File Changes

| Crate            | File                | Change                                                                 |
|------------------|---------------------|------------------------------------------------------------------------|
| apex-core        | `hash.rs`           | New — extract `fnv1a_hash()` as shared infrastructure; add `pub mod hash` to `lib.rs` |
| apex-core        | `types.rs`          | Add `JsRuntime`, `ModuleSystem`, `MonorepoKind` enums; add `"ts"`/`"typescript"` to `Language::FromStr`; update feature matrix; update `BranchId.direction` doc comment to "arm index" |
| apex-lang        | `js_env.rs`         | New — shared `JsEnvironment::detect()` logic, used by both runner and instrumentor |
| apex-instrument  | `javascript.rs`     | Refactor into 5-stage pipeline, delegate to new modules               |
| apex-instrument  | `v8_coverage.rs`    | New — V8 format parser + `OffsetIndex`                                |
| apex-instrument  | `source_map.rs`     | New — source map remapping via `sourcemap` crate                      |
| apex-lang        | `javascript.rs`     | Use shared `JsEnvironment` from `js_env.rs`, add Bun runtime/pkg manager |
| apex-sandbox     | `javascript.rs`     | Update Istanbul parser to also handle V8 format; use shared `fnv1a_hash` |
| apex-concolic    | `condition_tree.rs` | New — shared `ConditionTree` IR enum                                  |
| apex-concolic    | `js_conditions.rs`  | New — JS/TS condition parser emitting `ConditionTree`                 |
| apex-concolic    | `python.rs`         | Refactor to emit `ConditionTree` IR instead of raw strings            |
| apex-cli         | `lib.rs`            | No structural changes — existing JS dispatch covers TS sub-mode       |
| Cargo.toml       | apex-instrument     | Add `sourcemap` crate dependency                                      |

## Phasing

1. **Phase 1**: TS sub-mode detection + environment struct (`JsEnvironment` in apex-lang/js_env.rs, `"ts"` alias, `fnv1a_hash` extraction, `BranchId.direction` doc update)
2. **Phase 2**: V8 coverage parser + `OffsetIndex` + c8/Vitest support + ESM handling
3. **Phase 3**: Source map remapping (depends on Phase 2 for V8 offset→line/col conversion)
4. **Phase 4**: Bun runtime + monorepo support
5. **Phase 5**: Concolic — `ConditionTree` IR, refactor Python parser, then JS/TS condition parser
