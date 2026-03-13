# JS/TS Support Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend APEX JavaScript support with TypeScript sub-mode, V8 coverage format, ESM/Bun/monorepo, source maps, and JS/TS concolic execution.

**Architecture:** Five-stage instrumentation pipeline (detect_environment → select_coverage_tool → run_under_coverage → parse_coverage → remap_source_maps). TypeScript handled as sub-mode of `Language::JavaScript`. V8 as primary coverage format, Istanbul as legacy fallback.

**Tech Stack:** Rust, `sourcemap` crate, V8 coverage JSON, Istanbul JSON, Source Map v3 spec.

**Spec:** `docs/superpowers/specs/2026-03-14-js-ts-support-design.md`

---

## Chunk 1: Foundation (Tasks 1-3)

### Task 1: Extract `fnv1a_hash` to apex-core

**Files:**
- Create: `crates/apex-core/src/hash.rs`
- Modify: `crates/apex-core/src/lib.rs`
- Modify: `crates/apex-instrument/src/javascript.rs`
- Modify: `crates/apex-instrument/src/python.rs`
- Modify: `crates/apex-sandbox/src/javascript.rs`

- [ ] **Step 1: Create `apex-core/src/hash.rs` with test**

```rust
// crates/apex-core/src/hash.rs

/// FNV-1a hash — stable file_id from repo-relative path strings.
pub fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        assert_eq!(fnv1a_hash("src/index.js"), fnv1a_hash("src/index.js"));
    }

    #[test]
    fn empty_returns_offset_basis() {
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn different_strings_differ() {
        assert_ne!(fnv1a_hash("a.js"), fnv1a_hash("b.js"));
    }
}
```

- [ ] **Step 2: Register module in `apex-core/src/lib.rs`**

Add `pub mod hash;` after the existing `pub mod fixture_runner;` line (line 9 of `crates/apex-core/src/lib.rs`).

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p apex-core hash`
Expected: 3 tests pass.

- [ ] **Step 4: Replace duplicates in apex-instrument and apex-sandbox**

In `crates/apex-instrument/src/javascript.rs`:
- Remove the `fn fnv1a_hash` function (lines 20-27)
- Add `use apex_core::hash::fnv1a_hash;` to the imports

In `crates/apex-instrument/src/python.rs`:
- Find and remove the local `fn fnv1a_hash` function
- Add `use apex_core::hash::fnv1a_hash;` to the imports

In `crates/apex-sandbox/src/javascript.rs`:
- Remove the `fn fnv1a_hash` function (lines 14-21)
- Add `use apex_core::hash::fnv1a_hash;` to the imports

- [ ] **Step 5: Run full test suite to verify no regressions**

Run: `cargo test -p apex-instrument -p apex-sandbox -p apex-core`
Expected: All existing tests pass (the hash function is identical).

- [ ] **Step 6: Commit**

```bash
git add crates/apex-core/src/hash.rs crates/apex-core/src/lib.rs crates/apex-instrument/src/javascript.rs crates/apex-instrument/src/python.rs crates/apex-sandbox/src/javascript.rs
git commit -m "refactor: extract fnv1a_hash to apex-core::hash"
```

---

### Task 2: Add `"ts"`/`"typescript"` aliases + update `BranchId.direction` doc

**Files:**
- Modify: `crates/apex-core/src/types.rs`

- [ ] **Step 1: Write failing test for ts/typescript aliases**

Add to the `language_parse_aliases` test in `crates/apex-core/src/types.rs` (around line 618):

```rust
assert_eq!("ts".parse::<Language>().unwrap(), Language::JavaScript);
assert_eq!("typescript".parse::<Language>().unwrap(), Language::JavaScript);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core language_parse_aliases`
Expected: FAIL — "unknown language: ts"

- [ ] **Step 3: Add aliases to `FromStr` impl**

In `crates/apex-core/src/types.rs`, in the `from_str` match (line 165), change:

```rust
"javascript" | "js" | "node" => Ok(Language::JavaScript),
```

to:

```rust
"javascript" | "js" | "node" | "ts" | "typescript" => Ok(Language::JavaScript),
```

- [ ] **Step 4: Update `BranchId.direction` doc comment**

In `crates/apex-core/src/types.rs` (line 51), change:

```rust
/// 0 = taken / true branch, 1 = not-taken / false branch.
```

to:

```rust
/// Arm index within a branch point (0, 1, 2, ...).
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-core`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add crates/apex-core/src/types.rs
git commit -m "feat: add ts/typescript aliases to Language::FromStr, update direction doc"
```

---

### Task 3: Create `JsEnvironment` in apex-lang

**Files:**
- Create: `crates/apex-lang/src/js_env.rs`
- Modify: `crates/apex-lang/src/lib.rs`
- Modify: `crates/apex-lang/src/javascript.rs`

- [ ] **Step 1: Write `js_env.rs` with types and detection logic**

