<!-- status: ACTIVE -->

# Performance Test Generation — Resource-Guided Fuzzing, Complexity Estimation, ReDoS Detection

**Goal:** Add performance-oriented test generation to APEX: worst-case input generation
using resource-guided fuzzing (PerfFuzz approach), resource consumption profiling during
test execution, empirical algorithmic complexity estimation, ReDoS detection, and
configurable throughput/latency assertions. Enables APEX to find inputs that make code
slow, code paths that consume excessive resources, and scenarios that enable DoS attacks.

**Bug classes caught:** Algorithmic complexity vulnerabilities, ReDoS (CWE-1333),
resource exhaustion (CWE-400), hash collision DoS, quadratic accumulation, memory
leaks under load, performance regressions.

## Current State

| Component | Status | Location |
|-----------|--------|----------|
| Coverage-guided fuzzing | Done | `crates/apex-fuzz/src/lib.rs` — `FuzzStrategy` maximizes branch coverage |
| Semantic feedback (branch + semantic) | Done | `crates/apex-fuzz/src/semantic_feedback.rs` — configurable weight blend |
| Per-edge tracking concept | Partial | `crates/apex-fuzz/src/cmplog.rs` — CmpLog tracks per-branch comparisons, but not execution counts |
| Corpus energy scheduling | Done | `crates/apex-fuzz/src/corpus.rs` — Explore/Fast/Rare power schedules |
| Ensemble fuzzing | Done | `crates/apex-fuzz/src/ensemble.rs` — parallel strategy execution with shared corpus |
| MOpt/PSO/DE/Thompson schedulers | Done | `crates/apex-fuzz/src/scheduler.rs`, `de_scheduler.rs`, `thompson.rs` |
| Directed fuzzing (AFLGo) | Done | `crates/apex-fuzz/src/directed.rs` — simulated annealing energy |
| ExecutionResult.duration_ms | Done | `crates/apex-core/src/types.rs:466` — wall-clock time captured |
| ExecutionResult resource metrics | **Not done** | No memory, allocation count, instruction count, or CPU time |
| Finding with CWE-400 | Partial | Some detectors emit CWE-400 (regex-in-loop, string-concat-in-loop) |
| FindingCategory::PerformanceRisk | **Not done** | No dedicated performance category |
| Evidence::PerformanceProfile | **Not done** | No performance evidence variant |
| PerfBaseline/PerfDiffReport | Done | `crates/apex-detect/src/perf_diff.rs` — baseline comparison logic |
| regex-in-loop detector | Done | `crates/apex-detect/src/detectors/regex_in_loop.rs` — CWE-400 |
| string-concat-in-loop detector | Done | `crates/apex-detect/src/detectors/string_concat_in_loop.rs` — CWE-400 |
| ReDoS static analysis | **Not done** | No regex backtracking analysis |
| Algorithmic complexity detector | **Not done** | No nested-loop/recursion complexity detection |
| Resource-guided fuzzing | **Not done** | Feedback maximizes coverage, not resource consumption |
| Complexity estimation | **Not done** | No input-size scaling + curve fitting |
| `apex perf` CLI command | **Not done** | No performance subcommand |
| Performance SLO verification | **Not done** | No SLO assertion framework |
| Resource profiling per language | **Not done** | No tracemalloc/perf_hooks/getrusage integration |

### Key Architecture Points

- **Strategy trait** (`crates/apex-core/src/traits.rs:10`): `suggest_inputs()` + `observe()` — the
  extension point for PerfFuzzStrategy. Same trait, different optimization objective.
- **FuzzStrategy** (`crates/apex-fuzz/src/lib.rs:55`): Only adds to corpus when `new_branches`
  is non-empty. PerfFuzzStrategy must instead add when resource consumption exceeds prior maximum.
- **ExecutionResult** (`crates/apex-core/src/types.rs:460`): Has `duration_ms`, `new_branches`,
  `stdout`, `stderr`, `input`. Needs `resource_metrics: Option<ResourceMetrics>`.
- **Corpus** (`crates/apex-fuzz/src/corpus.rs`): Energy-weighted sampling. Needs a `ResourceMax`
  power schedule that energizes entries by peak resource consumption.
- **Detector trait** (`crates/apex-detect/src/lib.rs:58`): `async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>`.
  ReDoS and complexity detectors implement this directly.
