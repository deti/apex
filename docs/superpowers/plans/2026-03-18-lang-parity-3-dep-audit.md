<!-- status: FUTURE -->

# Language Parity Plan 3: Dependency Audit Expansion

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add dependency audit support for all languages with package managers: C# (dotnet), Swift (SPM), Ruby (bundler-audit), C++ (Conan/vcpkg), Kotlin (already via Maven). Improve Go's existing govulncheck integration.

**Architecture:** All work is in `crates/apex-detect/src/detectors/dep_audit.rs`. Each language gets: (1) an `async fn audit_<lang>()` method on `DependencyAuditDetector`, (2) a parser function for the tool's output format, (3) a match arm in `analyze()`, (4) tests with realistic fixture data.

**Tech Stack:** `dotnet list package --vulnerable`, `bundler-audit`, `swift package audit` (or manual advisory lookup), govulncheck (existing)

**Depends on:** Nothing — independent of Plans 1-2.

---

## Chunk 1: .NET, Ruby, Swift

### Task 1: C# / .NET Dependency Audit

**Files:**
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parse_dotnet_vulnerable_packages() {
    let output = r#"Project 'MyApp' has the following vulnerable packages
   [net8.0]:
   Top-level Package                     Requested   Resolved   Severity   Advisory URL
   > System.Text.Json                    8.0.0       8.0.0      High       https://github.com/advisories/GHSA-1234
   > Microsoft.Data.SqlClient            5.1.0       5.1.0      Critical   https://github.com/advisories/GHSA-5678
"#;
    let findings = parse_dotnet_audit_output(output).unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].severity, Severity::High);
    assert!(findings[0].title.contains("System.Text.Json"));
    assert_eq!(findings[1].severity, Severity::Critical);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p apex-detect parse_dotnet`
Expected: FAIL

- [ ] **Step 3: Implement dotnet audit**

```rust
async fn audit_dotnet(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
    let spec = CommandSpec::new("dotnet", &ctx.target_root)
        .args(["list", "package", "--vulnerable", "--include-transitive"]);

    let output = match ctx.runner.run_command(&spec).await {
        Ok(o) => o,
        Err(e) if is_tool_not_found(&e) => {
            return Ok(vec![tool_not_installed_finding("dotnet", "*.csproj")]);
        }
        Err(e) => return Err(ApexError::Detect(format!("dotnet list: {e}"))),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_dotnet_audit_output(&stdout)
}
```

```rust
pub fn parse_dotnet_audit_output(raw: &str) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    // Parse tabular output: lines starting with "   > " are vulnerable packages
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('>') { continue; }
        let parts: Vec<&str> = trimmed[1..].split_whitespace().collect();
        if parts.len() < 5 { continue; }
        let pkg = parts[0];
        let version = parts[2]; // Resolved version
        let sev_str = parts[3].to_lowercase();
        let severity = match sev_str.as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "moderate" | "medium" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Medium,
        };
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "dependency-audit".into(),
            severity,
            category: FindingCategory::DependencyVuln,
            file: PathBuf::from("*.csproj"),
            line: None,
            title: format!("{pkg} {version}"),
            description: format!("Vulnerable package: {pkg} {version} ({sev_str})"),
            evidence: vec![],
            covered: true,
            suggestion: format!("Upgrade {pkg} to a patched version"),
            explanation: None,
            fix: None,
            cwe_ids: vec![1395],
        });
    }
    Ok(findings)
}
```

- [ ] **Step 4: Add match arm in `analyze()`**

```rust
Language::CSharp => self.audit_dotnet(ctx).await,
```

- [ ] **Step 5: Run tests**

Run: `cargo nextest run -p apex-detect dep_audit`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(detect): add .NET dependency audit via dotnet list"
```

---

### Task 2: Ruby Dependency Audit (bundler-audit)

**Files:**
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parse_bundler_audit_output() {
    let output = "Name: actionpack\nVersion: 7.0.4\nAdvisory: CVE-2023-22795\nCriticality: High\nURL: https://nvd.nist.gov/...\nTitle: ReDoS in Action Dispatch\nSolution: upgrade to ~> 7.0.4.1\n\n";
    let findings = parse_bundler_audit(output).unwrap();
    assert_eq!(findings.len(), 1);
    assert!(findings[0].title.contains("actionpack"));
    assert_eq!(findings[0].severity, Severity::High);
}
```

- [ ] **Step 2: Implement bundler-audit parser**

```rust
async fn audit_bundler(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
    let spec = CommandSpec::new("bundler-audit", &ctx.target_root).args(["check"]);
    // ... standard tool-not-found pattern ...
}

