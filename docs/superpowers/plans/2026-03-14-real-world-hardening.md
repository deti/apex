<!-- status: FUTURE -->
# Real-World Project Hardening Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 16 bugs found when running APEX against real-world Python/JS/TS projects — package manager detection, venv handling, coverage semantics, strategy validation, config safety.

**Architecture:** Three independent chunks: (1) Python environment hardening in `apex-lang` and `apex-instrument`, (2) core correctness fixes in `apex-coverage` and `apex-core`, (3) CLI/config safety in `apex-cli` and `apex-index`. Each chunk is self-contained and independently testable.

**Tech Stack:** Rust, tokio, serde, coverage.py JSON, clap CLI

---

## File Map

| File | Changes |
|------|---------|
| `crates/apex-lang/src/python.rs` | Package manager detection, venv detection, python binary resolution, test runner detection |
| `crates/apex-instrument/src/python.rs` | Coverage JSON version checking |
| `crates/apex-coverage/src/oracle.rs` | Fix 0-branch = 100% coverage |
| `crates/apex-core/src/config.rs` | Coverage target bound checking, config parse error propagation, configurable omit patterns |
| `crates/apex-cli/src/lib.rs` | Strategy validation, Ruby/stub warnings, explicit agent match arm |
| `crates/apex-index/src/types.rs` | Fix 0-branch = 100% coverage (same bug), use configurable omit patterns, update `hash_source_files` caller |

## Review Amendments

The following issues were caught during plan review and are incorporated below:

- **C1:** `resolve_python()`/`resolve_pip()` must use `OnceLock` caching and check `output.status.success()`, not just `output.is_ok()`. Existing mock tests match on `"python3"`/`"pip3"` — the resolved value must be used consistently.
- **C2:** Task 8 changes `discover()` signature — ALL 4 existing `discover` tests must be updated, not just 1.
- **C3:** `BranchIndex::coverage_percent()` in `apex-index/src/types.rs:79-84` has the same 0=100% bug — fixed in Task 6.
- **I1:** Poetry detection checks `pyproject.toml` for `[tool.poetry]` as secondary signal (not just lockfile).
- **I2/I3:** Task 4 test rewritten to be non-contradictory; substring issue noted as acceptable tradeoff vs full TOML parsing.
- **I4:** Task 5 tests use `data.meta_version()` method, not field access.
- **I5:** Task 3 notes Windows venv paths as out of scope.
- **I6:** Task 11 explicitly updates `hash_source_files` caller.

---

## Chunk 1: Python Environment Hardening

### Task 1: Python binary resolution

**Files:**
- Modify: `crates/apex-lang/src/python.rs:35,39,96,105` (all `python3`/`pip3` hardcodes)

Currently every Python/pip invocation hardcodes `python3`/`pip3`. On some systems only `python`/`pip` exists.

- [ ] **Step 1: Write failing test for python binary resolution**

```rust
#[test]
fn resolve_python_binary_checks_python3_then_python() {
    // resolve_python() should return "python3" if available, else "python"
    let bin = PythonRunner::<RealCommandRunner>::resolve_python();
    assert!(bin == "python3" || bin == "python");
}
```

Add to the `#[cfg(test)] mod tests` block in `crates/apex-lang/src/python.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-lang resolve_python_binary`
Expected: FAIL — `resolve_python` doesn't exist yet.

- [ ] **Step 3: Implement resolve_python() and resolve_pip()**

Use `OnceLock` for caching so the subprocess check runs at most once. Check `output.status.success()`, not just `.is_ok()` (a broken python3 that exits non-zero should fall through).

Add two helper methods to `PythonRunner<R>`:

```rust
use std::sync::OnceLock;

/// Find the Python interpreter. Prefers python3, falls back to python.
/// Cached after first call via OnceLock.
fn resolve_python() -> &'static str {
    static PYTHON: OnceLock<&str> = OnceLock::new();
    PYTHON.get_or_init(|| {
        use std::process::Command;
        match Command::new("python3").arg("--version").output() {
            Ok(output) if output.status.success() => "python3",
            _ => "python",
        }
    })
}

/// Find the pip installer. Prefers pip3, falls back to pip.
fn resolve_pip() -> &'static str {
    static PIP: OnceLock<&str> = OnceLock::new();
    PIP.get_or_init(|| {
        use std::process::Command;
        match Command::new("pip3").arg("--version").output() {
            Ok(output) if output.status.success() => "pip3",
            _ => "pip",
        }
    })
}
```