- **Sandbox** (`crates/apex-core/src/traits.rs:19`): `async fn run(&self, input: &InputSeed) -> Result<ExecutionResult>`.
  Must be extended to populate ResourceMetrics in the result.
- **AgentCluster** (`crates/apex-agent/src/orchestrator.rs`): Runs strategies in parallel,
  merges coverage, broadcasts results. PerfFuzzStrategy plugs in alongside FuzzStrategy.

## File Map

| Crew | Files |
|------|-------|
| foundation | `crates/apex-core/src/types.rs` (ResourceMetrics, ComplexityClass, extend ExecutionResult), `crates/apex-detect/src/finding.rs` (FindingCategory::PerformanceRisk, Evidence::PerformanceProfile) |
| security-detect | `crates/apex-detect/src/detectors/redos.rs` (new), `crates/apex-detect/src/detectors/algorithmic_complexity.rs` (new), `crates/apex-detect/src/detectors/hash_collision_risk.rs` (new), `crates/apex-detect/src/detectors/mod.rs`, `crates/apex-detect/src/pipeline.rs` |
| exploration | `crates/apex-fuzz/src/perf_feedback.rs` (new), `crates/apex-fuzz/src/perf_strategy.rs` (new), `crates/apex-fuzz/src/corpus.rs` (ResourceMax schedule), `crates/apex-fuzz/src/lib.rs` (re-exports) |
| runtime | `crates/apex-sandbox/src/process.rs` (resource measurement), `crates/apex-sandbox/src/python.rs` (tracemalloc), `crates/apex-sandbox/src/javascript.rs` (perf_hooks), `crates/apex-index/src/regex_extract.rs` (new) |
| intelligence | `crates/apex-agent/src/complexity_estimator.rs` (new), `crates/apex-agent/src/router.rs` (perf routing), `crates/apex-synth/src/perf_prompts.rs` (new) |
| platform | `crates/apex-cli/src/lib.rs` (Perf subcommand), `crates/apex-cli/src/perf.rs` (new handler module) |
| mcp-integration | `crates/apex-rpc/src/mcp.rs` (perf tools) |

---

## Wave 1 — Foundation Types + Static Detectors (no dependencies)

These tasks establish the type system extensions and static-analysis detectors that all
later waves depend on. No cross-crew dependencies within this wave.

### Task 1.1 — foundation crew
**Files:** `crates/apex-core/src/types.rs`
**Summary:** Add ResourceMetrics type and extend ExecutionResult

Resource measurement is the substrate everything else builds on. Every sandbox, strategy,
and report needs these types.

- [ ] Add `ResourceMetrics` struct:
  ```rust
  #[derive(Debug, Clone, Default, Serialize, Deserialize)]
  pub struct ResourceMetrics {
      pub wall_time_ms: u64,
      pub cpu_time_ms: Option<u64>,
      pub peak_memory_bytes: Option<u64>,
      pub allocation_count: Option<u64>,
      pub instruction_count: Option<u64>,
      /// Per-edge execution counts for PerfFuzz feedback.
      /// Key: edge/branch identifier, Value: execution count.
      #[serde(default, skip_serializing_if = "HashMap::is_empty")]
      pub edge_counts: HashMap<u64, u64>,
  }
  ```
- [ ] Add `resource_metrics: Option<ResourceMetrics>` field to `ExecutionResult` (with `#[serde(default, skip_serializing_if = "Option::is_none")]`)
- [ ] Add `ComplexityClass` enum:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  pub enum ComplexityClass {
      Constant,     // O(1)
      Logarithmic,  // O(log n)
      Linear,       // O(n)
      Linearithmic, // O(n log n)
      Quadratic,    // O(n²)
      Cubic,        // O(n³)
      Exponential,  // O(2^n)
      Unknown,
  }
  ```
- [ ] Add `ComplexityEstimate` struct:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ComplexityEstimate {
      pub class: ComplexityClass,
      pub confidence: f64,           // 0.0–1.0
      pub r_squared: f64,            // Goodness-of-fit
      pub samples: Vec<(usize, f64)>, // (input_size, measurement)
  }
  ```
- [ ] Update existing tests that construct `ExecutionResult` to include `resource_metrics: None`
- [ ] Add unit tests for `ResourceMetrics` serialization roundtrip
- [ ] Add unit tests for `ComplexityClass` serialization roundtrip
- [ ] Run `cargo nextest run -p apex-core` — confirm pass
- [ ] Commit

