# APEX Research Overview

## 6 Digs Completed (2026-03-20 to 2026-03-21)

### Round 1: Widen the scope

**Dig 1 — Binary instrumentation alternatives** (research-analyst)
- Evaluated: Frida, DynamoRIO, Intel Pin, QEMU, rr, Valgrind, GDB breakpoints, Intel PT, Apple Processor Trace, LD_PRELOAD, DTrace
- **Winner: Frida Stalker** — only mature tool for macOS ARM64, no recompilation, basic-block granularity, `frida-gum` Rust crate exists
- Apple Processor Trace (M4+) is the future — zero overhead, every branch recorded
- rr/Valgrind/DTrace — dead ends on macOS ARM64

**Dig 2 — Alternative compilers, IR, VMs** (research-analyst)
- Evaluated: tree-sitter, GraalVM, WASM IR, Cranelift, LLVM bitcode, mutation testing, AI coverage estimation
- **Top 2: Tree-sitter branch extraction + mutation testing as coverage proxy**
- Tree-sitter: parse 40+ languages without compiling, identify all branch points
- Mutation testing: "is this code effectively tested?" — strictly stronger than line coverage
- GraalVM: strong for Python/JS/Ruby (built-in `--coverage` flag), but 500MB runtime
- **Compound Coverage Oracle concept**: combine multiple signals with confidence weighting

**Dig 3 — Industry survey, awesome lists, competitors** (research-analyst)
- Evaluated: Codecov, Coveralls, SonarQube, Diffblue, Qodo, ClusterFuzz, RESTler, Sapienz
- **Key finding: NO production tool compiles the target project**
- Codecov/SonarQube = report parsers. JaCoCo = JVM agent. coverage.py = sys.settrace wrapper
- APEX's approach (compile + instrument) is fundamentally wrong for `apex analyze`
- **Paradigm shifts found:**
  - ConcoLLMic (S&P 2026): LLM replaces SMT solver, 115-233% more branches than KLEE
  - Meta ACH (FSE 2025): mutation-guided LLM test gen, 73% engineer acceptance
  - TestForge (CMU 2025): agentic test gen, $0.63/file, 84% pass rate

### Round 2: Go deeper on top leads

**Dig 4 — LLM concolic integration** (ai-engineer)
- LLM as third solver in existing `PortfolioSolver`: gradient → Z3 → LLM
- ~60 lines of new code, plugs into existing trait system
- Cost: ~$0.21/project (LLM only fires on ~15 hardest branches after gradient + Z3 fail)
- Two phases: concolic (reachability) + mutation (correctness)
- `ConditionTree.to_source_constraint()` for readable LLM prompts

**Dig 5 — Compound Coverage Oracle design** (fullstack-developer)
- Log-odds Bayesian combination of multiple coverage signals
- Confidence tiers: Instrumented (1.0) > Import (0.95) > Mutation (0.85) > Concolic (0.80) > Static (0.50) > AI (0.35)
- Graceful degradation: 4 tiers from full runtime to static-only
- Severity re-scoring: `adjusted = base * (2.0 - coverage_confidence)`
- Tree-sitter replaces 8 hand-rolled regex parsers with one AST walker

**Dig 6 — DX and adoption** (product-manager)
- SARIF output EXISTS in code (`sarif.rs`) but never exposed to users
- 7 items for 10x adoption: SARIF, lcov export, markdown, badge, changed-files filter, ci-report, GitHub Action
- "The PR comment is the product" — Codecov's $30M business is a PR comment
- Zero-config GitHub Action (no API key, no cloud) = distribution advantage
- v0.4.0 should be adoption, not features

---

## Key Strategic Insights

### 1. Coverage is a spectrum, not a boolean
```
100% confidence: instrumented coverage (tests ran with probes)
 95% confidence: imported coverage (user-provided data)
 85% confidence: mutation survived (code not effectively tested)
 80% confidence: concolic reached (solver found path to branch)
 50% confidence: test-name heuristic (test_auth → validate_token)
 35% confidence: AI estimation (LLM predicts coverage)
  0% confidence: no data
```

### 2. APEX should be a coverage CONSUMER, not a coverage PRODUCER
The industry separates these roles. Codecov/SonarQube consume. coverage.py/JaCoCo produce. APEX tries to be both and fails at producing. The fix: consume first (`--coverage-file`), produce when possible (WRAP/INSTRUMENT).

### 3. LLM is the universal solver
Z3 needs formal semantics per language. Gradient descent needs differentiable conditions. LLM understands ANY language's conditions from source text. ConcoLLMic proved this at S&P 2026. APEX already has the portfolio architecture — LLM is just a third solver.

### 4. The PR comment is the product
Features don't drive adoption. Integration does. Codecov's entire business is a PR comment. APEX has more capabilities but zero presence where merge decisions happen.

### 5. Mutation testing > line coverage
"Did the test execute this line?" is weaker than "Would the test catch a bug on this line?" Mutation testing answers the stronger question. Meta's ACH system (73% engineer acceptance) proves the approach works at scale.

---

## Research Documents

