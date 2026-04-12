---
id: 01KNZ4VB6JKC337NWTGFZRA8GF
title: "Test Design, Execution, Analysis — The Performance Test Workflow Loop"
type: concept
tags: [workflow, test-design, execution, analysis, meier-2007, sdlc]
links:
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: extends
  - target: 01KNZ4VB6JHJSARKD8E9XVGVRC
    type: related
  - target: 01KNZ4VB6JQZHJVB2EQK6HVXE0
    type: related
  - target: 01KNZ4VB6J22PTMXAYQ3V2WYAZ
    type: related
  - target: 01KNZ4VB6JK3TC0S5YZWHNNDEV
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Meier et al. 2007 Ch. 4 'Core Activities' and Ch. 12-16; Barber 'Beyond Performance Testing' series"
---

# Test Design, Execution, Analysis — The Performance Test Workflow Loop

## The activity loop

Meier et al. 2007 (Ch. 4) structure a performance test as a **repeating loop of seven core activities**:

1. Identify test environment.
2. Identify performance acceptance criteria (SLOs).
3. Plan and design tests.
4. Configure test environment.
5. Implement test design.
6. Execute tests.
7. Analyze results, report, retest.

The loop runs once per cycle; the output of step 7 feeds step 3 or step 2 of the next cycle. Each activity has explicit inputs, outputs, and entry/exit criteria. This note covers activities 3, 5, 6, and 7 — the core design/execute/analyze triangle — that dominate the per-cycle workload.

## Test design (activity 3)

Inputs: workload characterisation (separate note `01KNZ4VB6JHJSARKD8E9XVGVRC`), SLOs (`01KNZ4VB6JQZHJVB2EQK6HVXE0`), environment parity analysis (`01KNZ4VB6J22PTMXAYQ3V2WYAZ`).

Outputs: test scripts (scenarios), test data generators, the workload model (open/closed/partly-open), load profile (ramp, steady, ramp-down), pass/fail criteria, and explicit exclusions.

### Design choices

- **Workload model**: open, closed, or partly-open (per Schroeder et al., `01KNZ4VB6JX0CQ5RFAZDJTQMCS`). Picked based on session length from workload characterisation.
- **Arrival distribution**: Poisson, constant-rate, self-similar, or trace-replay. Picked based on observed production traffic shape.
- **Endpoint mix**: relative frequencies from production logs.
- **Session scripts**: the sequence of requests a virtual user (or synthetic session) performs. Data-driven where possible.
- **Test data**: seeding the environment with realistic data; fresh input generation per run to avoid cache warming across runs.
- **Load profile**: the shape of offered load over time. Minimum phases: warm-up, ramp-up, steady-state, ramp-down.
- **Steady-state window**: explicit start and end timestamps, excluded ramp-up, included measurement.
- **Pass/fail criteria**: the SLO translated into a numeric comparison.
- **Reporting artifacts**: which histograms, which time series, which plots will be produced automatically.

### Design anti-patterns

- Starting from a tool's default and tweaking until "it runs" — driven by tooling rather than by requirements.
- Designing from memory rather than from the workload characterisation.
- No explicit steady-state window → no way to isolate "what we're measuring" from "what happened during warmup".
- Pass/fail criteria as "looks reasonable" rather than as a numeric SLO.

## Test execution (activity 6)

Inputs: test design, configured environment, calibrated generator.

Outputs: raw sample data (ideally histograms, not scalars), logs, resource-utilisation time series, metadata about the run.

### What a correct execution looks like

1. **Start from a known state.** Restart the SUT or drop caches to reach a canonical starting point, unless the test specifically targets warm state.
2. **Begin warmup.** Offer load at below-target for a fixed duration, record but discard samples. Let JIT, caches, pools, and GC reach steady state.
3. **Transition to steady state.** Ramp to target load quickly (linear ramp over < 1 minute, typically). Mark the start of the steady-state window.
4. **Sustain target load.** Offer load for the full steady-state duration. Capture all samples via a lossless histogram.
5. **Collect multi-dimensional metrics.** Latency histograms split by endpoint, success/error; throughput per endpoint; resource utilisation per host; application-level gauges (queue depths, connection counts).
6. **Ramp down gracefully.** Stop offering load over ~10 s. Capture the tail-drain period for recovery analysis.
7. **Shut down and collect final artifacts.** Save per-host metrics, GC logs, system logs, flame graphs.

### Execution anti-patterns