```rust
// crates/apex-lang/src/js_env.rs

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsRuntime {
    Node,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgManager {
    Npm,
    Yarn,
    Pnpm,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTestRunner {
    Jest,
    Mocha,
    Vitest,
    BunTest,
    NpmScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSystem {
    CommonJS,
    ESM,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonorepoKind {
    NpmWorkspaces,
    Yarn,
    Pnpm,
    Turborepo,
    Nx,
}

#[derive(Debug, Clone)]
pub struct JsEnvironment {
    pub runtime: JsRuntime,
    pub pkg_manager: PkgManager,
    pub test_runner: JsTestRunner,
    pub module_system: ModuleSystem,
    pub is_typescript: bool,
    pub source_maps: bool,
    pub monorepo: Option<MonorepoKind>,
}

impl JsEnvironment {
    /// Detect the JS/TS project environment from the filesystem.
    pub fn detect(target: &Path) -> Option<JsEnvironment> {
        if !target.join("package.json").exists() {
            return None;
        }

        let runtime = detect_runtime(target);
        let pkg_manager = detect_pkg_manager(target, runtime);
        let test_runner = detect_test_runner(target);
        let module_system = detect_module_system(target);
        let is_typescript = detect_typescript(target);
        let source_maps = is_typescript; // TS always produces source maps
        let monorepo = detect_monorepo(target);

        Some(JsEnvironment {
            runtime,
            pkg_manager,
            test_runner,
            module_system,
            is_typescript,
            source_maps,
            monorepo,
        })
    }
}

fn detect_runtime(target: &Path) -> JsRuntime {
    if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
        JsRuntime::Bun
    } else {
        JsRuntime::Node
    }
}

fn detect_pkg_manager(target: &Path, runtime: JsRuntime) -> PkgManager {
    if runtime == JsRuntime::Bun {
        return PkgManager::Bun;
    }
    if target.join("yarn.lock").exists() {
        return PkgManager::Yarn;
    }
    if target.join("pnpm-lock.yaml").exists() {
        return PkgManager::Pnpm;
    }
    PkgManager::Npm
}

/// Detect test runner from package.json content.
pub fn detect_test_runner(target: &Path) -> JsTestRunner {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();

    if detect_runtime(target) == JsRuntime::Bun {
        // Bun projects may still use Jest/Vitest, check first
        if pkg_content.contains("\"vitest\"") {
            return JsTestRunner::Vitest;
        }
        return JsTestRunner::BunTest;
    }

    if pkg_content.contains("\"jest\"") {
        return JsTestRunner::Jest;
    }
    if pkg_content.contains("\"mocha\"") {
        return JsTestRunner::Mocha;
    }
    if pkg_content.contains("\"vitest\"") {
        return JsTestRunner::Vitest;
    }
    if pkg_content.contains("\"scripts\"") && pkg_content.contains("\"test\"") {
        return JsTestRunner::NpmScript;
    }
    JsTestRunner::Jest // default fallback
}

fn detect_module_system(target: &Path) -> ModuleSystem {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();
    let has_type_module = pkg_content.contains("\"type\": \"module\"")
        || pkg_content.contains("\"type\":\"module\"");

    // Quick scan for .mjs or .cjs files in src/
    let src_dir = target.join("src");
    let has_mjs = src_dir.join("index.mjs").exists();
    let has_cjs = src_dir.join("index.cjs").exists();

    match (has_type_module, has_mjs, has_cjs) {
        (true, _, true) => ModuleSystem::Mixed,
        (true, _, _) => ModuleSystem::ESM,
        (false, true, _) => ModuleSystem::Mixed,
        _ => ModuleSystem::CommonJS,
    }
}

fn detect_typescript(target: &Path) -> bool {
    // Check for tsconfig*.json at target root
    if let Ok(entries) = std::fs::read_dir(target) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("tsconfig") && name_str.ends_with(".json") {
                return true;
            }
        }
    }
    // Fallback: check for .ts/.tsx files in src/
    let src_dir = target.join("src");
    if let Ok(entries) = std::fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".ts") || name_str.ends_with(".tsx") {
                return true;
            }
        }
    }
    false
}

fn detect_monorepo(target: &Path) -> Option<MonorepoKind> {
    // Check for workspace indicators
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();

    if target.join("nx.json").exists() {
        return Some(MonorepoKind::Nx);
    }
    if target.join("turbo.json").exists() {
        return Some(MonorepoKind::Turborepo);
    }
    if target.join("pnpm-workspace.yaml").exists() {
        return Some(MonorepoKind::Pnpm);
    }
    if pkg_content.contains("\"workspaces\"") {
        // Could be npm or yarn — check for yarn.lock
        if target.join("yarn.lock").exists() {
            return Some(MonorepoKind::Yarn);
        }
        return Some(MonorepoKind::NpmWorkspaces);
    }
    None
}

/// Return the test command for the given environment.
pub fn test_command(env: &JsEnvironment) -> (String, Vec<String>) {
    match env.test_runner {
        JsTestRunner::Jest => (
            "npx".to_string(),
            vec!["jest".to_string(), "--passWithNoTests".to_string()],
        ),
        JsTestRunner::Mocha => ("npx".to_string(), vec!["mocha".to_string()]),
        JsTestRunner::Vitest => (
            "npx".to_string(),
            vec!["vitest".to_string(), "run".to_string()],
        ),
        JsTestRunner::BunTest => ("bun".to_string(), vec!["test".to_string()]),
        JsTestRunner::NpmScript => ("npm".to_string(), vec!["test".to_string()]),
    }
}

/// Return the install command for the given environment.
pub fn install_command(env: &JsEnvironment) -> &'static str {
    match env.pkg_manager {
        PkgManager::Npm => "npm",
        PkgManager::Yarn => "yarn",
        PkgManager::Pnpm => "pnpm",
        PkgManager::Bun => "bun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_none_without_package_json() {
        let dir = tempdir().unwrap();
        assert!(JsEnvironment::detect(dir.path()).is_none());
    }

    #[test]
    fn detect_basic_npm_project() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "test", "devDependencies": {"jest": "^29"}}"#,
        ).unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Node);
        assert_eq!(env.pkg_manager, PkgManager::Npm);
        assert_eq!(env.test_runner, JsTestRunner::Jest);
        assert_eq!(env.module_system, ModuleSystem::CommonJS);
        assert!(!env.is_typescript);
        assert!(env.monorepo.is_none());
    }

    #[test]
    fn detect_typescript_via_tsconfig() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts-proj"}"#).unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
        assert!(env.source_maps);
    }

    #[test]
    fn detect_typescript_via_tsconfig_build() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts"}"#).unwrap();
        std::fs::write(dir.path().join("tsconfig.build.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
    }

    #[test]
    fn detect_typescript_via_ts_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts"}"#).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/index.ts"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
    }

    #[test]
    fn detect_bun_runtime() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "bun-proj"}"#).unwrap();
        std::fs::write(dir.path().join("bun.lockb"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Bun);
        assert_eq!(env.pkg_manager, PkgManager::Bun);
        assert_eq!(env.test_runner, JsTestRunner::BunTest);
    }

    #[test]
    fn detect_yarn_pkg_manager() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "yarn-proj", "devDependencies": {"jest": "^29"}}"#,
        ).unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.pkg_manager, PkgManager::Yarn);
    }

    #[test]
    fn detect_pnpm_pkg_manager() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "pnpm"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.pkg_manager, PkgManager::Pnpm);
    }

    #[test]
    fn detect_esm_module_system() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "esm", "type": "module"}"#,
        ).unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.module_system, ModuleSystem::ESM);
    }

    #[test]
    fn detect_vitest_runner() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        ).unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.test_runner, JsTestRunner::Vitest);
    }

    #[test]
    fn detect_npm_workspaces_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "root", "workspaces": ["packages/*"]}"#,
        ).unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::NpmWorkspaces));
    }

    #[test]
    fn detect_turborepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("turbo.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Turborepo));
    }

    #[test]
    fn detect_nx_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("nx.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Nx));
    }

    #[test]
    fn detect_pnpm_workspace_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Pnpm));
    }

    #[test]
    fn test_command_jest() {
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["jest", "--passWithNoTests"]);
    }

    #[test]
    fn test_command_bun() {
        let env = JsEnvironment {
            runtime: JsRuntime::Bun,
            pkg_manager: PkgManager::Bun,
            test_runner: JsTestRunner::BunTest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "bun");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn install_command_variants() {
        assert_eq!(install_command(&JsEnvironment {
            runtime: JsRuntime::Node, pkg_manager: PkgManager::Npm,
            test_runner: JsTestRunner::Jest, module_system: ModuleSystem::CommonJS,
            is_typescript: false, source_maps: false, monorepo: None,
        }), "npm");

        assert_eq!(install_command(&JsEnvironment {
            runtime: JsRuntime::Bun, pkg_manager: PkgManager::Bun,
            test_runner: JsTestRunner::BunTest, module_system: ModuleSystem::ESM,
            is_typescript: false, source_maps: false, monorepo: None,
        }), "bun");
    }
}
```

- [ ] **Step 2: Register module in `apex-lang/src/lib.rs`**

Add `pub mod js_env;` after `pub mod javascript;` and add `pub use js_env::JsEnvironment;`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-lang js_env`
Expected: All tests pass.

- [ ] **Step 4: Refactor `JavaScriptRunner` to use `JsEnvironment`**

In `crates/apex-lang/src/javascript.rs`:

Replace `detect_test_runner` and `detect_package_manager` methods with calls to `js_env`:

```rust
use crate::js_env;

// In detect_test_runner, delegate:
fn detect_test_runner(target: &Path) -> (String, Vec<String>) {
    let runner = js_env::detect_test_runner(target);
    // Convert to the (binary, args) format that existing callers expect
    let env = JsEnvironment {
        runtime: if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
            js_env::JsRuntime::Bun
        } else {
            js_env::JsRuntime::Node
        },
        pkg_manager: js_env::PkgManager::Npm, // not used by test_command
        test_runner: runner,
        module_system: js_env::ModuleSystem::CommonJS, // not used by test_command
        is_typescript: false,
        source_maps: false,
        monorepo: None,
    };
    js_env::test_command(&env)
}

// In detect_package_manager, delegate:
fn detect_package_manager(target: &Path) -> &'static str {
    if let Some(env) = JsEnvironment::detect(target) {
        js_env::install_command(&env)
    } else {
        "npm"
    }
}
```

- [ ] **Step 5: Run full apex-lang tests to verify no regressions**

Run: `cargo test -p apex-lang`
Expected: All existing JavaScript runner tests still pass.

- [ ] **Step 6: Commit**

```bash
git add crates/apex-lang/src/js_env.rs crates/apex-lang/src/lib.rs crates/apex-lang/src/javascript.rs
git commit -m "feat: add JsEnvironment detection in apex-lang, refactor JS runner"
```

---

## Chunk 2: V8 Coverage Parser (Tasks 4-5)

### Task 4: V8 Coverage Parser + OffsetIndex

**Files:**
- Create: `crates/apex-instrument/src/v8_coverage.rs`
- Modify: `crates/apex-instrument/src/lib.rs`

- [ ] **Step 1: Write V8 parser with OffsetIndex**

```rust
// crates/apex-instrument/src/v8_coverage.rs