### Task 1.2 — foundation crew
**Files:** `crates/apex-detect/src/finding.rs`
**Summary:** Add PerformanceRisk category and PerformanceProfile evidence variant

- [ ] Add `PerformanceRisk` variant to `FindingCategory` enum (after `TestDataQuality`)
- [ ] Add `PerformanceProfile` variant to `Evidence` enum:
  ```rust
  PerformanceProfile {
      function: String,
      metric: String,          // "wall_time_ms", "peak_memory_bytes", etc.
      baseline_value: Option<f64>,
      measured_value: f64,
      input_description: String, // Human-readable description of the worst-case input
  },
  ```
- [ ] Add `FindingCategory::PerformanceRisk` to the `finding_category_all_variants` test
- [ ] Add serialization test for `Evidence::PerformanceProfile`
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass (may need to fix other tests that exhaustively match FindingCategory)
- [ ] Commit

### Task 1.3 — security-detect crew
**Files:** `crates/apex-detect/src/detectors/redos.rs` (new), `crates/apex-detect/src/detectors/mod.rs`, `crates/apex-detect/src/pipeline.rs`
**Summary:** ReDoS detector — static analysis for catastrophic backtracking in regex

This is the highest-value static detector: regex with nested quantifiers or overlapping
alternatives cause exponential backtracking. CWE-1333 (child of CWE-400).

Detection algorithm (NFA-based):
1. Extract all regex literals from source code (language-aware patterns)
2. Parse each regex to identify vulnerable patterns:
   - Nested quantifiers: `(a+)+`, `(a*)*`, `(a+)*`
   - Overlapping alternatives with quantifier: `(a|a)+`, `(\w|\d)+`
   - Quantified groups with overlapping tails: `(a+)+b` vs input "aaa...c"
3. For each flagged regex, generate a worst-case input string
4. Optionally verify dynamically (execute regex with worst-case input, measure time)

Language-specific regex extraction patterns:
- **Python**: `re.compile(r"...")`, `re.match(r"...", ...)`, `re.search(...)`
- **JavaScript**: `/pattern/flags`, `new RegExp("pattern")`, `RegExp("pattern")`
- **Rust**: `Regex::new(r"...")`, `Regex::new("...")`
- **Go**: `regexp.Compile("...")`, `regexp.MustCompile("...")`
- **Java**: `Pattern.compile("...")`, `String.matches("...")`
- **Ruby**: `/pattern/`, `Regexp.new("pattern")`

- [ ] Create `crates/apex-detect/src/detectors/redos.rs` with `ReDoSDetector` struct
- [ ] Implement `Detector` trait: scan `ctx.source_cache` for regex patterns per language
- [ ] Implement NFA ambiguity check: detect nested quantifiers, overlapping alternatives
- [ ] For each vulnerable regex, generate a concrete worst-case input string (pump string)
- [ ] Emit `Finding` with:
  - `category: FindingCategory::PerformanceRisk`
  - `cwe_ids: vec![1333, 400]`
  - `severity`: High for exponential, Medium for polynomial backtracking
  - `evidence`: `PerformanceProfile` with worst-case input description
  - `fix`: `Fix::CodePatch` suggesting atomic grouping or possessive quantifier
- [ ] Add `pub mod redos; pub use redos::ReDoSDetector;` to `detectors/mod.rs`
- [ ] Register in `pipeline.rs`: `if cfg.enabled.contains(&"redos".into()) { detectors.push(Box::new(ReDoSDetector)); }`
- [ ] Add to `default_audit_enabled()` set
- [ ] Write tests: known-vulnerable regexes (`(a+)+$`, `(a|a)*$`, `(\d+\.?\d*|\.\d+)`) yield findings
- [ ] Write tests: safe regexes (`^[a-z]+$`, `^\d{3}-\d{4}$`) yield no findings
- [ ] Write tests: multi-language regex extraction (Python re.compile, JS /pattern/, Rust Regex::new)
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass
- [ ] Commit

### Task 1.4 — security-detect crew
**Files:** `crates/apex-detect/src/detectors/algorithmic_complexity.rs` (new), `crates/apex-detect/src/detectors/mod.rs`, `crates/apex-detect/src/pipeline.rs`
**Summary:** Static detector for algorithmic complexity risks

Detect code patterns that indicate potential super-linear complexity:

1. **Nested loops with data-dependent bounds** — `for i in items: for j in items:` → O(n²)
2. **Recursive functions without memoization** — `fn fib(n): fib(n-1) + fib(n-2)` → O(2^n)
3. **Quadratic string accumulation** — already covered by `string-concat-in-loop`, but this
   detector adds CWE-400 classification and complexity estimation
4. **Sorting with comparison callbacks** — user-controlled comparators can be made slow
5. **Unbounded recursion depth** — no base case or stack depth limit

- [ ] Create `crates/apex-detect/src/detectors/algorithmic_complexity.rs`
- [ ] Implement nested-loop detection using `find_loop_scopes()` utility from `detectors/util.rs`
- [ ] Implement recursive-without-memo detection: find functions that call themselves without cache/memo patterns
- [ ] Emit findings with `FindingCategory::PerformanceRisk`, `cwe_ids: vec![400]`, estimated complexity class in description
- [ ] Register in pipeline as `"algorithmic-complexity"` detector
- [ ] Write tests: nested loops, recursive fib, memoized fib (should NOT fire)
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass
- [ ] Commit

### Task 1.5 — security-detect crew
**Files:** `crates/apex-detect/src/detectors/hash_collision_risk.rs` (new), `crates/apex-detect/src/detectors/mod.rs`, `crates/apex-detect/src/pipeline.rs`
**Summary:** Static detector for hash collision DoS risk

Detect hash table operations with user-controlled keys that could enable collision attacks
(Crosby & Wallach 2003). Flag when:
- HTTP header/query parameter keys are used as hash map keys without randomized hashing
- User input is used as dictionary/map keys in hot paths
- Language default hash implementations known to be vulnerable (Python dict with `__hash__`,
  Java HashMap without randomized seed pre-JDK8)

- [ ] Create `crates/apex-detect/src/detectors/hash_collision_risk.rs`
- [ ] Detect patterns: user input → hash map key (use SecurityPattern-style indicators)
- [ ] Language-specific patterns:
  - Python: `dict[user_input]`, `set.add(user_input)` in request handlers
  - Java: `HashMap.put(untrustedKey, ...)` without `LinkedHashMap` or sorted alternative
  - JS: `object[userInput]` in Express/Koa handlers
- [ ] Emit findings with `cwe_ids: vec![400]`, severity Medium
- [ ] Register in pipeline as `"hash-collision-risk"` detector
- [ ] Write tests with mock HTTP handler code
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass
- [ ] Commit

---

## Wave 2 — Core Engines (depends on Wave 1 types)

These tasks build the runtime engines: resource-guided fuzzing, resource measurement
in sandboxes, and complexity estimation. They depend on Wave 1 types being in place.

### Task 2.1 — exploration crew
**Files:** `crates/apex-fuzz/src/perf_feedback.rs` (new), `crates/apex-fuzz/src/lib.rs`
**Summary:** PerfFuzz feedback mechanism — per-edge execution count maximization

The core of resource-guided fuzzing. Instead of tracking "did we cover a new branch?"
(binary), track "how many times was each edge executed?" (count). An input is interesting
if it causes any edge to execute more times than the current maximum for that edge.

This is the PerfFuzz (ISSTA 2018) approach: multi-dimensional feedback where each edge
is an independent maximization objective.

- [ ] Create `crates/apex-fuzz/src/perf_feedback.rs`
- [ ] Implement `PerfFeedback` struct:
  ```rust
  pub struct PerfFeedback {
      /// Per-edge maximum execution count seen so far
      max_edge_counts: HashMap<u64, u64>,
      /// Total instruction count maximum
      max_total_count: u64,
      /// Peak memory maximum
      max_peak_memory: u64,
  }
  ```
- [ ] Implement `is_interesting(&mut self, metrics: &ResourceMetrics) -> bool`:
  - Return true if any edge count exceeds current max for that edge
  - Return true if total instruction count exceeds max_total_count
  - Return true if peak memory exceeds max_peak_memory
  - Update maximums when returning true
- [ ] Implement `score(&self, metrics: &ResourceMetrics) -> f64`:
  - Sum of (edge_count / max_edge_count) for each edge — higher = closer to known worst case
  - Used for corpus energy assignment
- [ ] Implement `hottest_edge(&self) -> Option<(u64, u64)>` — edge with highest max count
- [ ] Add `pub mod perf_feedback;` to `lib.rs`
- [ ] Add `pub use perf_feedback::PerfFeedback;` to `lib.rs` re-exports
- [ ] Write tests: new edge counts above max → interesting; below max → not interesting
- [ ] Write tests: score increases monotonically with edge counts
- [ ] Run `cargo nextest run -p apex-fuzz` — confirm pass
- [ ] Commit