1. **No warm-up.** Steady-state measurement includes cold transients.
2. **Measuring from t = 0.** Ramp is averaged into steady-state.
3. **Single-iteration execution.** No repetition; no confidence interval; one run is the result.
4. **Mixing generator and SUT on the same host.** Generator contends with SUT for CPU; measurements are garbage.
5. **No record of what was running when.** Multiple tests in the same window, overlapping, results interleaved.
6. **Discarding raw samples, keeping only summaries.** Cannot re-compute different percentiles after the fact; cannot compose histograms.
7. **Aborting on first error.** A run that errors early gives no information; it should be logged and continue (unless errors invalidate the test).

## Analysis and reporting (activity 7)

Inputs: raw run data, historical baselines, test design.

Outputs: pass/fail decision, executive summary, detailed report, regression analysis, recommended actions.

### Minimum viable analysis

1. **Verify the run was valid**: warm-up reached steady state, no early termination, target load was actually offered, no generator saturation.
2. **Little's-Law sanity check**: reported throughput × response time ≈ average concurrency. If not, something is miscounted.
3. **Apply SLO oracle**: percentile(s) of interest vs threshold; pass or fail.
4. **Compare against baseline**: previous run's numbers; delta; statistical significance (CI overlap, bootstrap).
5. **Investigate anomalies**: any per-endpoint outlier, any ramp-up that hadn't completed, any error spike.
6. **Summarise in a one-page report** with: config, top-line numbers, pass/fail, deltas, anomalies, next steps.

### Advanced analysis

- **Histogram diff**: KS test or Earth Mover's distance between this run and the baseline histogram. Detects distributional change that percentile-scalar comparison misses.
- **Flame graph diff**: before-and-after profiles; hot differences identified automatically.
- **Per-percentile regression**: a regression at p99 with no change at p50 tells you something specific about the tail.
- **Resource-utilisation trending**: did CPU headroom go from 40 % to 20 % across releases? A leading indicator of future SLO breaches.

### Report content

A useful performance test report contains:

- **Metadata**: date, SUT version/SHA, environment, operator, test design ID.
- **Workload summary**: mix, rate, duration, model.
- **Top-line metrics**: p50 / p90 / p99 latency per endpoint; throughput; errors; resource utilisation summary.
- **Pass/fail against SLO**: explicit, numeric.
- **Delta vs baseline**: per metric, with CI.
- **Histograms and time series**: not just scalars.
- **Anomalies and observations**: free text.
- **Recommendations**: what to investigate, what to fix, whether to ship.

### Anti-patterns in analysis

- **Only looking at the pass/fail bit** and skipping the full report when it passes. Silent drift accumulates.
- **Comparing means when the oracle is a percentile.**
- **No baseline**: "100 ms is fast, right?" — with no context, no decision.
- **Comparing against baselines from a different hardware generation**. The baseline must be re-established on the current hardware.
- **Report without recommendations**: the operator spent 2 hours running a test and the report has no next steps.

## The retest step

Any failure, any surprising finding, or any environment change triggers a retest:

- Failure: fix, rerun, verify recovery.
- Surprise: drill down (see isolation testing note), form hypothesis, design a narrower test.
- Environment change: re-baseline, re-calibrate, re-run baseline tests to confirm nothing regressed.

Retest is an activity of its own, not "just run again". Each retest has its own design, execution, and analysis, and its own archived report.

## The cost of getting the workflow right

A well-executed performance test cycle is expensive. A careful test takes 1–3 days of engineering time, 1–4 hours of compute time per run, dedicated infrastructure, and tools/scripts that must be maintained. Most orgs short-cut most of the steps, getting results that are cheaper and much less reliable. The cheap test finds the 2x regressions; the careful test finds the 5 % drift. Both have their place; only the careful test catches the drift before it accumulates into a 40 % degradation over two years.

## Relevance to APEX

APEX's G-46 spec focuses on generating *input* corner cases and *single-function* profiles, not on running full-workflow performance tests. If APEX adds load-testing capabilities, the Meier workflow is the reference structure. The test-design activity is where APEX could automate the most (synthesise workload, derive session model from call graph, pick arrival distribution from observed production shape); execution and analysis would be at least partly delegated to standard tools (k6, wrk2, HdrHistogram, Prometheus).

## References

- Meier et al. — Microsoft p&p Performance Testing Guidance, Ch. 4 "Core Activities", Ch. 12–16.
- Barber, S. — *Beyond Performance Testing: A Series*, PerfTestPlus, 2004–2007.
- Humble, J., Farley, D. — *Continuous Delivery*, Addison-Wesley 2010 — the automation-first framing of the Meier workflow.