use apex_core::{hash::fnv1a_hash, types::BranchId};
use serde::Deserialize;
use std::{collections::HashMap, path::{Path, PathBuf}};

// ---------------------------------------------------------------------------
// V8 coverage JSON schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct V8CoverageResult {
    pub result: Vec<V8ScriptCoverage>,
}

#[derive(Debug, Deserialize)]
pub struct V8ScriptCoverage {
    pub url: String,
    pub functions: Vec<V8FunctionCoverage>,
}

#[derive(Debug, Deserialize)]
pub struct V8FunctionCoverage {
    pub ranges: Vec<V8CoverageRange>,
}

#[derive(Debug, Deserialize)]
pub struct V8CoverageRange {
    #[serde(rename = "startOffset")]
    pub start_offset: usize,
    #[serde(rename = "endOffset")]
    pub end_offset: usize,
    pub count: u64,
}

// ---------------------------------------------------------------------------
// OffsetIndex — byte offset → (line, col) for a source file
// ---------------------------------------------------------------------------

pub struct OffsetIndex {
    /// Byte offsets of each newline character. line_offsets[i] is the byte
    /// offset of the newline ending line i (0-indexed).
    line_starts: Vec<usize>,
}

impl OffsetIndex {
    /// Build from source text in a single pass.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize]; // line 0 starts at offset 0
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        OffsetIndex { line_starts }
    }

    /// Convert byte offset to (line, col). Both are 1-based for BranchId compatibility.
    pub fn offset_to_line_col(&self, offset: usize) -> (u32, u16) {
        // Binary search for the line containing this offset.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let col = offset.saturating_sub(self.line_starts[line_idx]);
        let line = (line_idx + 1) as u32; // 1-based
        let col = col.min(u16::MAX as usize) as u16; // saturate for minified files
        (line, col)
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Convert a `file://` URL to a repo-relative path.
pub fn url_to_repo_relative(url: &str, repo_root: &Path) -> Option<PathBuf> {
    let path_str = url.strip_prefix("file://")?;
    let abs_path = Path::new(path_str);
    let rel = abs_path
        .strip_prefix(repo_root)
        .unwrap_or(abs_path);
    Some(rel.to_path_buf())
}

/// Parse V8 coverage JSON into branch IDs.
///
/// `source_loader` is called with the absolute file path to load source text
/// for offset→line/col conversion. Return None to skip that file.
pub fn parse_v8_coverage(
    json_str: &str,
    repo_root: &Path,
    source_loader: &dyn Fn(&Path) -> Option<String>,
) -> Result<(Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>), String> {
    let data: V8CoverageResult =
        serde_json::from_str(json_str).map_err(|e| format!("parse V8 JSON: {e}"))?;

    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();
    let mut file_paths = HashMap::new();

    for script in &data.result {
        let Some(rel_path) = url_to_repo_relative(&script.url, repo_root) else {
            continue;
        };
        let rel_str = rel_path.to_string_lossy();
        let file_id = fnv1a_hash(&rel_str);
        file_paths.insert(file_id, rel_path);

        // Load source for offset→line/col. Skip if unavailable.
        let abs_path = repo_root.join(&rel_str);
        let Some(source) = source_loader(&abs_path) else {
            continue;
        };
        let index = OffsetIndex::new(&source);

        for func in &script.functions {
            // Find sibling groups: ranges that share a parent (same nesting level).
            // V8 ranges are sorted by startOffset. The first range is the function body.
            // Inner ranges at the same depth are siblings.
            let branch_points = extract_branch_points(&func.ranges);

            for group in &branch_points {
                if group.len() < 2 {
                    continue; // single-child ranges are not branches
                }
                for (direction, range_idx) in group.iter().enumerate() {
                    let range = &func.ranges[*range_idx];
                    let (line, col) = index.offset_to_line_col(range.start_offset);
                    let dir = (direction).min(u8::MAX as usize) as u8;
                    let bid = BranchId::new(file_id, line, col, dir);
                    all_branches.push(bid.clone());
                    if range.count > 0 {
                        executed_branches.push(bid);
                    }
                }
            }
        }
    }

    Ok((all_branches, executed_branches, file_paths))
}

/// Extract sibling groups from V8 function ranges.
///
/// V8 ranges are nested. The outermost range covers the whole function.
/// Inner ranges at the same nesting level are siblings (alternative branches).
/// Returns a Vec of groups, where each group is a Vec of range indices.
fn extract_branch_points(ranges: &[V8CoverageRange]) -> Vec<Vec<usize>> {
    if ranges.len() < 2 {
        return Vec::new();
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current_group: Vec<usize> = Vec::new();
    let mut parent_end: usize = ranges[0].end_offset;

    // Skip index 0 (function body range). Process inner ranges.
    for i in 1..ranges.len() {
        let range = &ranges[i];
        if range.start_offset >= parent_end {
            // This range is outside the current parent — start new context
            if current_group.len() >= 2 {
                groups.push(current_group.clone());
            }
            current_group.clear();
            parent_end = range.end_offset;
        }
        current_group.push(i);
    }

    if current_group.len() >= 2 {
        groups.push(current_group);
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_index_simple() {
        let source = "line1\nline2\nline3";
        let idx = OffsetIndex::new(source);
        assert_eq!(idx.offset_to_line_col(0), (1, 0)); // start of line 1
        assert_eq!(idx.offset_to_line_col(3), (1, 3)); // middle of line 1
        assert_eq!(idx.offset_to_line_col(6), (2, 0)); // start of line 2
        assert_eq!(idx.offset_to_line_col(12), (3, 0)); // start of line 3
    }

    #[test]
    fn offset_index_empty_source() {
        let idx = OffsetIndex::new("");
        assert_eq!(idx.offset_to_line_col(0), (1, 0));
    }

    #[test]
    fn offset_index_col_saturates() {
        // Simulate a minified line longer than u16::MAX
        let long_line = "x".repeat(70000);
        let idx = OffsetIndex::new(&long_line);
        let (line, col) = idx.offset_to_line_col(66000);
        assert_eq!(line, 1);
        assert_eq!(col, u16::MAX); // saturated
    }

    #[test]
    fn url_to_repo_relative_strips_prefix() {
        let repo = Path::new("/home/user/project");
        let url = "file:///home/user/project/src/index.js";
        let rel = url_to_repo_relative(url, repo).unwrap();
        assert_eq!(rel, PathBuf::from("src/index.js"));
    }

    #[test]
    fn url_to_repo_relative_non_file_url() {
        let repo = Path::new("/project");
        assert!(url_to_repo_relative("https://example.com/file.js", repo).is_none());
    }

    #[test]
    fn url_to_repo_relative_outside_repo() {
        let repo = Path::new("/project");
        let url = "file:///other/path/file.js";
        let rel = url_to_repo_relative(url, repo).unwrap();
        // Falls back to full path
        assert_eq!(rel, PathBuf::from("/other/path/file.js"));
    }

    #[test]
    fn parse_v8_simple_branch() {
        let json = r#"{
            "result": [{
                "url": "file:///repo/src/app.js",
                "functions": [{
                    "functionName": "main",
                    "ranges": [
                        {"startOffset": 0, "endOffset": 100, "count": 1},
                        {"startOffset": 10, "endOffset": 50, "count": 1},
                        {"startOffset": 50, "endOffset": 90, "count": 0}
                    ]
                }]
            }]
        }"#;
        let source = "if (x) {\n  doA();\n} else {\n  doB();\n}\n".repeat(3);
        let repo = Path::new("/repo");
        let (all, exec, files) =
            parse_v8_coverage(json, repo, &|_| Some(source.clone())).unwrap();

        assert_eq!(files.len(), 1);
        assert!(files.values().any(|p| p == Path::new("src/app.js")));
        assert_eq!(all.len(), 2); // two sibling ranges = branch with 2 arms
        assert_eq!(exec.len(), 1); // only first arm was hit
    }

    #[test]
    fn parse_v8_no_branches_single_range() {
        let json = r#"{
            "result": [{
                "url": "file:///repo/src/simple.js",
                "functions": [{
                    "functionName": "noop",
                    "ranges": [
                        {"startOffset": 0, "endOffset": 50, "count": 1}
                    ]
                }]
            }]
        }"#;
        let repo = Path::new("/repo");
        let (all, _, _) = parse_v8_coverage(json, repo, &|_| Some("noop".into())).unwrap();
        assert!(all.is_empty()); // single range = no branches
    }

    #[test]
    fn parse_v8_invalid_json() {
        let result = parse_v8_coverage("not json", Path::new("/"), &|_| None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_v8_skips_files_without_source() {
        let json = r#"{"result": [{"url": "file:///repo/x.js", "functions": []}]}"#;
        let repo = Path::new("/repo");
        let (all, _, files) = parse_v8_coverage(json, repo, &|_| None).unwrap();
        assert!(all.is_empty());
        assert_eq!(files.len(), 1); // file registered even without source
    }

    #[test]
    fn extract_branch_points_two_siblings() {
        let ranges = vec![
            V8CoverageRange { start_offset: 0, end_offset: 100, count: 1 },
            V8CoverageRange { start_offset: 10, end_offset: 50, count: 1 },
            V8CoverageRange { start_offset: 50, end_offset: 90, count: 0 },
        ];
        let groups = extract_branch_points(&ranges);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![1, 2]);
    }

    #[test]
    fn extract_branch_points_no_inner_ranges() {
        let ranges = vec![
            V8CoverageRange { start_offset: 0, end_offset: 100, count: 1 },
        ];
        let groups = extract_branch_points(&ranges);
        assert!(groups.is_empty());
    }

    #[test]
    fn direction_saturates_at_u8_max() {
        // Build JSON with 260 sibling ranges — direction should saturate
        let mut ranges_json = String::from(
            r#"{"startOffset": 0, "endOffset": 10000, "count": 1}"#
        );
        for i in 0..260u32 {
            let start = (i + 1) * 10;
            let end = start + 9;
            ranges_json.push_str(&format!(
                r#", {{"startOffset": {start}, "endOffset": {end}, "count": 1}}"#
            ));
        }
        let json = format!(
            r#"{{"result": [{{"url": "file:///repo/big.js", "functions": [{{"functionName": "f", "ranges": [{ranges_json}]}}]}}]}}"#
        );
        let repo = Path::new("/repo");
        let source = " ".repeat(11000);
        let (all, _, _) = parse_v8_coverage(&json, repo, &|_| Some(source.clone())).unwrap();
        // Check that direction values don't panic and are <= 255
        for bid in &all {
            assert!(bid.direction <= u8::MAX);
        }
    }
}
```

- [ ] **Step 2: Register module in `apex-instrument/src/lib.rs`**

Add `pub mod v8_coverage;` after `pub mod wasm;`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-instrument v8_coverage`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-instrument/src/v8_coverage.rs crates/apex-instrument/src/lib.rs
git commit -m "feat: add V8 coverage parser with OffsetIndex"
```

---

### Task 5: Coverage Tool Selection + Pipeline Refactor

**Files:**
- Modify: `crates/apex-instrument/src/javascript.rs`

- [ ] **Step 1: Add coverage tool types and selection logic**

At the top of `crates/apex-instrument/src/javascript.rs`, after the existing imports, add:

```rust
use crate::v8_coverage;
use apex_lang::js_env::{self, JsEnvironment, JsRuntime, JsTestRunner, ModuleSystem, CoverageTool};
```

Wait — `CoverageTool` and related types belong in the instrumentor since they're instrumentation-specific. Add these types to `javascript.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoverageTool {
    Nyc,
    C8,
    Vitest,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoverageFormat {
    V8,
    Istanbul,
}

#[derive(Debug)]
enum CoverageOutput {
    FilePath(PathBuf),
    Stdout,
}

struct CoverageToolConfig {
    tool: CoverageTool,
    command: Vec<String>,
    output_path: CoverageOutput,
    format: CoverageFormat,
}

fn select_coverage_tool(env: &JsEnvironment, target: &Path) -> CoverageToolConfig {
    match env.runtime {
        JsRuntime::Bun => CoverageToolConfig {
            tool: CoverageTool::Bun,
            command: vec!["bun".into(), "test".into(), "--coverage".into()],
            output_path: CoverageOutput::Stdout,
            format: CoverageFormat::V8,
        },
        JsRuntime::Node => {
            if env.test_runner == JsTestRunner::Vitest {
                // Check for @vitest/coverage-v8
                let has_vitest_v8 = target
                    .join("node_modules/@vitest/coverage-v8")
                    .exists();
                if has_vitest_v8 {
                    let report_dir = target.join(".apex_coverage_js");
                    return CoverageToolConfig {
                        tool: CoverageTool::Vitest,
                        command: vec![
                            "npx".into(), "vitest".into(), "run".into(),
                            "--coverage".into(),
                            "--coverage.reporter=v8".into(),
                            format!("--coverage.reportsDirectory={}", report_dir.display()),
                        ],
                        output_path: CoverageOutput::FilePath(report_dir.join("coverage-final.json")),
                        format: CoverageFormat::V8,
                    };
                }
            }

            match env.module_system {
                ModuleSystem::ESM | ModuleSystem::Mixed => {
                    // c8 for ESM
                    let report_dir = target.join(".apex_coverage_js");
                    CoverageToolConfig {
                        tool: CoverageTool::C8,
                        command: {
                            let (bin, args) = js_env::test_command(env);
                            let mut cmd = vec![
                                "npx".into(), "c8".into(),
                                "--reporter=json".into(),
                                format!("--reports-dir={}", report_dir.display()),
                                bin,
                            ];
                            cmd.extend(args);
                            cmd
                        },
                        output_path: CoverageOutput::FilePath(report_dir.join("coverage-final.json")),
                        format: CoverageFormat::V8,
                    }
                }
                ModuleSystem::CommonJS => {
                    // Check if nyc is available
                    let has_nyc = target.join("node_modules/.bin/nyc").exists();
                    if has_nyc {
                        // Use existing nyc path
                        let report_dir = target.join(".apex_coverage_js");
                        CoverageToolConfig {
                            tool: CoverageTool::Nyc,
                            command: {
                                let (bin, args) = js_env::test_command(env);
                                let mut cmd = vec![
                                    "npx".into(), "nyc".into(),
                                    "--reporter=json".into(),
                                    format!("--report-dir={}", report_dir.display()),
                                    "--temp-dir=.nyc_output".into(),
                                    "--include=**/*.js".into(),
                                    "--exclude=node_modules/**".into(),
                                    bin,
                                ];
                                cmd.extend(args);
                                cmd
                            },
                            output_path: CoverageOutput::FilePath(report_dir.join("coverage-final.json")),
                            format: CoverageFormat::Istanbul,
                        }
                    } else {
                        // Fallback to c8
                        let report_dir = target.join(".apex_coverage_js");
                        CoverageToolConfig {
                            tool: CoverageTool::C8,
                            command: {
                                let (bin, args) = js_env::test_command(env);
                                let mut cmd = vec![
                                    "npx".into(), "c8".into(),
                                    "--reporter=json".into(),
                                    format!("--reports-dir={}", report_dir.display()),
                                    bin,
                                ];
                                cmd.extend(args);
                                cmd
                            },
                            output_path: CoverageOutput::FilePath(report_dir.join("coverage-final.json")),
                            format: CoverageFormat::V8,
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: Refactor `instrument()` to use 5-stage pipeline**

Replace the `instrument()` method body in the `Instrumentor` impl:

```rust
#[async_trait]
impl Instrumentor for JavaScriptInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        // Stage 1: Detect environment
        let env = JsEnvironment::detect(&target.root).ok_or_else(|| {
            ApexError::Instrumentation(
                "project not detected: expected package.json in target root".into(),
            )
        })?;

        info!(
            runtime = ?env.runtime,
            typescript = env.is_typescript,
            module_system = ?env.module_system,
            test_runner = ?env.test_runner,
            "detected JS environment"
        );

        // Stage 2: Select coverage tool
        let config = select_coverage_tool(&env, &target.root);
        info!(tool = ?config.tool, format = ?config.format, "selected coverage tool");

        // Stage 3: Run under coverage
        let report_dir = target.root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir)
            .map_err(|e| ApexError::Instrumentation(format!("create report dir: {e}")))?;

        let effective_cmd = if target.test_command.is_empty() {
            config.command.clone()
        } else {
            // User provided custom test command — wrap it with coverage tool
            match config.tool {
                CoverageTool::Nyc => {
                    let mut cmd = vec![
                        "npx".into(), "nyc".into(),
                        "--reporter=json".into(),
                        format!("--report-dir={}", report_dir.display()),
                        "--temp-dir=.nyc_output".into(),
                    ];
                    cmd.extend(target.test_command.clone());
                    cmd
                }
                CoverageTool::C8 => {
                    let mut cmd = vec![
                        "npx".into(), "c8".into(),
                        "--reporter=json".into(),
                        format!("--reports-dir={}", report_dir.display()),
                    ];
                    cmd.extend(target.test_command.clone());
                    cmd
                }
                _ => config.command.clone(),
            }
        };

        let spec = CommandSpec::new(&effective_cmd[0], &target.root)
            .args(&effective_cmd[1..]);
        let output = self.runner.run_command(&spec).await
            .map_err(|e| ApexError::Instrumentation(format!("spawn coverage tool: {e}")))?;

        if output.exit_code != 0 {
            warn!(
                exit = output.exit_code,
                "coverage/test run returned non-zero (coverage data may still be valid)"
            );
        }

        // Stage 4: Parse coverage
        let (branch_ids, executed_branch_ids, file_paths) = match config.format {
            CoverageFormat::Istanbul => {
                let json_path = match &config.output_path {
                    CoverageOutput::FilePath(p) => p.clone(),
                    CoverageOutput::Stdout => unreachable!("Istanbul always writes to file"),
                };
                if !json_path.exists() {
                    return Err(ApexError::Instrumentation(
                        format!("coverage-final.json not produced at {}; is the coverage tool installed?", json_path.display()),
                    ));
                }
                let mut inner = JavaScriptInstrumentor::with_runner(self.runner.clone());
                inner.parse_istanbul_json(&json_path, &target.root)?;
                (inner.branch_ids, inner.executed_branch_ids, inner.file_paths)
            }
            CoverageFormat::V8 => {
                let json_str = match &config.output_path {
                    CoverageOutput::FilePath(p) => {
                        if !p.exists() {
                            return Err(ApexError::Instrumentation(
                                format!("V8 coverage JSON not produced at {}", p.display()),
                            ));
                        }
                        std::fs::read_to_string(p)
                            .map_err(|e| ApexError::Instrumentation(format!("read V8 coverage: {e}")))?
                    }
                    CoverageOutput::Stdout => {
                        String::from_utf8_lossy(&output.stdout).to_string()
                    }
                };
                v8_coverage::parse_v8_coverage(
                    &json_str,
                    &target.root,
                    &|path| std::fs::read_to_string(path).ok(),
                ).map_err(|e| ApexError::Instrumentation(e))?
            }
        };

        // Stage 5: Source map remapping (Phase 3 — stubbed for now)
        // TODO: Implement source map remapping

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir: target.root.clone(),
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }
}
```

- [ ] **Step 3: Run existing tests to verify Istanbul path still works**

Run: `cargo test -p apex-instrument -- javascript`
Expected: All existing tests pass (they use Istanbul/nyc mocks).

- [ ] **Step 4: Commit**

```bash
git add crates/apex-instrument/src/javascript.rs
git commit -m "feat: refactor JS instrumentor to 5-stage pipeline with V8 + tool selection"
```

---

## Chunk 3: Source Map Remapping (Task 6)

### Task 6: Source Map Remapping

**Files:**
- Create: `crates/apex-instrument/src/source_map.rs`
- Modify: `crates/apex-instrument/src/lib.rs`
- Modify: `crates/apex-instrument/Cargo.toml`

- [ ] **Step 1: Add `sourcemap` crate dependency**

In `crates/apex-instrument/Cargo.toml`, add to `[dependencies]`:

```toml
sourcemap = "9"
```

- [ ] **Step 2: Write source map remapping module**

```rust
// crates/apex-instrument/src/source_map.rs

use apex_core::{hash::fnv1a_hash, types::BranchId};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::warn;

/// Remap branch IDs from emitted JS locations to original TS/source locations.
///
/// Returns updated branches and file_paths map.
pub fn remap_source_maps(
    branches: Vec<BranchId>,
    file_paths: &HashMap<u64, PathBuf>,
    target: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut remapped_branches = Vec::new();
    let mut remapped_file_paths = HashMap::new();

    // Pre-load source maps for each unique file_id
    let mut source_maps: HashMap<u64, Option<sourcemap::SourceMap>> = HashMap::new();
    for (&file_id, rel_path) in file_paths {
        let abs_path = target.join(rel_path);
        let sm = load_source_map(&abs_path);
        source_maps.insert(file_id, sm);
    }

    for branch in branches {
        let sm_opt = source_maps.get(&branch.file_id).and_then(|s| s.as_ref());

        if let Some(sm) = sm_opt {
            // BranchId uses 1-based line; sourcemap crate uses 0-based
            let line_0 = branch.line.saturating_sub(1);
            let col = branch.col as u32;

            if let Some(token) = sm.lookup_token(line_0, col) {
                if let Some(source) = token.get_source() {
                    // Resolve source path relative to source map's sourceRoot
                    let source_root = sm
                        .get_source_root()
                        .unwrap_or("");
                    let original_path = if source_root.is_empty() {
                        PathBuf::from(source)
                    } else {
                        PathBuf::from(source_root).join(source)
                    };

                    let original_rel = original_path.to_string_lossy();
                    let new_file_id = fnv1a_hash(&original_rel);
                    let new_line = token.get_src_line() + 1; // back to 1-based
                    let new_col = token.get_src_col().min(u16::MAX as u32) as u16;

                    remapped_file_paths
                        .insert(new_file_id, original_path);

                    remapped_branches.push(BranchId::new(
                        new_file_id,
                        new_line,
                        new_col,
                        branch.direction,
                    ));
                    continue;
                }
            }
            // Source map exists but no mapping found — generated code, drop it
        } else {
            // No source map — keep original location
            if let Some(path) = file_paths.get(&branch.file_id) {
                remapped_file_paths.insert(branch.file_id, path.clone());
            }
            remapped_branches.push(branch);
        }
    }

    (remapped_branches, remapped_file_paths)
}

/// Try to load a source map for the given JS file.
fn load_source_map(js_path: &Path) -> Option<sourcemap::SourceMap> {
    // Try .map sidecar
    let map_path = js_path.with_extension("js.map");
    if map_path.exists() {
        match std::fs::read(&map_path) {
            Ok(bytes) => {
                return sourcemap::SourceMap::from_reader(&bytes[..]).ok();
            }
            Err(e) => {
                warn!(path = %map_path.display(), error = %e, "failed to read source map");
            }
        }
    }

    // Try inline source map in the JS file
    if let Ok(content) = std::fs::read_to_string(js_path) {
        if let Some(pos) = content.rfind("//# sourceMappingURL=data:") {
            let data_url = &content[pos + 26..];
            // Format: application/json;base64,<data>
            if let Some(comma_pos) = data_url.find(',') {
                let b64 = data_url[comma_pos + 1..].trim();
                if let Ok(decoded) = base64_decode(b64) {
                    return sourcemap::SourceMap::from_reader(&decoded[..]).ok();
                }
            }
        }
    }

    None
}

/// Simple base64 decode (avoid extra dependency).
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    // Use the base64 alphabet
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
        let val = TABLE.iter().position(|&c| c == byte).ok_or(())? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_no_source_maps_passes_through() {
        let mut file_paths = HashMap::new();
        file_paths.insert(42, PathBuf::from("src/app.js"));
        let branches = vec![BranchId::new(42, 10, 5, 0)];

        let (remapped, new_files) = remap_source_maps(branches, &file_paths, Path::new("/nonexistent"));
        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].file_id, 42);
        assert_eq!(remapped[0].line, 10);
        assert!(new_files.contains_key(&42));
    }

    #[test]
    fn base64_decode_basic() {
        let encoded = "SGVsbG8=";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn base64_decode_no_padding() {
        let encoded = "SGVsbG8";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn load_source_map_nonexistent_file() {
        assert!(load_source_map(Path::new("/no/such/file.js")).is_none());
    }
}
```

- [ ] **Step 3: Register module**

Add `pub mod source_map;` to `crates/apex-instrument/src/lib.rs`.

- [ ] **Step 4: Integrate into the pipeline**

In `crates/apex-instrument/src/javascript.rs`, replace the `// TODO: Implement source map remapping` comment with:

```rust
// Stage 5: Source map remapping
let (branch_ids, file_paths) = if env.is_typescript || env.source_maps {
    crate::source_map::remap_source_maps(branch_ids, &file_paths, &target.root)
} else {
    (branch_ids, file_paths)
};
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-instrument`
Expected: All tests pass (source map tests + existing Istanbul tests).

- [ ] **Step 6: Commit**

```bash
git add crates/apex-instrument/src/source_map.rs crates/apex-instrument/src/lib.rs crates/apex-instrument/src/javascript.rs crates/apex-instrument/Cargo.toml
git commit -m "feat: add source map remapping for TS/JS coverage"
```

---

## Chunk 4: Bun + Monorepo + Feature Matrix (Tasks 7-9)

### Task 7: Update Feature Matrix

**Files:**
- Modify: `crates/apex-core/src/types.rs`

- [ ] **Step 1: Write failing test**

Add to `crates/apex-core/src/types.rs` tests:

```rust
#[test]
fn javascript_concolic_full() {
    let features = Language::JavaScript.supported_features();
    let concolic = features.iter().find(|f| f.name == "concolic").unwrap();
    assert_eq!(concolic.status, FeatureStatus::Full);
}

#[test]
fn javascript_instrumentation_tools_updated() {
    let features = Language::JavaScript.supported_features();
    let instr = features.iter().find(|f| f.name == "instrumentation").unwrap();
    assert!(instr.tool.contains("v8"), "tool should mention v8: {}", instr.tool);
}
```

- [ ] **Step 2: Run to verify failing**

Run: `cargo test -p apex-core javascript_concolic_full`
Expected: FAIL — concolic is Missing.

- [ ] **Step 3: Update feature matrix**

In `crates/apex-core/src/types.rs`, in the `Language::JavaScript` match arm of `supported_features()` (around line 232):

Change:
```rust
feat("instrumentation", Full, "istanbul"),
```
to:
```rust
feat("instrumentation", Full, "istanbul+v8+c8"),
```

Change:
```rust
feat("concolic", Missing, ""),
```
to:
```rust
feat("concolic", Full, "ast+z3"),
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-core`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/types.rs
git commit -m "feat: update JS feature matrix — instrumentation v8/c8, concolic full"
```

---

### Task 8: Update apex-sandbox JS to use shared hash

**Files:**
- Modify: `crates/apex-sandbox/src/javascript.rs`

This was partially done in Task 1 (fnv1a_hash extraction). The sandbox also has its own Istanbul parser — for now, leave it as-is since it's a simpler, different parser (it only extracts hit/miss, not full BranchId mapping). Adding V8 format support to the sandbox is deferred until the sandbox is actually exercised with V8-format projects.

- [ ] **Step 1: Verify sandbox tests pass after Task 1 changes**

Run: `cargo test -p apex-sandbox`
Expected: All pass.

- [ ] **Step 2: No additional changes needed — commit note**

The sandbox's `parse_istanbul_branches` is a different, simpler parser (returns `BranchHit` tuples, not `BranchId`). It will be updated when V8 sandbox support is needed.

---

### Task 9: Bun Runtime + Monorepo (apex-lang)

Bun runtime support is already wired via `JsEnvironment` (Task 3). The coverage tool selection (Task 5) handles Bun. The `JavaScriptRunner` in apex-lang needs explicit Bun install support.

**Files:**
- Modify: `crates/apex-lang/src/javascript.rs`

- [ ] **Step 1: Write test for Bun install detection**

Add to `crates/apex-lang/src/javascript.rs` tests:

```rust
#[test]
fn detect_package_manager_bun() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bun.lockb"), "").unwrap();
    assert_eq!(
        JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
        "bun"
    );
}
```

- [ ] **Step 2: Run to verify failing**

Run: `cargo test -p apex-lang detect_package_manager_bun`
Expected: FAIL — returns "npm" because current code doesn't check bun.lockb.

- [ ] **Step 3: Update detect_package_manager**

The detect_package_manager now delegates to `JsEnvironment` (from Task 3), which already handles Bun. Verify the delegation is correct.

If the delegation from Task 3's Step 4 is implemented correctly, this test should pass. If not, add Bun detection directly:

```rust
fn detect_package_manager(target: &Path) -> &'static str {
    if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
        return "bun";
    }
    if target.join("yarn.lock").exists() {
        return "yarn";
    }
    if target.join("pnpm-lock.yaml").exists() {
        return "pnpm";
    }
    "npm"
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-lang`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add crates/apex-lang/src/javascript.rs
git commit -m "feat: add Bun runtime detection to JS runner"
```

---

## Chunk 5: Concolic Execution (Tasks 10-12)

### Task 10: Create `ConditionTree` IR

**Files:**
- Create: `crates/apex-concolic/src/condition_tree.rs`
- Modify: `crates/apex-concolic/src/lib.rs`

- [ ] **Step 1: Write the shared `ConditionTree` enum**

```rust
// crates/apex-concolic/src/condition_tree.rs

use serde::{Deserialize, Serialize};

/// Language-agnostic representation of a branch condition.
/// Extracted from source code AST, consumed by the Z3 constraint solver.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionTree {
    /// Comparison: x op y (where x, y are expressions)
    Compare {
        left: Box<Expr>,
        op: CompareOp,
        right: Box<Expr>,
    },
    /// Conjunction: a && b
    And(Box<ConditionTree>, Box<ConditionTree>),
    /// Disjunction: a || b
    Or(Box<ConditionTree>, Box<ConditionTree>),
    /// Negation: !a
    Not(Box<ConditionTree>),
    /// Type check: typeof x === "string" / isinstance(x, T) / x instanceof T
    TypeCheck {
        expr: Box<Expr>,
        type_name: String,
    },
    /// Membership: x in collection / "key" in obj
    Contains {
        needle: Box<Expr>,
        haystack: Box<Expr>,
    },
    /// Null/None check: x is None / x === null / x == null
    NullCheck {
        expr: Box<Expr>,
        is_null: bool, // true = checking for null, false = checking for not-null
    },
    /// Length check: len(x) op n / x.length op n
    LengthCheck {
        expr: Box<Expr>,
        op: CompareOp,
        value: Box<Expr>,
    },
    /// Opaque condition we couldn't parse — carries the raw source text.
    Unknown(String),
}

/// An expression within a condition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Expr {
    Variable(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BoolLiteral(bool),
    Null,
    /// Property access: x.y
    PropertyAccess { object: Box<Expr>, property: String },
    /// Function/method call (opaque — we store the text)
    Call(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompareOp {
    Eq,       // == / ===
    NotEq,    // != / !==
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl std::fmt::Display for CompareOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompareOp::Eq => write!(f, "=="),
            CompareOp::NotEq => write!(f, "!="),
            CompareOp::Lt => write!(f, "<"),
            CompareOp::LtEq => write!(f, "<="),
            CompareOp::Gt => write!(f, ">"),
            CompareOp::GtEq => write!(f, ">="),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_roundtrip() {
        let cond = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let json = serde_json::to_string(&cond).unwrap();
        let back: ConditionTree = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, back);
    }

    #[test]
    fn and_or_not() {
        let a = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let b = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Lt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        let combined = ConditionTree::And(Box::new(a), Box::new(b));
        let negated = ConditionTree::Not(Box::new(combined));
        // Just verify these construct without panic
        assert!(matches!(negated, ConditionTree::Not(_)));
    }

    #[test]
    fn null_check() {
        let cond = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("result".into())),
            is_null: true,
        };
        assert!(matches!(cond, ConditionTree::NullCheck { is_null: true, .. }));
    }

    #[test]
    fn type_check() {
        let cond = ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable("err".into())),
            type_name: "Error".into(),
        };
        assert!(matches!(cond, ConditionTree::TypeCheck { .. }));
    }

    #[test]
    fn compare_op_display() {
        assert_eq!(CompareOp::Eq.to_string(), "==");
        assert_eq!(CompareOp::NotEq.to_string(), "!=");
        assert_eq!(CompareOp::Lt.to_string(), "<");
        assert_eq!(CompareOp::GtEq.to_string(), ">=");
    }

    #[test]
    fn unknown_preserves_text() {
        let cond = ConditionTree::Unknown("some complex expr".into());
        if let ConditionTree::Unknown(text) = cond {
            assert_eq!(text, "some complex expr");
        } else {
            panic!("expected Unknown");
        }
    }
}
```

- [ ] **Step 2: Register module**

Add `pub mod condition_tree;` to `crates/apex-concolic/src/lib.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-concolic condition_tree`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-concolic/src/condition_tree.rs crates/apex-concolic/src/lib.rs
git commit -m "feat: add ConditionTree shared IR for concolic condition parsing"
```

---

### Task 11: JS/TS Condition Parser

**Files:**
- Create: `crates/apex-concolic/src/js_conditions.rs`
- Modify: `crates/apex-concolic/src/lib.rs`

- [ ] **Step 1: Write the JS condition parser**

```rust
// crates/apex-concolic/src/js_conditions.rs

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse a JavaScript/TypeScript condition string into a ConditionTree.
///
/// This is a best-effort text-based parser that handles common patterns.
/// Complex conditions fall back to `ConditionTree::Unknown`.
pub fn parse_js_condition(condition: &str) -> ConditionTree {
    let trimmed = condition.trim();

    // Try to parse logical operators (lowest precedence)
    if let Some(tree) = try_parse_logical(trimmed) {
        return tree;
    }

    // Try to parse comparisons
    if let Some(tree) = try_parse_comparison(trimmed) {
        return tree;
    }

    // Try typeof check: typeof x === "string"
    if let Some(tree) = try_parse_typeof(trimmed) {
        return tree;
    }

    // Try instanceof: x instanceof T
    if let Some(tree) = try_parse_instanceof(trimmed) {
        return tree;
    }

    // Try "in" operator: "key" in obj
    if let Some(tree) = try_parse_in(trimmed) {
        return tree;
    }

    // Try null check: x === null / x !== null / x == null / x != null
    if let Some(tree) = try_parse_null_check(trimmed) {
        return tree;
    }

    // Try optional chaining: x?.y (simplified: treat as null check on x)
    if let Some(tree) = try_parse_optional_chain(trimmed) {
        return tree;
    }

    // Try .length check: x.length > 0
    if let Some(tree) = try_parse_length_check(trimmed) {
        return tree;
    }

    // Fallback
    ConditionTree::Unknown(trimmed.to_string())
}

fn try_parse_logical(s: &str) -> Option<ConditionTree> {
    // Find && or || at the top level (not inside parens)
    let mut depth = 0i32;
    let bytes = s.as_bytes();

    // Check for || first (lower precedence)
    for i in 0..bytes.len().saturating_sub(1) {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'|' if depth == 0 && bytes.get(i + 1) == Some(&b'|') => {
                let left = &s[..i];
                let right = &s[i + 2..];
                return Some(ConditionTree::Or(
                    Box::new(parse_js_condition(left)),
                    Box::new(parse_js_condition(right)),
                ));
            }
            _ => {}
        }
    }

    depth = 0;
    for i in 0..bytes.len().saturating_sub(1) {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'&' if depth == 0 && bytes.get(i + 1) == Some(&b'&') => {
                let left = &s[..i];
                let right = &s[i + 2..];
                return Some(ConditionTree::And(
                    Box::new(parse_js_condition(left)),
                    Box::new(parse_js_condition(right)),
                ));
            }
            _ => {}
        }
    }

    // Check for leading !
    if s.starts_with('!') {
        let inner = s[1..].trim();
        let inner = if inner.starts_with('(') && inner.ends_with(')') {
            &inner[1..inner.len() - 1]
        } else {
            inner
        };
        return Some(ConditionTree::Not(Box::new(parse_js_condition(inner))));
    }

    None
}

fn try_parse_comparison(s: &str) -> Option<ConditionTree> {
    // Try strict operators first: ===, !==, then >=, <=, >, <, ==, !=
    let ops = [
        ("===", CompareOp::Eq),
        ("!==", CompareOp::NotEq),
        (">=", CompareOp::GtEq),
        ("<=", CompareOp::LtEq),
        (">", CompareOp::Gt),
        ("<", CompareOp::Lt),
        ("==", CompareOp::Eq),
        ("!=", CompareOp::NotEq),
    ];

    for (token, op) in &ops {
        if let Some(pos) = s.find(token) {
            let left = s[..pos].trim();
            let right = s[pos + token.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                return Some(ConditionTree::Compare {
                    left: Box::new(parse_expr(left)),
                    op: *op,
                    right: Box::new(parse_expr(right)),
                });
            }
        }
    }
    None
}

fn try_parse_typeof(s: &str) -> Option<ConditionTree> {
    // typeof x === "string"
    if let Some(rest) = s.strip_prefix("typeof ") {
        // Find the comparison operator
        for op_str in ["===", "==", "!==", "!="] {
            if let Some(pos) = rest.find(op_str) {
                let var = rest[..pos].trim();
                let type_name = rest[pos + op_str.len()..]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                return Some(ConditionTree::TypeCheck {
                    expr: Box::new(parse_expr(var)),
                    type_name: type_name.to_string(),
                });
            }
        }
    }
    None
}

fn try_parse_instanceof(s: &str) -> Option<ConditionTree> {
    if let Some(pos) = s.find(" instanceof ") {
        let expr = s[..pos].trim();
        let type_name = s[pos + 12..].trim();
        return Some(ConditionTree::TypeCheck {
            expr: Box::new(parse_expr(expr)),
            type_name: type_name.to_string(),
        });
    }
    None
}

fn try_parse_in(s: &str) -> Option<ConditionTree> {
    // "key" in obj
    if let Some(pos) = s.find(" in ") {
        let needle = s[..pos].trim();
        let haystack = s[pos + 4..].trim();
        return Some(ConditionTree::Contains {
            needle: Box::new(parse_expr(needle)),
            haystack: Box::new(parse_expr(haystack)),
        });
    }
    None
}

fn try_parse_null_check(s: &str) -> Option<ConditionTree> {
    for (op_str, is_null) in [("=== null", true), ("== null", true), ("!== null", false), ("!= null", false)] {
        if s.ends_with(op_str) {
            let expr = s[..s.len() - op_str.len()].trim();
            return Some(ConditionTree::NullCheck {
                expr: Box::new(parse_expr(expr)),
                is_null,
            });
        }
    }
    // null === x / null == x
    for (prefix, is_null) in [("null === ", true), ("null == ", true), ("null !== ", false), ("null != ", false)] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(parse_expr(rest.trim())),
                is_null,
            });
        }
    }
    None
}

fn try_parse_optional_chain(s: &str) -> Option<ConditionTree> {
    if s.contains("?.") {
        let parts: Vec<&str> = s.splitn(2, "?.").collect();
        if parts.len() == 2 {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(parse_expr(parts[0].trim())),
                is_null: false,
            });
        }
    }
    None
}

