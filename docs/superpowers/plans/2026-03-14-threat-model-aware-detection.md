# Threat-Model-Aware Detection Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce APEX audit false positives by letting each repo declare its threat model (`cli-tool`, `web-service`, `library`, `ci-pipeline`) in `apex.toml`, then suppressing findings where the detected sink only receives trusted input for that context.

**Architecture:** Add `[threat_model]` config section to `apex-core`. In `apex-detect`, build per-language trust tables keyed by threat model type. `SecurityPatternDetector` checks threat model before emitting findings — trusted source→sink flows are suppressed. For Python, build CPG and use taint analysis for precise flow verification. A `/apex-threat-model` wizard command guides users through config creation.

**Tech Stack:** Rust, serde/toml, tree-sitter (existing CPG builder), clap (existing CLI)

---

## File Structure

| File | Role | Action |
|------|------|--------|
| `crates/apex-core/src/config.rs` | Threat model config types | Modify |
| `crates/apex-detect/src/threat_model.rs` | Trust classification tables | Create |
| `crates/apex-detect/src/lib.rs` | Export new module | Modify |
| `crates/apex-detect/src/context.rs` | Add threat model to context | Modify |
| `crates/apex-detect/src/detectors/security_pattern.rs` | Use threat model for suppression | Modify |
| `crates/apex-cli/src/lib.rs` | Build CPG + pass threat model in `run_audit` | Modify |
| `.claude/commands/apex-threat-model.md` | Wizard command | Create |
| `TODO.md` | Add CPG-for-other-languages items | Modify |

---

## Task 1: Add `ThreatModelConfig` to `apex-core`

**Files:**
- Modify: `crates/apex-core/src/config.rs`

- [ ] **Step 1: Add `ThreatModelType` enum and `ThreatModelConfig` struct**

Add after the `DetectConfig` section (after line 298):

```rust
// ---------------------------------------------------------------------------
// Threat Model
// ---------------------------------------------------------------------------

/// What kind of software is being analyzed.
/// Determines which input sources are considered trusted vs untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreatModelType {
    /// CLI tool — argv, env vars, config files are trusted.
    CliTool,
    /// Web service — request data is untrusted, env/config are trusted.
    WebService,
    /// Library — all external input is untrusted.
    Library,
    /// CI pipeline — env vars and argv are trusted, network input is not.
    CiPipeline,
}

/// Threat model configuration from `[threat_model]` in apex.toml.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThreatModelConfig {
    /// The type of software being analyzed.
    #[serde(rename = "type")]
    pub model_type: Option<ThreatModelType>,
    /// Additional sources the user considers trusted (beyond the defaults for this type).
    pub trusted_sources: Vec<String>,
    /// Additional sources the user considers untrusted (overrides defaults).
    pub untrusted_sources: Vec<String>,
}
```

- [ ] **Step 2: Add `threat_model` field to `ApexConfig`**

Add to the `ApexConfig` struct (line ~22):

```rust
pub threat_model: ThreatModelConfig,
```

- [ ] **Step 3: Write tests for config parsing**

```rust
#[test]
fn parse_threat_model_cli_tool() {
    let toml = r#"
[threat_model]
type = "cli-tool"
trusted_sources = ["config_file"]
"#;
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert_eq!(cfg.threat_model.model_type, Some(ThreatModelType::CliTool));
    assert_eq!(cfg.threat_model.trusted_sources, vec!["config_file"]);
}

#[test]
fn parse_threat_model_web_service() {
    let toml = r#"
[threat_model]
type = "web-service"
"#;
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert_eq!(cfg.threat_model.model_type, Some(ThreatModelType::WebService));
}

#[test]
fn parse_threat_model_library() {
    let toml = r#"
[threat_model]
type = "library"
untrusted_sources = ["user_callback"]
"#;
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert_eq!(cfg.threat_model.model_type, Some(ThreatModelType::Library));
    assert_eq!(cfg.threat_model.untrusted_sources, vec!["user_callback"]);
}

#[test]
fn parse_threat_model_ci_pipeline() {
    let toml = r#"
[threat_model]
type = "ci-pipeline"
"#;
    let cfg = ApexConfig::parse_toml(toml).unwrap();
    assert_eq!(cfg.threat_model.model_type, Some(ThreatModelType::CiPipeline));
}

#[test]
fn missing_threat_model_is_none() {
    let cfg = ApexConfig::parse_toml("").unwrap();
    assert!(cfg.threat_model.model_type.is_none());
    assert!(cfg.threat_model.trusted_sources.is_empty());
}
```

