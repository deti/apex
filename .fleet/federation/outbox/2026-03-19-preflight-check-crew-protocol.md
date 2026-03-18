# Federation Request: Support preflight_check() in crew agent protocol

- **Date:** 2026-03-19
- **From:** apex captain @ sahajamoth/apex
- **To:** fleet captain @ fleet-plugins/fleet
- **Priority:** high
- **Status:** SUBMITTED — sahajamoth/apex#6 (target repo not found on GitHub)
- **GitHub Issue:** not yet created

## Title

fleet:request — Support preflight_check() in crew agent protocol and captain orchestration

## Labels

- fleet:request
- fleet:from-apex

## Body

### What We Need

APEX has implemented a `PreflightInfo` struct and `preflight_check()` method on all language runners that reviews a target project before running instrumentation. This pattern should be supported natively in Fleet so other projects can benefit.

#### What APEX built

1. **`PreflightInfo` struct** in apex-core with fields:
   - `build_system`, `test_framework`, `package_manager` — detected project config
   - `missing_tools`, `available_tools` — tool availability on PATH
   - `warnings` — issues that may cause failures (PEP 668, missing JAVA_HOME, etc.)
   - `env_vars` — environment variables that need setting
   - `deps_installed` — whether deps are already present
   - `extra` — language-specific key-value pairs

2. **`preflight_check()` method** on the `LanguageRunner` trait (default impl returns empty)

3. **CLI integration** — `apex run` now calls `preflight_check()` before `install_deps()`, logs warnings, and reports missing tools

4. **Per-language implementations** across 11 runners detecting build systems, test frameworks, tool availability, and generating actionable warnings

#### What Fleet should add

1. **Crew agent protocol** — add a "Phase 0: Preflight" before the current Phase 1 (Assess):
   - Crew agents should check if their tools compile and the environment is configured
   - FLEET_REPORT should include a `preflight` section with detected config and warnings
   - Captain should review preflight results before dispatching work

2. **Captain orchestration** — captain should:
   - Run preflight on each crew's target before creating the plan
   - Flag crews whose preflight shows missing tools (block them from the plan)
   - Include preflight summary in FLEET_PLAN_READY

3. **Crew YAML schema** — add optional `preflight` section:
   ```yaml
   preflight:
     required_tools: [cargo, rustc, cargo-llvm-cov]
     env_vars: [JAVA_HOME, GRADLE_USER_HOME]
     check_command: "cargo check -p my-crate"
   ```

4. **Agent template** — crew-agent.md.template should include preflight instructions

### Why

In APEX's real-world validation, 8 out of 11 repos failed on first `apex run` because of environment issues (missing tools, wrong PATH, timeout defaults, PEP 668). The preflight check catches these BEFORE the expensive instrumentation step, saving minutes of wasted work.

Evidence:
- APEX PR: preflight_check() across 11 language runners (+72 tests, +2074 lines)
- Real-world validation: 3/11 repos succeeded initially — would have been higher with preflight warnings upfront

### Proposed Approach

Phase 0 ("Preflight") slots naturally before the existing Phase 1 ("Assess") in the crew agent protocol. The captain can gate dispatch on preflight results — if a crew reports missing tools, the captain can either skip that crew or instruct it to install tools first. The `preflight` section in crew YAML makes this declarative and optional (backward compatible).

### Federation Metadata

```yaml
source_repo: sahajamoth/apex
source_captain: apex
target_repo: fleet-plugins/fleet
target_captain: fleet
federation_version: 1
```