fn try_parse_length_check(s: &str) -> Option<ConditionTree> {
    // x.length > 0, arr.length >= 1, etc.
    if let Some(pos) = s.find(".length") {
        let expr = &s[..pos];
        let rest = s[pos + 7..].trim();
        for (token, op) in [(">=", CompareOp::GtEq), ("<=", CompareOp::LtEq), (">", CompareOp::Gt), ("<", CompareOp::Lt), ("===", CompareOp::Eq), ("==", CompareOp::Eq)] {
            if let Some(val) = rest.strip_prefix(token) {
                return Some(ConditionTree::LengthCheck {
                    expr: Box::new(parse_expr(expr.trim())),
                    op,
                    value: Box::new(parse_expr(val.trim())),
                });
            }
        }
    }
    None
}

fn parse_expr(s: &str) -> Expr {
    let trimmed = s.trim();

    // Null
    if trimmed == "null" || trimmed == "undefined" {
        return Expr::Null;
    }

    // Boolean
    if trimmed == "true" {
        return Expr::BoolLiteral(true);
    }
    if trimmed == "false" {
        return Expr::BoolLiteral(false);
    }

    // String literal
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('`') && trimmed.ends_with('`'))
    {
        return Expr::StringLiteral(trimmed[1..trimmed.len() - 1].to_string());
    }

    // Integer
    if let Ok(n) = trimmed.parse::<i64>() {
        return Expr::IntLiteral(n);
    }

    // Float
    if let Ok(f) = trimmed.parse::<f64>() {
        return Expr::FloatLiteral(f);
    }

    // Property access: x.y (but not method calls x.y())
    if let Some(dot_pos) = trimmed.rfind('.') {
        let prop = &trimmed[dot_pos + 1..];
        if !prop.contains('(') && !prop.is_empty() {
            return Expr::PropertyAccess {
                object: Box::new(parse_expr(&trimmed[..dot_pos])),
                property: prop.to_string(),
            };
        }
    }

    // Call: anything with parens
    if trimmed.contains('(') {
        return Expr::Call(trimmed.to_string());
    }

    // Variable
    Expr::Variable(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strict_equality() {
        let tree = parse_js_condition("x === 0");
        assert!(matches!(tree, ConditionTree::Compare { op: CompareOp::Eq, .. }));
    }

    #[test]
    fn parse_strict_inequality() {
        let tree = parse_js_condition("x !== null");
        assert!(matches!(tree, ConditionTree::NullCheck { is_null: false, .. }));
    }

    #[test]
    fn parse_typeof() {
        let tree = parse_js_condition(r#"typeof x === "string""#);
        assert!(matches!(tree, ConditionTree::TypeCheck { .. }));
        if let ConditionTree::TypeCheck { type_name, .. } = tree {
            assert_eq!(type_name, "string");
        }
    }

    #[test]
    fn parse_instanceof() {
        let tree = parse_js_condition("err instanceof Error");
        assert!(matches!(tree, ConditionTree::TypeCheck { .. }));
        if let ConditionTree::TypeCheck { type_name, .. } = tree {
            assert_eq!(type_name, "Error");
        }
    }

    #[test]
    fn parse_in_operator() {
        let tree = parse_js_condition(r#""key" in obj"#);
        assert!(matches!(tree, ConditionTree::Contains { .. }));
    }

    #[test]
    fn parse_and() {
        let tree = parse_js_condition("x > 5 && x < 10");
        assert!(matches!(tree, ConditionTree::And(_, _)));
    }

    #[test]
    fn parse_or() {
        let tree = parse_js_condition("a || b");
        assert!(matches!(tree, ConditionTree::Or(_, _)));
    }

    #[test]
    fn parse_not() {
        let tree = parse_js_condition("!valid");
        assert!(matches!(tree, ConditionTree::Not(_)));
    }

    #[test]
    fn parse_null_check_eq() {
        let tree = parse_js_condition("x === null");
        assert!(matches!(tree, ConditionTree::NullCheck { is_null: true, .. }));
    }

    #[test]
    fn parse_null_check_ne() {
        let tree = parse_js_condition("x != null");
        assert!(matches!(tree, ConditionTree::NullCheck { is_null: false, .. }));
    }

    #[test]
    fn parse_optional_chain() {
        let tree = parse_js_condition("user?.name");
        assert!(matches!(tree, ConditionTree::NullCheck { is_null: false, .. }));
    }

    #[test]
    fn parse_length_check() {
        let tree = parse_js_condition("arr.length > 0");
        assert!(matches!(tree, ConditionTree::LengthCheck { .. }));
    }

    #[test]
    fn parse_unknown_fallback() {
        let tree = parse_js_condition("some.complex(thing).here");
        assert!(matches!(tree, ConditionTree::Unknown(_)));
    }

    #[test]
    fn parse_expr_int() {
        assert!(matches!(parse_expr("42"), Expr::IntLiteral(42)));
    }

    #[test]
    fn parse_expr_string() {
        assert!(matches!(parse_expr(r#""hello""#), Expr::StringLiteral(_)));
    }

    #[test]
    fn parse_expr_null() {
        assert!(matches!(parse_expr("null"), Expr::Null));
    }

    #[test]
    fn parse_expr_undefined() {
        assert!(matches!(parse_expr("undefined"), Expr::Null));
    }

    #[test]
    fn parse_expr_property() {
        let e = parse_expr("obj.field");
        assert!(matches!(e, Expr::PropertyAccess { property, .. } if property == "field"));
    }

    #[test]
    fn parse_switch_case_comparison() {
        // switch cases come through as equality comparisons
        let tree = parse_js_condition("x === 1");
        assert!(matches!(tree, ConditionTree::Compare { op: CompareOp::Eq, .. }));
    }

    #[test]
    fn parse_comparison_gt() {
        let tree = parse_js_condition("count > 100");
        if let ConditionTree::Compare { left, op, right } = tree {
            assert_eq!(op, CompareOp::Gt);
            assert!(matches!(*left, Expr::Variable(ref v) if v == "count"));
            assert!(matches!(*right, Expr::IntLiteral(100)));
        } else {
            panic!("expected Compare");
        }
    }
}
```

- [ ] **Step 2: Register module**

Add `pub mod js_conditions;` to `crates/apex-concolic/src/lib.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-concolic js_conditions`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-concolic/src/js_conditions.rs crates/apex-concolic/src/lib.rs
git commit -m "feat: add JS/TS condition parser for concolic execution"
```

---

### Task 12: Final Integration Test

- [ ] **Step 1: Run full workspace test suite**

Run: `cargo test`
Expected: All tests across all crates pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace 2>&1 | head -50`
Expected: No new warnings.

- [ ] **Step 3: Commit any clippy fixes if needed**

```bash
git commit -m "style: fix clippy warnings from JS/TS support"
```
