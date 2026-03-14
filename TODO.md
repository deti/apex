# APEX TODO

## Threat-Model-Aware Detection

Current detectors have ~97% false positive rate on APEX itself because they don't know the software's trust boundaries. `Command::new("cargo")` in a CLI tool is not command injection.

- [x] Add `[threat_model]` section to `apex.toml` — `type = "cli-tool" | "web-service" | "library" | "ci-pipeline"`
- [x] Classify sources by trust level per threat model (e.g. `sys.argv` trusted in CLI, untrusted in web service)
- [x] Suppress pattern-match findings when all matched indicators are trusted for the threat model
- [ ] Wire CPG taint analysis into detectors — only flag flows from **untrusted** sources to sinks (requires CPG integration)
- [x] `/apex-threat-model` wizard command for interactive threat model setup (replaces CLI flag — config lives in repo)

## CPG Integration

- [ ] Build CPG for Python in `run_audit` pipeline (tree-sitter → AST+CFG+REACHING_DEF)
- [ ] Build CPG for JavaScript/TypeScript
- [ ] Build CPG for Rust
- [ ] Build CPG for Java
- [ ] Build CPG for Ruby
- [ ] Build CPG for C/C++
- [ ] Use CPG taint flows in SecurityPatternDetector (replace substring matching with reachability)

## JS/TS Concolic — Not Yet Covered

- [ ] Dynamic `eval()` / `new Function()` — not statically analyzable, needs runtime tracing
- [ ] Proxy/Reflect metaprogramming — intercepted property access creates invisible branches
- [ ] Async control flow constraints — Promise branching, `await` paths, race conditions