### Task 2.2 — exploration crew
**Files:** `crates/apex-fuzz/src/perf_strategy.rs` (new), `crates/apex-fuzz/src/corpus.rs`, `crates/apex-fuzz/src/lib.rs`
**Summary:** PerfFuzzStrategy — Strategy impl that maximizes resource consumption

A new `Strategy` implementation that plugs into the existing AgentCluster orchestration
loop. Uses the same mutators and schedulers as FuzzStrategy, but the fitness function
rewards resource consumption instead of coverage.

- [ ] Add `ResourceMax` variant to `PowerSchedule` enum in `corpus.rs`:
  ```rust
  ResourceMax, // energy proportional to resource consumption of the seed
  ```
- [ ] Implement `ResourceMax` energy calculation in `Corpus::sample()`:
  - `energy = duration_ms / max_duration_ms` (normalized resource consumption)
  - Fall back to 1.0 if no duration data
- [ ] Create `crates/apex-fuzz/src/perf_strategy.rs` with `PerfFuzzStrategy`:
  ```rust
  pub struct PerfFuzzStrategy {
      feedback: Mutex<PerfFeedback>,
      corpus: Mutex<Corpus>,
      rng: Mutex<StdRng>,
      scheduler: Mutex<MOptScheduler>,
      /// Which resource to maximize
      objective: PerfObjective,
  }

  pub enum PerfObjective {
      WallTime,
      PeakMemory,
      InstructionCount,
      HottestEdge,
  }
  ```
- [ ] Implement `Strategy` trait for `PerfFuzzStrategy`:
  - `name()` → `"perf-fuzz"`
  - `suggest_inputs()` — same as FuzzStrategy (mutate from corpus), but corpus uses `ResourceMax` schedule
  - `observe()` — check `result.resource_metrics` against `PerfFeedback::is_interesting()`;
    if interesting, add to corpus with energy proportional to resource score
- [ ] Add `seed_corpus()` method matching `FuzzStrategy` API
- [ ] Add `worst_case_input(&self) -> Option<(Vec<u8>, ResourceMetrics)>` — returns the input
  that produced the highest resource consumption
- [ ] Add `pub mod perf_strategy;` and re-export to `lib.rs`
- [ ] Write tests using `ScriptedSandbox` pattern: feed canned `ExecutionResult`s with
  varying `resource_metrics`, verify corpus grows on interesting results
- [ ] Write tests: `worst_case_input()` returns input with highest duration
- [ ] Run `cargo nextest run -p apex-fuzz` — confirm pass
- [ ] Commit

### Task 2.3 — runtime crew
**Files:** `crates/apex-sandbox/src/process.rs`, `crates/apex-sandbox/src/python.rs`, `crates/apex-sandbox/src/javascript.rs`
**Summary:** Add resource measurement to sandbox execution

Populate `ExecutionResult.resource_metrics` from actual process execution. The
measurement approach varies by platform and language.

**Generic (ProcessSandbox):**
- Linux: `getrusage(RUSAGE_CHILDREN)` after `waitpid()` → user time, max RSS
- macOS: same POSIX API, slightly different RSS semantics
- Fallback: `duration_ms` already captured; set only `wall_time_ms`

**Python (PythonTestSandbox):**
- Inject `tracemalloc.start()` / `tracemalloc.get_traced_memory()` into test harness
- Parse peak memory from coverage JSON output or separate metrics file

**JavaScript (JavaScriptTestSandbox):**
- Use `process.memoryUsage()` before/after test
- Use `performance.now()` for high-resolution timing

- [ ] In `ProcessSandbox::run()`, capture `getrusage(RUSAGE_CHILDREN)` after child exits (Linux/macOS)
- [ ] Map rusage fields: `ru_utime` → `cpu_time_ms`, `ru_maxrss` → `peak_memory_bytes`
- [ ] Populate `ExecutionResult.resource_metrics` with captured data
- [ ] In `PythonTestSandbox`: inject tracemalloc snippet, parse peak memory from output
- [ ] In `JavaScriptTestSandbox`: inject `process.memoryUsage()` capture, parse from output
- [ ] Handle platforms where getrusage is unavailable: set `resource_metrics` to wall-time only
- [ ] Write tests: verify ResourceMetrics populated after execution (at minimum wall_time_ms)
- [ ] Run `cargo nextest run -p apex-sandbox` — confirm pass
- [ ] Commit