pub fn parse_bundler_audit(raw: &str) -> Result<Vec<Finding>> {
    // bundler-audit outputs blocks separated by blank lines
    // Each block: Name:, Version:, Advisory:, Criticality:, URL:, Title:, Solution:
    let mut findings = Vec::new();
    for block in raw.split("\n\n") {
        let mut name = "";
        let mut version = "";
        let mut advisory = "";
        let mut severity = Severity::Medium;
        let mut title = "";
        let mut solution = "";
        for line in block.lines() {
            if let Some(v) = line.strip_prefix("Name: ") { name = v; }
            if let Some(v) = line.strip_prefix("Version: ") { version = v; }
            if let Some(v) = line.strip_prefix("Advisory: ") { advisory = v; }
            if let Some(v) = line.strip_prefix("Criticality: ") {
                severity = match v.to_lowercase().as_str() {
                    "critical" => Severity::Critical,
                    "high" => Severity::High,
                    "medium" => Severity::Medium,
                    "low" => Severity::Low,
                    _ => Severity::Medium,
                };
            }
            if let Some(v) = line.strip_prefix("Title: ") { title = v; }
            if let Some(v) = line.strip_prefix("Solution: ") { solution = v; }
        }
        if !name.is_empty() {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "dependency-audit".into(),
                severity,
                category: FindingCategory::DependencyVuln,
                file: PathBuf::from("Gemfile.lock"),
                line: None,
                title: format!("{name} {version} ({advisory})"),
                description: title.to_string(),
                evidence: vec![],
                covered: true,
                suggestion: solution.to_string(),
                explanation: None,
                fix: None,
                cwe_ids: vec![1395],
            });
        }
    }
    Ok(findings)
}
```

- [ ] **Step 3: Add match arm**: `Language::Ruby => self.audit_bundler(ctx).await,`

- [ ] **Step 4: Run tests, commit**

```bash
git commit -m "feat(detect): add Ruby dependency audit via bundler-audit"
```

---

### Task 3: Swift Dependency Audit

**Files:**
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs`

- [ ] **Step 1: Implement swift audit**

Swift Package Manager doesn't have a built-in audit command. Use `swift package show-dependencies --format json` to list deps, then check against a known advisory database. For now, implement as a stub that reports Info when no audit tool is available, with support for `swift-audit` if it exists.

```rust
async fn audit_swift(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
    // Try swift-audit first (third-party tool)
    let spec = CommandSpec::new("swift-audit", &ctx.target_root).args(["check"]);
    match ctx.runner.run_command(&spec).await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_swift_audit_output(&stdout)
        }
        Err(e) if is_tool_not_found(&e) => {
            Ok(vec![tool_not_installed_finding("swift-audit", "Package.resolved")])
        }
        Err(e) => Err(ApexError::Detect(format!("swift-audit: {e}"))),
    }
}
```

- [ ] **Step 2: Add match arm**: `Language::Swift => self.audit_swift(ctx).await,`

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat(detect): add Swift dependency audit stub"
```

---

## Chunk 2: C++ and Wildcard

### Task 4: C++ Dependency Audit (Conan/vcpkg)

**Files:**
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs`

- [ ] **Step 1: Implement C++ audit**

C++ has no standard package audit tool. Check for `conanfile.txt`/`vcpkg.json` and report an Info finding noting that manual review is needed. If `osv-scanner` is available (Google's cross-language scanner), use it.

```rust
async fn audit_cpp(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
    // Try osv-scanner first (works for C++ via lockfiles)
    let spec = CommandSpec::new("osv-scanner", &ctx.target_root).args(["--lockfile", "."]);
    match ctx.runner.run_command(&spec).await {
        Ok(output) => parse_osv_scanner_output(&String::from_utf8_lossy(&output.stdout)),
        Err(e) if is_tool_not_found(&e) => {
            Ok(vec![tool_not_installed_finding("osv-scanner", "conanfile.txt")])
        }
        Err(e) => Err(ApexError::Detect(format!("osv-scanner: {e}"))),
    }
}
```

- [ ] **Step 2: Add match arms**:
```rust
Language::Cpp | Language::C => self.audit_cpp(ctx).await,
```

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat(detect): add C/C++ dependency audit via osv-scanner"
```

---

### Task 5: Update Wildcard Arm

**Files:**
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs`

- [ ] **Step 1: Replace `_ => Ok(vec![])` with explicit arms**

Every language should now have a handler. Replace the wildcard with explicit remaining languages (WASM → `Ok(vec![])`) so future languages get a compile error if unhandled.

- [ ] **Step 2: Run full test suite**

Run: `cargo nextest run -p apex-detect`
Expected: All pass

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(detect): exhaustive language matching in dep audit"
```