- [ ] **Step 4: Run tests, verify pass**

```bash
cargo test -p apex-core -- threat_model
```

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/config.rs
git commit -m "feat: add ThreatModelConfig to apex-core config"
```

---

## Task 2: Create trust classification module in `apex-detect`

**Files:**
- Create: `crates/apex-detect/src/threat_model.rs`
- Modify: `crates/apex-detect/src/lib.rs`

- [ ] **Step 1: Create `threat_model.rs` with trust tables**

```rust
//! Trust classification of input sources per threat model type.
//!
//! Each threat model type defines which input sources are trusted (safe to flow
//! into sinks without flagging) vs untrusted (flows should be reported).

use apex_core::config::{ThreatModelConfig, ThreatModelType};

/// Whether a source is trusted in the given threat model context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Safe — flows from this source to sinks are not flagged.
    Trusted,
    /// Dangerous — flows from this source to sinks ARE flagged.
    Untrusted,
    /// Not applicable to this threat model (source doesn't exist in this context).
    NotApplicable,
}

/// Built-in source patterns and their trust levels per threat model type.
struct SourceTrust {
    /// Pattern to match (substring of the indicator found in code context).
    pattern: &'static str,
    cli_tool: TrustLevel,
    web_service: TrustLevel,
    library: TrustLevel,
    ci_pipeline: TrustLevel,
}

use TrustLevel::*;

const SOURCE_TRUST_TABLE: &[SourceTrust] = &[
    SourceTrust {
        pattern: "argv",
        cli_tool: Trusted, web_service: NotApplicable, library: Untrusted, ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "args",
        cli_tool: Trusted, web_service: Untrusted, library: Untrusted, ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "request",
        cli_tool: NotApplicable, web_service: Untrusted, library: NotApplicable, ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "query",
        cli_tool: NotApplicable, web_service: Untrusted, library: Untrusted, ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "form",
        cli_tool: NotApplicable, web_service: Untrusted, library: NotApplicable, ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "param",
        cli_tool: NotApplicable, web_service: Untrusted, library: Untrusted, ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "input",
        cli_tool: Trusted, web_service: Untrusted, library: Untrusted, ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "stdin",
        cli_tool: Trusted, web_service: Untrusted, library: Untrusted, ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "environ",
        cli_tool: Trusted, web_service: Trusted, library: Untrusted, ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "getenv",
        cli_tool: Trusted, web_service: Trusted, library: Untrusted, ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "recv",
        cli_tool: Untrusted, web_service: Untrusted, library: Untrusted, ci_pipeline: Untrusted,
    },
    SourceTrust {
        pattern: "socket",
        cli_tool: Untrusted, web_service: Untrusted, library: Untrusted, ci_pipeline: Untrusted,
    },
    SourceTrust {
        pattern: "upload",
        cli_tool: NotApplicable, web_service: Untrusted, library: Untrusted, ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "file",
        cli_tool: Trusted, web_service: Untrusted, library: Untrusted, ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "user",
        cli_tool: NotApplicable, web_service: Untrusted, library: Untrusted, ci_pipeline: NotApplicable,
    },
];

/// Classify whether a set of user-input indicators (from a SecurityPattern) are
/// trusted or untrusted in the given threat model.
///
/// Returns `true` if ALL matched indicators are trusted (finding should be suppressed).
/// Returns `false` if ANY matched indicator is untrusted (finding should be reported).
/// Returns `None` if no threat model is configured.
pub fn should_suppress(
    config: &ThreatModelConfig,
    matched_indicators: &[&str],
) -> Option<bool> {
    let model_type = config.model_type?;

    // If no indicators matched, we can't determine trust — don't suppress.
    if matched_indicators.is_empty() {
        return Some(false);
    }

    for indicator in matched_indicators {
        let indicator_lower = indicator.to_lowercase();

        // Check user-defined overrides first.
        if config.trusted_sources.iter().any(|s| indicator_lower.contains(&s.to_lowercase())) {
            continue; // Trusted by user override.
        }
        if config.untrusted_sources.iter().any(|s| indicator_lower.contains(&s.to_lowercase())) {
            return Some(false); // Untrusted by user override.
        }

        // Check built-in table.
        let trust = lookup_trust(&indicator_lower, model_type);
        match trust {
            Untrusted => return Some(false),
            Trusted | NotApplicable => continue,
        }
    }

    // All matched indicators were trusted or N/A — suppress.
    Some(true)
}