### Task 2.4 — runtime crew
**Files:** `crates/apex-index/src/regex_extract.rs` (new), `crates/apex-index/src/lib.rs`
**Summary:** Extract regex patterns from source code for ReDoS pipeline

Provide a structured regex extraction API that the ReDoS detector (Task 1.3) can use
for more accurate pattern extraction than ad-hoc regex-on-regex matching.

- [ ] Create `crates/apex-index/src/regex_extract.rs`
- [ ] Define `ExtractedRegex` struct:
  ```rust
  pub struct ExtractedRegex {
      pub pattern: String,
      pub file: PathBuf,
      pub line: u32,
      pub language: Language,
      pub flags: Vec<String>,  // e.g., "i", "m", "g"
  }
  ```
- [ ] Implement `extract_regexes(source: &str, lang: Language, file: &Path) -> Vec<ExtractedRegex>`
- [ ] Language-specific extraction:
  - Python: `re.compile(r"PATTERN")`, `re.match(r"PATTERN", ...)`, etc.
  - JavaScript: `/PATTERN/flags`, `new RegExp("PATTERN")`, `RegExp("PATTERN", "flags")`
  - Rust: `Regex::new(r"PATTERN")`, `Regex::new("PATTERN")`
  - Go: `regexp.Compile("PATTERN")`, `regexp.MustCompile("PATTERN")`
  - Java: `Pattern.compile("PATTERN")`, raw string regex in `matches()`
  - Ruby: `/PATTERN/flags`, `Regexp.new("PATTERN")`
- [ ] Handle escaped quotes and raw strings correctly per language
- [ ] Add `pub mod regex_extract;` to `lib.rs`
- [ ] Write tests for each language's extraction patterns
- [ ] Run `cargo nextest run -p apex-index` — confirm pass
- [ ] Commit

### Task 2.5 — intelligence crew
**Files:** `crates/apex-agent/src/complexity_estimator.rs` (new), `crates/apex-agent/src/lib.rs`
**Summary:** Empirical complexity estimation from execution traces

Implements the Goldsmith et al. (2007) approach: execute a function with systematically
increasing input sizes, measure resource consumption, fit to complexity models using
least-squares regression, report the best-fitting model.

- [ ] Create `crates/apex-agent/src/complexity_estimator.rs`
- [ ] Define `ComplexityEstimator` struct:
  ```rust
  pub struct ComplexityEstimator {
      sandbox: Arc<dyn Sandbox>,
      input_sizes: Vec<usize>,  // Default: [10, 50, 100, 500, 1000, 5000, 10000]
      iterations_per_size: usize, // Default: 5 (for statistical stability)
  }
  ```
- [ ] Implement `async fn estimate(&self, target: &Target, generate_input: impl Fn(usize) -> Vec<u8>) -> Result<ComplexityEstimate>`:
  1. For each input size, generate input, run in sandbox `iterations_per_size` times
  2. Take median duration_ms for each size (robust to outliers)
  3. Fit measurements to models: O(1), O(log n), O(n), O(n log n), O(n²), O(n³), O(2^n)
  4. Select model with highest R² (coefficient of determination)
  5. Return `ComplexityEstimate` with class, confidence (R²), and raw samples
- [ ] Implement curve fitting for each model:
  - O(1): constant fit → variance / mean²
  - O(log n): `t = a * ln(n) + b` → least squares on (ln(n), t)
  - O(n): `t = a * n + b` → least squares on (n, t)
  - O(n log n): `t = a * n * ln(n) + b` → least squares on (n*ln(n), t)
  - O(n²): `t = a * n² + b` → least squares on (n², t)
  - O(n³): `t = a * n³ + b` → least squares on (n³, t)
  - O(2^n): `ln(t) = a * n + b` → least squares on (n, ln(t))
- [ ] Add `pub mod complexity_estimator;` to `lib.rs`
- [ ] Write tests with synthetic timing data: linear data → ComplexityClass::Linear, quadratic data → Quadratic
- [ ] Write tests: confidence > 0.9 for clean data, < 0.5 for noisy random data
- [ ] Run `cargo nextest run -p apex-agent` — confirm pass
- [ ] Commit

### Task 2.6 — intelligence crew
**Files:** `crates/apex-synth/src/perf_prompts.rs` (new), `crates/apex-synth/src/lib.rs`
**Summary:** Performance-aware LLM synthesis prompts