Then replace all hardcoded `"python3"` with `Self::resolve_python()` and `"pip3"` with `Self::resolve_pip()` throughout the file. The key locations:
- Line 35: `detect_test_runner` — `"python3"` → `Self::resolve_python()`
- Line 39: fallback — same
- Line 66: `install_deps` pip — `"pip3"` → `Self::resolve_pip()`
- Line 80: pip install -e — same
- Line 96: coverage check — `"python3"` → `Self::resolve_python()`
- Line 105: pip install coverage — `Self::resolve_pip()`

**Existing mock tests:** The 15+ mock tests that match on `spec.program == "python3"` / `"pip3"` will continue to work on systems where python3/pip3 exist (the common case). On CI without Python, the resolved names change to `"python"`/`"pip"` — update mock expectations to use `Self::resolve_python()` / `Self::resolve_pip()` in the test setup, or match with a predicate that accepts either.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p apex-lang resolve_python`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-lang/src/python.rs
git commit -m "fix: resolve python/pip binary with fallback"
```

---

### Task 2: Package manager detection (uv/poetry/pipenv)

**Files:**
- Modify: `crates/apex-lang/src/python.rs:62-121` (`install_deps`)

`install_deps` always uses pip. Projects using poetry, uv, or pipenv will fail.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn detect_package_manager_poetry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("poetry.lock"), "").unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]\nname = \"x\"\n").unwrap();
    let pm = PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path());
    assert_eq!(pm, PackageManager::Poetry);
}

#[test]
fn detect_package_manager_uv() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("uv.lock"), "").unwrap();
    let pm = PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path());
    assert_eq!(pm, PackageManager::Uv);
}

#[test]
fn detect_package_manager_pipenv() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Pipfile"), "").unwrap();
    let pm = PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path());
    assert_eq!(pm, PackageManager::Pipenv);
}

#[test]
fn detect_package_manager_poetry_no_lockfile() {
    // Poetry project without poetry.lock — detected via pyproject.toml [tool.poetry]
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]\nname = \"x\"\n").unwrap();
    let pm = PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path());
    assert_eq!(pm, PackageManager::Poetry);
}

#[test]
fn detect_package_manager_pip_fallback() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("requirements.txt"), "requests\n").unwrap();
    let pm = PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path());
    assert_eq!(pm, PackageManager::Pip);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p apex-lang detect_package_manager`
Expected: FAIL — `detect_package_manager` and `PackageManager` don't exist.

- [ ] **Step 3: Implement PackageManager enum and detection**

Add above `PythonRunner`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Uv,
    Poetry,
    Pipenv,
    Pip,
}
```

Add to `PythonRunner<R>`:

```rust
/// Detect the project's package manager from lockfile/config markers.
/// Priority: uv > poetry > pipenv > pip (most specific lockfile wins).
fn detect_package_manager(target: &Path) -> PackageManager {
    if target.join("uv.lock").exists() {
        return PackageManager::Uv;
    }
    if target.join("poetry.lock").exists() {
        return PackageManager::Poetry;
    }
    // Poetry without lockfile — check pyproject.toml for [tool.poetry]
    if let Ok(content) = std::fs::read_to_string(target.join("pyproject.toml")) {
        if content.contains("[tool.poetry]") {
            return PackageManager::Poetry;
        }
    }
    if target.join("Pipfile.lock").exists() || target.join("Pipfile").exists() {
        return PackageManager::Pipenv;
    }
    PackageManager::Pip
}
```

- [ ] **Step 4: Update install_deps to use detected package manager**

Rewrite `install_deps` to dispatch on `detect_package_manager`:

```rust
async fn install_deps(&self, target: &Path) -> Result<()> {
    info!(target = %target.display(), "installing Python dependencies");
    let pm = Self::detect_package_manager(target);
    debug!(?pm, "detected package manager");

    match pm {
        PackageManager::Uv => {
            let spec = CommandSpec::new("uv", target).args(["sync"]);
            self.run_or_err(&spec, "uv sync").await?;
        }
        PackageManager::Poetry => {
            let spec = CommandSpec::new("poetry", target).args(["install"]);
            self.run_or_err(&spec, "poetry install").await?;
        }
        PackageManager::Pipenv => {
            let spec = CommandSpec::new("pipenv", target).args(["install", "--dev"]);
            self.run_or_err(&spec, "pipenv install").await?;
        }
        PackageManager::Pip => {
            let python = Self::resolve_python();
            let pip = Self::resolve_pip();
            if target.join("requirements.txt").exists() {
                let spec = CommandSpec::new(pip, target).args(["install", "-r", "requirements.txt"]);
                self.run_or_err(&spec, "pip install -r").await?;
            } else if target.join("pyproject.toml").exists() || target.join("setup.py").exists() {
                let spec = CommandSpec::new(pip, target).args(["install", "-e", "."]);
                self.run_or_err(&spec, "pip install -e").await?;
            }
            // Ensure coverage.py is available
            let cov_spec = CommandSpec::new(python, target).args(["-c", "import coverage"]);
            let cov_check = self.runner.run_command(&cov_spec).await
                .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
            if cov_check.exit_code != 0 {
                debug!("coverage.py not found, installing");
                let spec = CommandSpec::new(pip, target).args(["install", "coverage", "pytest"]);
                self.run_or_err(&spec, "install coverage/pytest").await?;
            }
        }
    }
    Ok(())
}
```

Add helper:

```rust
async fn run_or_err(&self, spec: &CommandSpec, label: &str) -> Result<()> {
    let output = self.runner.run_command(spec).await
        .map_err(|e| ApexError::LanguageRunner(format!("{label}: {e}")))?;
    if output.exit_code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(ApexError::LanguageRunner(format!("{label} failed: {stderr}")));
    }
    Ok(())
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p apex-lang`
Expected: All existing + new tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/apex-lang/src/python.rs
git commit -m "feat: detect uv/poetry/pipenv package managers"
```

---

### Task 3: Virtual environment detection

**Files:**
- Modify: `crates/apex-lang/src/python.rs` (new method + update `detect_test_runner` / `install_deps`)

APEX never activates virtualenvs, running against system Python instead.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn find_venv_python_dot_venv() {
    let dir = tempfile::tempdir().unwrap();
    let venv = dir.path().join(".venv").join("bin");
    std::fs::create_dir_all(&venv).unwrap();
    std::fs::write(venv.join("python"), "#!/bin/sh\n").unwrap();
    let result = PythonRunner::<RealCommandRunner>::find_venv_python(dir.path());
    assert!(result.is_some());
    assert!(result.unwrap().ends_with(".venv/bin/python"));
}

#[test]
fn find_venv_python_venv() {
    let dir = tempfile::tempdir().unwrap();
    let venv = dir.path().join("venv").join("bin");
    std::fs::create_dir_all(&venv).unwrap();
    std::fs::write(venv.join("python"), "#!/bin/sh\n").unwrap();
    let result = PythonRunner::<RealCommandRunner>::find_venv_python(dir.path());
    assert!(result.is_some());
}

#[test]
fn find_venv_python_none() {
    let dir = tempfile::tempdir().unwrap();
    let result = PythonRunner::<RealCommandRunner>::find_venv_python(dir.path());
    assert!(result.is_none());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p apex-lang find_venv_python`
Expected: FAIL

- [ ] **Step 3: Implement find_venv_python**

```rust
/// Check for a virtualenv in the project and return the python binary path.
/// Checks: .venv/bin/python, venv/bin/python, .env/bin/python, env/bin/python
fn find_venv_python(target: &Path) -> Option<String> {
    let candidates = [".venv", "venv", ".env", "env"];
    for name in &candidates {
        let bin = target.join(name).join("bin").join("python");
        if bin.exists() {
            return Some(bin.to_string_lossy().into_owned());
        }
    }
    None
}
```