fn lookup_trust(indicator: &str, model_type: ThreatModelType) -> TrustLevel {
    for entry in SOURCE_TRUST_TABLE {
        if indicator.contains(entry.pattern) {
            return match model_type {
                ThreatModelType::CliTool => entry.cli_tool,
                ThreatModelType::WebService => entry.web_service,
                ThreatModelType::Library => entry.library,
                ThreatModelType::CiPipeline => entry.ci_pipeline,
            };
        }
    }
    // Unknown indicator — treat as potentially untrusted.
    Untrusted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::CliTool),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        }
    }

    fn web_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::WebService),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        }
    }

    fn lib_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::Library),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        }
    }

    #[test]
    fn no_threat_model_returns_none() {
        let cfg = ThreatModelConfig::default();
        assert_eq!(should_suppress(&cfg, &["argv"]), None);
    }

    #[test]
    fn cli_tool_trusts_argv() {
        assert_eq!(should_suppress(&cli_config(), &["argv"]), Some(true));
    }

    #[test]
    fn cli_tool_trusts_stdin() {
        assert_eq!(should_suppress(&cli_config(), &["stdin"]), Some(true));
    }

    #[test]
    fn cli_tool_trusts_environ() {
        assert_eq!(should_suppress(&cli_config(), &["environ"]), Some(true));
    }

    #[test]
    fn cli_tool_does_not_trust_socket() {
        assert_eq!(should_suppress(&cli_config(), &["socket"]), Some(false));
    }

    #[test]
    fn web_service_does_not_trust_request() {
        assert_eq!(should_suppress(&web_config(), &["request"]), Some(false));
    }

    #[test]
    fn web_service_does_not_trust_query() {
        assert_eq!(should_suppress(&web_config(), &["query"]), Some(false));
    }

    #[test]
    fn web_service_trusts_environ() {
        assert_eq!(should_suppress(&web_config(), &["environ"]), Some(true));
    }

    #[test]
    fn library_trusts_nothing() {
        assert_eq!(should_suppress(&lib_config(), &["argv"]), Some(false));
        assert_eq!(should_suppress(&lib_config(), &["environ"]), Some(false));
        assert_eq!(should_suppress(&lib_config(), &["input"]), Some(false));
    }

    #[test]
    fn mixed_indicators_untrusted_wins() {
        // CLI: argv is trusted, but socket is untrusted — untrusted wins.
        assert_eq!(should_suppress(&cli_config(), &["argv", "socket"]), Some(false));
    }

    #[test]
    fn empty_indicators_not_suppressed() {
        assert_eq!(should_suppress(&cli_config(), &[]), Some(false));
    }

    #[test]
    fn user_override_trusted() {
        let mut cfg = web_config();
        cfg.trusted_sources = vec!["request".into()];
        assert_eq!(should_suppress(&cfg, &["request"]), Some(true));
    }

    #[test]
    fn user_override_untrusted() {
        let mut cfg = cli_config();
        cfg.untrusted_sources = vec!["argv".into()];
        assert_eq!(should_suppress(&cfg, &["argv"]), Some(false));
    }

    #[test]
    fn unknown_indicator_treated_as_untrusted() {
        assert_eq!(should_suppress(&cli_config(), &["some_unknown_source"]), Some(false));
    }

    #[test]
    fn ci_pipeline_trusts_argv_and_environ() {
        let cfg = ThreatModelConfig {
            model_type: Some(ThreatModelType::CiPipeline),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        };
        assert_eq!(should_suppress(&cfg, &["argv"]), Some(true));
        assert_eq!(should_suppress(&cfg, &["environ"]), Some(true));
    }
}
```

- [ ] **Step 2: Export module from `lib.rs`**

Add to `crates/apex-detect/src/lib.rs`:

```rust
pub mod threat_model;
```

- [ ] **Step 3: Run tests, verify pass**

```bash
cargo test -p apex-detect -- threat_model
```

- [ ] **Step 4: Commit**

```bash
git add crates/apex-detect/src/threat_model.rs crates/apex-detect/src/lib.rs
git commit -m "feat: add trust classification tables for threat-model-aware detection"
```

---

## Task 3: Pass threat model through `AnalysisContext`

**Files:**
- Modify: `crates/apex-detect/src/context.rs`
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add `threat_model` to `AnalysisContext`**

In `crates/apex-detect/src/context.rs`, add to the `AnalysisContext` struct:

```rust
pub threat_model: apex_core::config::ThreatModelConfig,
```

Update the Debug impl to include it:

```rust
.field("threat_model", &self.threat_model.model_type)
```

- [ ] **Step 2: Update all `AnalysisContext` construction sites**

In `crates/apex-cli/src/lib.rs` `run_audit()` (~line 1163), add:

```rust
threat_model: cfg.threat_model.clone(),
```

In `crates/apex-detect/src/pipeline.rs` `test_context()` (~line 241), add:

```rust
threat_model: apex_core::config::ThreatModelConfig::default(),
```

In `crates/apex-detect/src/context.rs` test `make_ctx` functions, add:

```rust
threat_model: apex_core::config::ThreatModelConfig::default(),
```

In `crates/apex-detect/src/detectors/security_pattern.rs` `make_ctx()` (~line 514), add:

```rust
threat_model: apex_core::config::ThreatModelConfig::default(),
```

Search for all other `AnalysisContext {` construction sites and add the field.

- [ ] **Step 3: Run full workspace tests to catch any missed sites**

```bash
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: pass ThreatModelConfig through AnalysisContext to detectors"
```

---

## Task 4: Integrate threat model into `SecurityPatternDetector`

**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

This is the core change. When a threat model is configured, the detector collects which user-input indicators actually matched, then asks `should_suppress()` whether those indicators are trusted. If all are trusted, the finding is suppressed entirely.

- [ ] **Step 1: Write failing test — CLI tool suppresses `Command::new` with format!**

Add to the test module:

```rust
#[tokio::test]
async fn cli_tool_threat_model_suppresses_trusted_command() {
    use apex_core::config::{ThreatModelConfig, ThreatModelType};
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("src/main.rs"),
        "fn run(user: &str) {\n    let cmd = format!(\"echo {}\", user);\n    Command::new(cmd);\n}\n".into(),
    );
    let mut ctx = make_ctx(files, Language::Rust);
    ctx.threat_model = ThreatModelConfig {
        model_type: Some(ThreatModelType::CliTool),
        trusted_sources: vec![],
        untrusted_sources: vec![],
    };
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    // In a CLI tool, format! with user string param is trusted — suppress.
    assert!(findings.is_empty(), "CLI tool should suppress Command::new with trusted input");
}
```

- [ ] **Step 2: Write failing test — web service does NOT suppress same pattern**

```rust
#[tokio::test]
async fn web_service_threat_model_keeps_command_finding() {
    use apex_core::config::{ThreatModelConfig, ThreatModelType};
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("src/handler.py"),
        "def handle(request):\n    cmd = request.get('cmd')\n    subprocess.call(cmd, shell=True)\n".into(),
    );
    let mut ctx = make_ctx(files, Language::Python);
    ctx.threat_model = ThreatModelConfig {
        model_type: Some(ThreatModelType::WebService),
        trusted_sources: vec![],
        untrusted_sources: vec![],
    };
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty(), "Web service should keep subprocess.call finding with request input");
}
```

- [ ] **Step 3: Write failing test — no threat model means no suppression**

```rust
#[tokio::test]
async fn no_threat_model_no_suppression() {
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("src/main.rs"),
        "fn run(user: &str) {\n    let cmd = format!(\"echo {}\", user);\n    Command::new(cmd);\n}\n".into(),
    );
    let ctx = make_ctx(files, Language::Rust);
    // Default context has no threat model → findings unchanged.
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert_eq!(findings.len(), 1);
}
```

- [ ] **Step 4: Run tests to confirm they fail**

```bash
cargo test -p apex-detect -- threat_model_
```

- [ ] **Step 5: Implement threat model check in `analyze()`**

Modify the `analyze` method in `SecurityPatternDetector`. After the existing indicator checks and severity adjustment (around line 470), add the threat model suppression check:

```rust
// Threat model suppression: if all matched indicators are trusted, skip.
if has_user_input {
    let matched: Vec<&str> = pattern.user_input_indicators
        .iter()
        .filter(|ind| has_indicator(&all_lines, line_num, &[ind]))
        .copied()
        .collect();
    if let Some(true) = crate::threat_model::should_suppress(
        &ctx.threat_model,
        &matched,
    ) {
        continue; // All matched sources are trusted in this threat model.
    }
}
// Also check: if no user input was found but pattern is inherently dangerous,
// suppress in CLI/CI contexts where the sink itself is trusted.
if !has_user_input && ctx.threat_model.model_type.is_some() {
    let sink_indicators = &[pattern.sink];
    if let Some(true) = crate::threat_model::should_suppress(
        &ctx.threat_model,
        sink_indicators,
    ) {
        continue;
    }
}
```

The key insight: we collect which *specific* indicators matched in the context window, then ask the trust table whether those specific indicators are trusted. This avoids the current problem where `Command::new("cargo")` flags because `format!` appears nearby.

- [ ] **Step 6: Run tests, verify pass**

```bash
cargo test -p apex-detect -- threat_model_
cargo test -p apex-detect  # all existing tests still pass
```

- [ ] **Step 7: Commit**

```bash
git add crates/apex-detect/src/detectors/security_pattern.rs
git commit -m "feat: SecurityPatternDetector suppresses findings based on threat model trust"
```

---

## Task 5: Build CPG for Python in `run_audit`

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

When the target language is Python and a threat model is configured, build the CPG and pass it through `AnalysisContext.cpg`. This enables precise taint-flow verification in future detector enhancements.

- [ ] **Step 1: Add CPG construction to `run_audit()`**

In `crates/apex-cli/src/lib.rs`, after building `source_cache` (~line 1161) and before constructing `AnalysisContext`:

```rust
// Build CPG for Python targets when threat model is configured
let cpg = if lang == Language::Python && cfg.threat_model.model_type.is_some() {
    let mut cpg = apex_cpg::Cpg::new();
    for (path, source) in &source_cache {
        if let Err(e) = apex_cpg::builder::build_python_cpg_into(&mut cpg, source, path) {
            debug!(file = %path.display(), error = %e, "CPG build skipped for file");
        }
    }
    // Compute reaching definitions for taint analysis
    apex_cpg::reaching_def::add_reaching_defs(&mut cpg);
    info!(nodes = cpg.node_count(), edges = cpg.edge_count(), "CPG built for taint analysis");
    Some(Arc::new(cpg))
} else {
    None
};
```

Then update the `AnalysisContext` construction to use `cpg` instead of `None`.

- [ ] **Step 2: Verify the CPG builder API matches**

Check that `build_python_cpg_into`, `add_reaching_defs`, `node_count`, `edge_count` exist on the `Cpg` type. If the API differs (e.g., function names), adjust Step 1 to match the actual API. The CPG builder was created in the Task 3 of the mechanism-reuse plan — read `crates/apex-cpg/src/builder.rs` and `crates/apex-cpg/src/reaching_def.rs` to confirm.

- [ ] **Step 3: Run tests**

```bash
cargo test -p apex-cli
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat: build CPG for Python targets when threat model is configured"
```

---

## Task 6: Create `/apex-threat-model` wizard command

**Files:**
- Create: `.claude/commands/apex-threat-model.md`

- [ ] **Step 1: Write the wizard command**

```markdown
# APEX Threat Model Wizard