New prompt strategies that instruct the LLM to generate worst-case inputs and performance
regression tests rather than functional coverage tests.

- [ ] Create `crates/apex-synth/src/perf_prompts.rs`
- [ ] Implement `PerfPromptStrategy` that generates prompts like:
  - "Generate an input that maximizes execution time for this function"
  - "This function has O(n²) complexity. Generate a test that demonstrates quadratic scaling"
  - "This regex is vulnerable to catastrophic backtracking. Generate a ReDoS proof-of-concept"
- [ ] Include language-specific timing instrumentation in generated tests:
  - Python: `import time; start = time.perf_counter(); ...; elapsed = time.perf_counter() - start; assert elapsed < THRESHOLD`
  - JavaScript: `const start = performance.now(); ...; const elapsed = performance.now() - start; expect(elapsed).toBeLessThan(THRESHOLD)`
  - Rust: `let start = std::time::Instant::now(); ...; assert!(start.elapsed() < Duration::from_millis(THRESHOLD))`
- [ ] Implement `SloPromptStrategy` for generating SLO verification tests:
  - Takes function name + SLO (e.g., "100ms for 10KB input")
  - Generates test that creates boundary-size input and asserts timing
- [ ] Add `pub mod perf_prompts;` to `lib.rs`
- [ ] Write tests: prompt generation includes timing instrumentation for each language
- [ ] Run `cargo nextest run -p apex-synth` — confirm pass
- [ ] Commit

---

## Wave 3 — Integration (depends on Wave 2 engines)

Wire the engines into the CLI, agent orchestration, and MCP interface.

### Task 3.1 — platform crew
**Files:** `crates/apex-cli/src/perf.rs` (new), `crates/apex-cli/src/lib.rs`
**Summary:** `apex perf` CLI command with complexity, redos, and slo modes

The user-facing command that ties everything together.

```
apex perf [OPTIONS] --target <PATH>

Options:
    --target <PATH>         Target file or directory
    --lang <LANG>           Language (auto-detected if omitted)
    --complexity            Run complexity scaling analysis
    --redos                 Scan for ReDoS vulnerabilities
    --slo <SPEC>            Verify SLO (format: "function:latency:input_size")
    --duration <DURATION>   Fuzzing duration (default: 5m)
    --objective <OBJ>       Optimization objective: time|memory|instructions (default: time)
    --perf-baseline <PATH>  Compare against saved baseline JSON
    --save-baseline <PATH>  Save current measurements as baseline
    --output <PATH>         Output file (JSON)
    --threshold <PCT>       Regression threshold percentage (default: 100, i.e. 2x)
```

- [ ] Create `crates/apex-cli/src/perf.rs` module
- [ ] Define `PerfArgs` struct with clap derive:
  - `target: PathBuf`
  - `lang: Option<LangArg>`
  - `complexity: bool`
  - `redos: bool`
  - `slo: Vec<String>`
  - `duration: Option<String>` (parse to Duration)
  - `objective: Option<PerfObjective>`
  - `perf_baseline: Option<PathBuf>`
  - `save_baseline: Option<PathBuf>`
  - `output: Option<PathBuf>`
  - `threshold: Option<f64>`
- [ ] Add `Perf(PerfArgs)` variant to `Commands` enum in `lib.rs`
- [ ] Add dispatch: `Commands::Perf(args) => perf::run_perf(args, cfg).await`
- [ ] Implement `run_perf()`:
  - If `--redos`: run `ReDoSDetector` on target, print findings
  - If `--complexity`: run `ComplexityEstimator` on target functions, print results
  - If `--slo`: parse SLO specs, generate and run boundary tests
  - Default (no flags): run resource-guided fuzzing with `PerfFuzzStrategy`
  - If `--perf-baseline`: load baseline, compare with `diff_perf()`, report regressions
  - If `--save-baseline`: save current measurements as `PerfBaseline` JSON
- [ ] Format output: table for terminal, JSON for `--output`
- [ ] Implement SLO spec parsing: `"function_name:100ms:10KB"` → (function, latency, input_size)
- [ ] Write integration test: `apex perf --redos --target fixtures/vulnerable_regex.py`
- [ ] Run `cargo nextest run -p apex-cli` — confirm pass
- [ ] Commit