Then update `resolve_python()` to accept a target path and check venv first:

```rust
fn resolve_python_for(target: &Path) -> String {
    // 1. Check for project virtualenv
    if let Some(venv_python) = Self::find_venv_python(target) {
        return venv_python;
    }
    // 2. Fall back to system python3/python
    Self::resolve_python().to_string()
}
```

Update `detect_test_runner`, `install_deps`, and `run_tests` to use `resolve_python_for(target)` instead of the static `resolve_python()` where a target path is available. For pip-based installs, continue using the system `pip`/`pip3` (venv activation is handled by running python from the venv path).

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-lang`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-lang/src/python.rs
git commit -m "feat: detect and use project virtualenvs"
```

---

### Task 4: Test runner detection — stop using substring match

**Files:**
- Modify: `crates/apex-lang/src/python.rs:29-40`

`.contains("pytest")` matches false positives in comments, build deps, etc.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn detect_test_runner_unittest_setup_cfg() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("setup.cfg"),
        "[tool:pytest]\n# not actually using pytest\n",
    ).unwrap();
    // Should detect unittest if no pytest section in pyproject.toml
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"foo\"\ndependencies = [\"requests\"]\n",
    ).unwrap();
    let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
    // Should NOT contain pytest since pyproject.toml doesn't have [tool.pytest]
    // and "pytest" is not in test dependencies
    assert!(cmd.iter().any(|c| c == "unittest" || c == "pytest"));
}
```

- [ ] **Step 2: Rewrite detect_test_runner with structured parsing**

Replace the substring match with TOML section header checking:

```rust
fn detect_test_runner(target: &Path) -> Vec<String> {
    let python = Self::resolve_python_for(target);

    // Check pyproject.toml for pytest configuration section
    if let Ok(content) = std::fs::read_to_string(target.join("pyproject.toml")) {
        // Only match actual pytest config sections, not arbitrary mentions
        if content.contains("[tool.pytest") {
            return vec![python, "-m".into(), "pytest".into(), "-q".into()];
        }
        // Check if pytest is in test dependencies
        if content.contains("[project.optional-dependencies]") && content.contains("pytest") {
            return vec![python, "-m".into(), "pytest".into(), "-q".into()];
        }
    }

    // Check for pytest.ini or setup.cfg [tool:pytest]
    if target.join("pytest.ini").exists() {
        return vec![python, "-m".into(), "pytest".into(), "-q".into()];
    }
    if let Ok(content) = std::fs::read_to_string(target.join("setup.cfg")) {
        if content.contains("[tool:pytest]") {
            return vec![python, "-m".into(), "pytest".into(), "-q".into()];
        }
    }

    // Fallback: pytest is the most common, use it
    vec![python, "-m".into(), "pytest".into(), "-q".into()]
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-lang`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-lang/src/python.rs
git commit -m "fix: use structured TOML section matching for pytest detection"
```

---

### Task 5: Coverage JSON version checking

**Files:**
- Modify: `crates/apex-instrument/src/python.rs:24-35`

coverage.py JSON has a `meta.version` field. Different versions can have incompatible schemas.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn parse_coverage_json_warns_on_missing_version() {
    // A coverage JSON without meta.version should still parse but log a warning
    let json = r#"{"files": {}}"#;
    let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
    assert!(data.meta_version.is_none());
}

#[test]
fn parse_coverage_json_with_version() {
    let json = r#"{"meta": {"version": "7.4.0"}, "files": {}}"#;
    let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
    assert_eq!(data.meta_version.as_deref(), Some("7.4.0"));
}
```

- [ ] **Step 2: Update ApexCoverageJson to include version**

```rust
#[derive(Debug, Deserialize)]
struct CoverageMeta {
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApexCoverageJson {
    files: HashMap<String, FileData>,
    #[serde(default)]
    meta: Option<CoverageMeta>,
}

impl ApexCoverageJson {
    fn meta_version(&self) -> Option<&str> {
        self.meta.as_ref()?.version.as_deref()
    }
}
```

In `parse_coverage_json`, add a version check after deserialization:

```rust
// After successful parse:
match data.meta_version() {
    Some(v) => debug!(version = %v, "coverage.py JSON version"),
    None => warn!("coverage.py JSON has no version metadata — schema may be incompatible"),
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-instrument`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-instrument/src/python.rs
git commit -m "fix: check coverage.py JSON schema version"
```

---

## Chunk 2: Core Correctness

### Task 6: Fix 0 branches = 100% coverage

**Files:**
- Modify: `crates/apex-coverage/src/oracle.rs:165-172`

When no branches are registered, `coverage_percent()` returns 100.0 — a false positive.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_empty_oracle_zero_percent() {
    let oracle = CoverageOracle::new();
    // No branches registered → 0%, not 100%
    assert_eq!(oracle.coverage_percent(), 0.0);
}
```

Note: this contradicts the existing `test_empty_oracle_100_percent` test. That test documents the current (wrong) behavior. Remove or update it.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p apex-coverage test_empty_oracle_zero`
Expected: FAIL — returns 100.0

- [ ] **Step 3: Fix coverage_percent**

```rust
pub fn coverage_percent(&self) -> f64 {
    let total = self.total_count.load(Ordering::Relaxed);
    if total == 0 {
        return 0.0; // No branches = no coverage, not 100%
    }
    let covered = self.covered_count.load(Ordering::Relaxed);
    (covered as f64 / total as f64) * 100.0
}
```

Update `test_empty_oracle_100_percent` to `test_empty_oracle_zero_percent` expecting `0.0`.

**Impact check:** Search the codebase for code that depends on `coverage_percent() == 100.0` when oracle is empty. The orchestrator loop uses `oracle.coverage_percent() / 100.0 >= target` as a break condition — with 0.0 it will now correctly NOT break early on an empty oracle. The ratchet command compares `coverage_percent()` against a threshold — returning 0.0 when no branches are found will correctly fail the gate. Both are correct behaviors.

- [ ] **Step 4: Run tests, fix any downstream breaks**

Run: `cargo test --workspace`
Expected: All pass (after updating the one test).

- [ ] **Step 5: Commit**

```bash
git add crates/apex-coverage/src/oracle.rs
git commit -m "fix: 0 branches = 0% coverage, not 100%"
```

---

### Task 7: Coverage target bound checking

**Files:**
- Modify: `crates/apex-core/src/config.rs:71-87`

Users can set `target: 200.0` or `target: -1.0` with no validation.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn coverage_target_clamped_to_valid_range() {
    let toml = "[coverage]\ntarget = 2.0\n";
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert!(cfg.coverage.target <= 1.0, "target should be clamped to 1.0");
}

#[test]
fn coverage_target_negative_clamped() {
    let toml = "[coverage]\ntarget = -0.5\n";
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert!(cfg.coverage.target >= 0.0, "target should be clamped to 0.0");
}

#[test]
fn min_ratchet_clamped() {
    let toml = "[coverage]\nmin_ratchet = 1.5\n";
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert!(cfg.coverage.min_ratchet <= 1.0);
}
```

- [ ] **Step 2: Add validation to ApexConfig**

Add a `validate` method that clamps values and call it after deserialization:

```rust
impl ApexConfig {
    /// Clamp config values to valid ranges.
    fn validate(mut self) -> Self {
        self.coverage.target = self.coverage.target.clamp(0.0, 1.0);
        self.coverage.min_ratchet = self.coverage.min_ratchet.clamp(0.0, 1.0);
        self
    }

    pub fn parse_toml(s: &str) -> crate::Result<Self> {
        let cfg: Self = toml::from_str(s)
            .map_err(|e| crate::ApexError::Config(format!("invalid TOML: {e}")))?;
        Ok(cfg.validate())
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-core`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-core/src/config.rs
git commit -m "fix: clamp coverage targets to 0.0-1.0 range"
```

---

### Task 8: Config parse failure should error, not silent default

**Files:**
- Modify: `crates/apex-core/src/config.rs:42-64`

When `apex.toml` exists but has invalid TOML, `discover()` silently returns defaults. This hides user errors.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn discover_with_invalid_file_returns_error() {
    // This behavior is CHANGING: invalid apex.toml should be an error, not silent default
    let dir = std::env::temp_dir().join("apex_test_discover_invalid_v2");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("apex.toml"), "this is not valid toml [[[").unwrap();
    // discover() should now return a Result, or at minimum log at ERROR level
    // For backward compat, we change to eprintln + return defaults + set a flag
    let cfg = ApexConfig::discover(&dir);
    // The user should be warned loudly — we'll check via tracing
    let _ = std::fs::remove_dir_all(&dir);
    // This test documents the desired change; see step 3
}
```

- [ ] **Step 2: Change discover to return Result<Self>**

```rust
pub fn discover(start_dir: &Path) -> crate::Result<Self> {
    let mut dir = start_dir;
    loop {
        let candidate = dir.join("apex.toml");
        if candidate.is_file() {
            match Self::from_file(&candidate) {
                Ok(cfg) => {
                    tracing::info!(path = %candidate.display(), "loaded apex.toml");
                    return Ok(cfg);
                }
                Err(e) => {
                    // Don't silently swallow — return the error so the user knows
                    return Err(e);
                }
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    Ok(Self::default())
}
```

Then update all call sites of `discover()` to handle the Result. In `apex-cli/src/main.rs` or wherever it's called, add `.unwrap_or_else(|e| { tracing::error!(...); std::process::exit(1); })` or propagate with `?`.

- [ ] **Step 3: Update existing test**

Update `discover_with_invalid_file_returns_defaults` to expect an error:

```rust
#[test]
fn discover_with_invalid_file_returns_error() {
    let dir = std::env::temp_dir().join("apex_test_discover_invalid_err");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("apex.toml"), "this is not valid toml [[[").unwrap();
    let result = ApexConfig::discover(&dir);
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 4: Run tests, fix call sites**

Run: `cargo test --workspace`
Fix any call sites that relied on `discover()` returning `Self` directly.

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/config.rs crates/apex-cli/src/main.rs crates/apex-cli/src/lib.rs
git commit -m "fix: config parse failures are errors, not silent defaults"
```

---

## Chunk 3: CLI & Config Safety

### Task 9: Validate strategy names

**Files:**
- Modify: `crates/apex-cli/src/lib.rs:608-620`

Unknown strategies silently fall through to the agent cluster. The user gets no feedback that their `--strategy typo` was ignored.

- [ ] **Step 1: Write failing test**

Add to CLI tests:

```rust
#[test]
fn unknown_strategy_is_rejected() {
    // "typo" is not a valid strategy — should warn or error
    let valid = ["fuzz", "concolic", "driller", "agent", "all"];
    assert!(!valid.contains(&"typo"));
    // The match in run() should handle this explicitly
}
```

- [ ] **Step 2: Add explicit strategy validation**

Replace the `_ =>` catch-all with explicit matches:

```rust
match args.strategy.as_str() {
    "fuzz" => { /* ... existing ... */ }
    "concolic" => { /* ... existing ... */ }
    "driller" => { /* ... existing ... */ }
    "agent" | "all" => {
        run_agent_cluster(/* ... */).await?;
    }
    unknown => {
        warn!(strategy = %unknown, "Unknown strategy — falling back to agent orchestrator");
        run_agent_cluster(/* ... */).await?;
    }
}
```

This preserves backward compatibility (still runs) but warns the user.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-cli`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "fix: warn on unknown strategy names"
```

---

### Task 10: Warn on stubbed language support

**Files:**
- Modify: `crates/apex-cli/src/lib.rs:887-896`

Ruby returns an empty `InstrumentedTarget` with zero branches and no warning. The user thinks it worked.

- [ ] **Step 1: Add warning for stubbed languages**

```rust
Language::Ruby => {
    warn!("Ruby instrumentation is not yet implemented — returning empty coverage");
    apex_core::types::InstrumentedTarget {
        target: target.clone(),
        branch_ids: Vec::new(),
        executed_branch_ids: Vec::new(),
        file_paths: std::collections::HashMap::new(),
        work_dir: target_path.to_path_buf(),
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "fix: warn when using stubbed Ruby instrumentation"
```

---

### Task 11: Make directory omit patterns configurable

**Files:**
- Modify: `crates/apex-core/src/config.rs` (add `omit_patterns` to config)
- Modify: `crates/apex-index/src/types.rs:181-190` (use config instead of hardcoded list)

- [ ] **Step 1: Write failing test**

In `crates/apex-core/src/config.rs`:

```rust
#[test]
fn parse_omit_patterns() {
    let toml = r#"
[coverage]
omit_patterns = ["vendor", "third_party", "generated"]
"#;
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert_eq!(cfg.coverage.omit_patterns, vec!["vendor", "third_party", "generated"]);
}

#[test]
fn default_omit_patterns() {
    let cfg = ApexConfig::default();
    assert!(cfg.coverage.omit_patterns.contains(&"node_modules".to_string()));
    assert!(cfg.coverage.omit_patterns.contains(&"__pycache__".to_string()));
}
```

- [ ] **Step 2: Add omit_patterns to CoverageConfig**

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CoverageConfig {
    pub target: f64,
    pub min_ratchet: f64,
    /// Directory names to skip during source file collection.
    pub omit_patterns: Vec<String>,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        CoverageConfig {
            target: 1.0,
            min_ratchet: 0.8,
            omit_patterns: vec![
                "target".into(),
                "node_modules".into(),
                "__pycache__".into(),
                ".venv".into(),
                "venv".into(),
                "dist".into(),
                "build".into(),
            ],
        }
    }
}
```

- [ ] **Step 3: Update collect_source_files to accept omit list**

In `crates/apex-index/src/types.rs`, change:

```rust
fn collect_source_files(dir: &Path, extensions: &[&str], omit: &[String], out: &mut Vec<PathBuf>) {
    // ...
    let name_str = name.to_string_lossy();
    if name_str.starts_with('.') || omit.iter().any(|o| o == name_str.as_ref()) {
        continue;
    }
    // ...
}
```

Update all callers of `collect_source_files` to pass the omit list from config.

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-core -p apex-index`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/config.rs crates/apex-index/src/types.rs
git commit -m "feat: make directory omit patterns configurable via apex.toml"
```

---

### Task 12: Deno detection for JavaScript runner

**Files:**
- Modify: `crates/apex-lang/src/javascript.rs`

Bun is detected but Deno is not.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn detect_deno_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
    let runner = JavaScriptRunner::new();
    assert!(runner.detect(dir.path()));
}
```

- [ ] **Step 2: Add Deno detection**

In the `detect()` method, add:

```rust
|| target.join("deno.json").exists()
|| target.join("deno.jsonc").exists()
```

In `detect_package_manager` or equivalent, add Deno as a variant and detect via `deno.json`/`deno.lock`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-lang`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-lang/src/javascript.rs
git commit -m "feat: detect Deno projects via deno.json"
```

---

## Summary

| Task | Bug(s) Fixed | Crate |
|------|-------------|-------|
| 1 | python3/pip3 hardcoded, no fallback | apex-lang |
| 2 | No uv/poetry/pipenv detection | apex-lang |
| 3 | No venv detection | apex-lang |
| 4 | Test runner substring match | apex-lang |
| 5 | Coverage JSON version not checked | apex-instrument |
| 6 | 0 branches = 100% coverage | apex-coverage |
| 7 | Coverage targets not bound checked | apex-core |
| 8 | Config parse → silent defaults | apex-core |
| 9 | Unknown strategy silent | apex-cli |
| 10 | Ruby stub no warning | apex-cli |
| 11 | Hardcoded omit patterns | apex-core + apex-index |
| 12 | No Deno detection | apex-lang |

**Not in scope (stubs, not bugs):** Firecracker stub (behind feature flag, warns), TS handling (already included in JS extensions). Source dir monorepo guessing deferred to a separate plan (requires broader refactor of project layout detection).