Interactive wizard to define the threat model for a repository. Writes the `[threat_model]` section to `apex.toml`.

## Usage
```
/apex-threat-model [target]
```
Examples:
- `/apex-threat-model` — configure threat model for current directory
- `/apex-threat-model /tmp/my-project`

## Instructions

Parse `$ARGUMENTS`: target path. Default: `.`

### Step 1: Detect existing config

```bash
cat <TARGET>/apex.toml 2>/dev/null
```

If `[threat_model]` section already exists, show current config and ask:
"This repo already has a threat model configured: `type = <current>`. Would you like to update it?"

### Step 2: Ask software type

Ask the user (present as numbered options):

```
What type of software is this repository?

1. **CLI tool** — command-line application, scripts, dev tools
   Trusts: argv, env vars, config files, stdin
   Example: cargo, git, curl

2. **Web service** — HTTP API, web app, microservice
   Trusts: env vars, config files
   Untrusts: request data, query params, form data, headers

3. **Library** — reusable package consumed by other software
   Trusts: nothing (all input is from callers you don't control)

4. **CI pipeline** — build scripts, deployment tools, GitHub Actions
   Trusts: argv, env vars, config files
   Untrusts: network input, downloaded artifacts
```

### Step 3: Ask about custom trust overrides

Based on the selected type, ask:

"Are there any input sources you'd like to **additionally trust** beyond the defaults for a <type>? (e.g., a specific API you control)"

Then ask:

"Are there any input sources you'd like to mark as **untrusted** even though they're normally trusted for a <type>? (e.g., environment variables set by untrusted CI runners)"

### Step 4: Generate and write config

Build the `[threat_model]` section:

```toml
[threat_model]
type = "<selected-type>"
trusted_sources = ["custom1", "custom2"]    # only if user provided any
untrusted_sources = ["custom3"]             # only if user provided any
```

If `apex.toml` exists, append the section (or replace existing `[threat_model]`).
If `apex.toml` doesn't exist, create it with just the `[threat_model]` section.

### Step 5: Show impact preview

After writing the config, run a quick audit to show the difference:

```bash
# Count findings without threat model (temporarily)
APEX_HOME=$(git rev-parse --show-toplevel 2>/dev/null || echo ".")
```

Show:
```
Threat model configured: <type>

This will affect `apex audit` results:
- Sources like <list> are now treated as **trusted** (findings suppressed)
- Sources like <list> remain **untrusted** (findings kept)

Run `apex audit` to see the updated results.
```

### Step 6: Suggest next steps

```
Next steps:
  apex audit --target <TARGET> --lang <LANG>    — run audit with threat model applied
  Edit apex.toml to fine-tune trusted/untrusted sources
```
```

- [ ] **Step 2: Commit**

```bash
git add .claude/commands/apex-threat-model.md
git commit -m "feat: add /apex-threat-model wizard command"
```

---

## Task 7: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Mark completed items, add CPG-for-other-languages items**

Update the Threat-Model-Aware Detection section. Mark completed items with `[x]` and add new items:

```markdown
## Threat-Model-Aware Detection