### Task 3.2 — intelligence crew
**Files:** `crates/apex-agent/src/router.rs`, `crates/apex-agent/src/orchestrator.rs`
**Summary:** Integrate PerfFuzzStrategy into agent orchestration

- [ ] Add `PerfFuzz` variant to `StrategyRecommendation` in `router.rs`
- [ ] In `S2FRouter::classify()`, add heuristic: if function has known super-linear complexity
  (from static detector findings), route to `PerfFuzz`
- [ ] In `AgentCluster`, when perf mode is enabled, add `PerfFuzzStrategy` alongside `FuzzStrategy`
- [ ] The bandit scheduler will naturally learn to allocate more iterations to whichever
  strategy (coverage vs perf) is finding more interesting results
- [ ] Write test: `ScriptedSandbox` + `PerfFuzzStrategy` integration
- [ ] Run `cargo nextest run -p apex-agent` — confirm pass
- [ ] Commit

### Task 3.3 — platform crew
**Files:** `crates/apex-cli/src/lib.rs` (report section)
**Summary:** Add "Performance Risk" section to analysis reports

When `apex analyze` or `apex run` completes, if performance detectors found findings,
include a dedicated "Performance Risk" section in the output.

- [ ] In the report formatting code, group findings by `FindingCategory::PerformanceRisk`
- [ ] Format performance findings with:
  - Complexity class (if estimated)
  - Worst-case input (if generated by perf fuzzing)
  - Resource consumption measurements
  - CWE-400/CWE-1333 classification
- [ ] Enable performance detectors (`redos`, `algorithmic-complexity`, `hash-collision-risk`)
  by default in the standard detector set
- [ ] Write test: report includes performance section when performance findings exist
- [ ] Run `cargo nextest run -p apex-cli` — confirm pass
- [ ] Commit

### Task 3.4 — mcp-integration crew
**Files:** `crates/apex-rpc/src/mcp.rs`
**Summary:** MCP tools for performance analysis

Add MCP tool definitions so AI coding assistants can invoke performance analysis.

- [ ] Add `apex_perf` tool: runs `apex perf` with configurable options
- [ ] Add `apex_perf_redos` tool: runs ReDoS scanning on a target
- [ ] Add `apex_perf_complexity` tool: runs complexity analysis on a target function
- [ ] Add `apex_perf_slo` tool: verifies SLO assertions
- [ ] Each tool returns structured JSON matching the CLI output format
- [ ] Write tests for tool parameter validation
- [ ] Run `cargo nextest run -p apex-rpc` — confirm pass
- [ ] Commit

---

## Wave 4 — End-to-End Verification (depends on Wave 3)

### Task 4.1 — platform crew
**Files:** `tests/` (integration tests)
**Summary:** End-to-end integration tests for the full performance pipeline

- [ ] Create test fixture: Python file with known ReDoS regex `(a+)+$`
  - Verify `apex perf --redos` finds it with CWE-1333
  - Verify finding includes concrete worst-case input
- [ ] Create test fixture: Python file with O(n²) nested loop
  - Verify `apex perf --complexity` identifies quadratic complexity
- [ ] Create test fixture: function with known performance SLO
  - Verify `apex perf --slo "func:100ms:1KB"` passes for fast function
  - Verify it fails for intentionally slow function
- [ ] Test baseline workflow:
  - `apex perf --save-baseline base.json`
  - Modify code to be slower
  - `apex perf --perf-baseline base.json` detects regression
- [ ] Test JSON output format consistency
- [ ] Run full integration test suite
- [ ] Commit

### Task 4.2 — all crews
**Files:** `CHANGELOG.md`
**Summary:** Update changelog and documentation

- [ ] Add `[Unreleased]` entry for performance test generation feature
- [ ] Document `apex perf` command and all subcommands
- [ ] Document new detectors: `redos`, `algorithmic-complexity`, `hash-collision-risk`
- [ ] Document new Finding category and evidence type
- [ ] Document MCP tools
- [ ] Commit

---

## Verification

- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo nextest run --workspace` passes (all ~3000+ tests)
- [ ] `apex perf --redos --target test_fixtures/` finds known ReDoS patterns
- [ ] `apex perf --complexity --target test_fixtures/` correctly classifies O(n²) function
- [ ] `apex perf --slo "sort:50ms:1000" --target test_fixtures/` verifies SLO
- [ ] Performance findings appear in `apex analyze` report with CWE-400/CWE-1333
- [ ] JSON output format is valid and parseable
- [ ] MCP tools respond correctly to invocations