| Document | Location |
|----------|----------|
| Binary instrumentation survey | `docs/research/2026-03-20-coverage-without-recompilation.md` |
| Industry tool architecture | `docs/research/coverage-tool-architecture-survey.md` |
| Coverage modes strategy | `docs/research/2026-03-21-coverage-modes-strategy.md` |
| Dig 1 detailed findings | `.claude/plans/sequential-sprouting-honey-agent-ab128160d41bf7e37.md` |
| Dig 2 detailed findings | `.claude/plans/sequential-sprouting-honey-agent-a40f23e51e5d3e017.md` |
| Dig 3 detailed findings | `.claude/plans/sequential-sprouting-honey-agent-a9d012ca8069dd11e.md` |
| Dig 4 detailed findings | `.claude/plans/sequential-sprouting-honey-agent-a0e97d05792b8efad.md` |
| Dig 5 detailed findings | `.claude/plans/sequential-sprouting-honey-agent-a29c5af861939b4b5.md` |
| Dig 6 detailed findings | `.claude/plans/sequential-sprouting-honey-agent-a42e69e969aca37fa.md` |

---

## Awesome Lists Referenced

- [awesome-static-analysis](https://github.com/analysis-tools-dev/static-analysis)
- [awesome-fuzzing (cpuu)](https://github.com/cpuu/awesome-fuzzing)
- [awesome-fuzzing (secfigo)](https://github.com/secfigo/Awesome-Fuzzing)
- [awesome-directed-fuzzing](https://github.com/strongcourage/awesome-directed-fuzzing)
- [awesome-symbolic-execution](https://github.com/ksluckow/awesome-symbolic-execution)
- [awesome-binary-analysis](https://github.com/open-crs/awesome-binary-analysis)
- [awesome-mutation-testing](https://github.com/theofidry/awesome-mutation-testing)
- [awesome-compilers](https://github.com/aalhour/awesome-compilers)
- [awesome-graal](https://github.com/neomatrix369/awesome-graal)
- [awesome-wasm-tools](https://github.com/vshymanskyy/awesome-wasm-tools)

### Round 3: Detector depth

**Dig 7 — CWE gap analysis** (security-engineer)
- APEX covers 52% of CWE Top 25 (2024) — 11/25 CWEs with 2 partial
- **Biggest gaps:** CSRF (CWE-352, #4 rank, zero coverage), XSS depth (CWE-79, #1, partial), file upload (CWE-434, #10, zero), info exposure (CWE-200, #17, zero), auth failures (CWE-287, #14, zero)
- APEX's moat: concurrency bugs, Rust-specific detectors, operational reliability — none covered by competitors
- APEX's weakness: web application security (CSRF, XSS, file upload, auth flows)
- Projected: 52% → 72% after 2 weeks → 92% after 5 weeks
- Bearer (Rust CLI, open source) is the closest architectural competitor

**Dig 8 — Taint analysis SOTA** (research-analyst)
- APEX's taint engine is at Semgrep OSS level but with a weaker parser
- **Three quick wins:** (1) tree-sitter parser replacement (2-3 days), (2) wire existing `TaintSummary` for inter-procedural (minimal code change), (3) HashMap for graph storage (unblocks scale)
- Intra-procedural taint catches ~70% of real injection vulns; inter-procedural adds ~20%
- The `TaintSummary`, `SummaryCache`, and `apply_summary_at_callsite()` already exist but are NOT wired into the main taint engine
- IFDS framework would formalize the ad-hoc backward BFS — consider if precision becomes bottleneck

**Dig 9 — Novel detector approaches** (ai-engineer)
- **Build now:** Type-state analysis (track object lifecycle: File opened/closed, Mutex locked/unlocked) — zero training data needed, 5-10% FP rate, catches CWE-416/415/401
- **Build next:** Differential taint flow analysis (compare CPGs across versions, flag new attack surface) — natural extension of `apex diff` + `taint.rs`
- **Park:** GNN and LLM vulnerability classification — blocked on training dataset problem (8-12 weeks, uncertain ROI)
- **Key insight:** approaches requiring ZERO training data deliver value fastest. ML approaches are blocked on a dataset problem unsolved industry-wide

---

## Strategic Synthesis (9 Digs)

### The 5 highest-impact changes for APEX

1. **GitHub Action + SARIF** (Dig 6) — 10x adoption, connects existing code to users, 1-3 days each
2. **LLM concolic solver** (Dig 4) — paradigm shift, ~60 lines, plugs into existing PortfolioSolver, $0.21/project
3. **Tree-sitter CPG parser** (Dig 8) — fixes taint engine's biggest weakness, 2-3 days, replaces 8 regex parsers
4. **CSRF + XSS + file upload detectors** (Dig 7) — CWE Top 25 coverage 52% → 72%, 2 weeks
5. **Type-state analysis** (Dig 9) — novel detector class, zero training data, catches resource leaks/use-after-free

### The 3 things NOT worth building now

1. GNN/ML vulnerability detection — no training dataset
2. Full IFDS taint framework — pragmatic summary-wiring achieves 80% of the value
3. Custom VS Code extension — Coverage Gutters works with lcov export for free

---

## Key Papers

- ConcoLLMic (S&P 2026) — LLM-powered concolic execution
- COTTONTAIL (S&P 2026) — LLM concolic for structured inputs
- AutoBug (OOPSLA 2025) — LLM symbolic execution on consumer hardware
- Meta ACH (FSE 2025) — Mutation-guided LLM test generation
- TestForge (CMU 2025) — Agentic file-level test generation
- CoverUp (2024) — Coverage-guided LLM test refinement
- ChatAFL (NDSS 2024) — LLM-guided protocol fuzzing
- Wizard Engine (ASPLOS 2024) — Non-intrusive WASM instrumentation
- Predicting Coverage without Execution (arXiv 2023) — AI coverage estimation