Current detectors have ~97% false positive rate on APEX itself because they don't know the software's trust boundaries. `Command::new("cargo")` in a CLI tool is not command injection.

- [x] Add `[threat_model]` section to `apex.toml` — `type = "cli-tool" | "web-service" | "library" | "ci-pipeline"`
- [x] Classify sources by trust level per threat model (e.g. `sys.argv` trusted in CLI, untrusted in web service)
- [ ] Wire CPG taint analysis into detectors — only flag flows from **untrusted** sources to sinks
- [x] Suppress pattern-match findings when no taint flow from untrusted source exists
- [ ] ~~Add `--threat-model` CLI flag to `apex audit`~~ — threat model lives in repo's `apex.toml`

### CPG Builders for Other Languages

CPG-based taint analysis currently only works for Python (via tree-sitter-python).
Adding CPG builders for other languages enables precise flow-based suppression.

- [ ] Rust CPG builder — parse with tree-sitter-rust, build AST+CFG+ReachingDef edges
- [ ] JavaScript/TypeScript CPG builder — parse with tree-sitter-javascript/typescript
- [ ] Ruby CPG builder — parse with tree-sitter-ruby
- [ ] C CPG builder — parse with tree-sitter-c
- [ ] Java CPG builder — parse with tree-sitter-java
```

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: update TODO with threat model progress and CPG builder backlog"
```

---

## Verification

After all tasks:

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Expected outcomes:
- **Task 1:** `apex.toml` with `[threat_model]` section parses correctly; missing section defaults to `None`
- **Task 2:** Trust tables classify sources correctly per threat model type
- **Task 3:** `AnalysisContext` carries threat model through to detectors
- **Task 4:** `SecurityPatternDetector` suppresses findings where all matched sources are trusted
- **Task 5:** CPG is built for Python targets when threat model is configured
- **Task 6:** `/apex-threat-model` wizard guides config creation
- **Task 7:** TODO reflects completed and remaining work

## Dependency Graph

```
Task 1 (config types) ──→ Task 2 (trust tables) ──→ Task 4 (detector integration)
Task 1 ──────────────────→ Task 3 (context wiring) ──→ Task 4
Task 1 ──────────────────→ Task 5 (CPG in run_audit)
Task 6 (wizard command) — independent
Task 7 (TODO update) — independent
```

Tasks 1 must be first. Tasks 2 and 3 can be parallel after Task 1. Task 4 depends on both 2 and 3. Tasks 5, 6, 7 are independent of each other but 5 depends on Task 3.
