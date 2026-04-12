---
id: 01KNZ6QBKG64F2WEP3PNJK61JH
title: Top Research Gaps in Performance Test Generation (Synthesis)
type: permanent
tags: [research-gaps, synthesis, toolmaker, performance-testing, test-generation, concept, roadmap]
links:
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: references
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: references
  - target: 01KNZ6QBH0YZYKPZNYDCZD5P2B
    type: references
  - target: 01KNZ4VB6JNFK2XFWP9N1HEJ5M
    type: references
  - target: 01KNZ4VB6JSR9RJ0RTWXB9P6FV
    type: references
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: references
  - target: 01KNZ5ZPSEK679QQYMHXF16WFF
    type: references
  - target: 01KNZ5F5C3YS1EYDCVFQ7TQS9H
    type: references
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: references
  - target: 01KNZ56MRW2B1XSH2X5K5AEJ33
    type: references
  - target: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
    type: references
  - target: 01KNZ68KJMZSSAZVFAB3ZNXNTJ
    type: references
  - target: 01KNZ72G2VNNP6JHWQAK0HJTXM
    type: references
  - target: 01KNZ72G5955YGB9B2W61QD2Z4
    type: references
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: references
  - target: 01KNZ4VB6JQZHJVB2EQK6HVXE0
    type: references
  - target: 01KNZ67FD9WNQDTFVMEXQ0PRRV
    type: references
  - target: 01KNZ67FDMCEA0MKZ8GZ841NDT
    type: references
  - target: 01KNZ67FDY7FH782T6V34Y2CFT
    type: references
  - target: 01KNZ67FCZ2BPSAM6QEAQ0RX2P
    type: references
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: references
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: references
  - target: 01KNZ6T759YNNAFPCMPAGSTCYV
    type: references
  - target: 01KNZ6T74XZWE3RYQ86DZ2WREJ
    type: references
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: references
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: references
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: references
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: references
  - target: 01KNZ5F557YM1X8Q2ZBXZEBXRM
    type: references
  - target: 01KNZ6WJ0CFDHHQSA1951PSSFF
    type: references
  - target: 01KNZ5ZPYD6VWX13H5G57D0TCH
    type: references
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: references
  - target: 01KNZ6GWDH2RR758AACF6V2X3V
    type: references
  - target: 01KNZ68KDQCP6HGTQEBGQW26VC
    type: references
  - target: 01KNZ5SMF1GAFA93P6D8TQFM4Z
    type: references
  - target: 01KNZ5SMHEAYVDMJWJ9NAZBPCD
    type: references
  - target: 01KNZ6GWG119PXENNVS99TD8PJ
    type: references
  - target: 01KNZ6QBEK839FPJSX8KWET9TQ
    type: references
  - target: 01KNZ55NZFXHMGS5NN0TN020MR
    type: references
  - target: 01KNZ5SMAD6NJG3EYE06C67A6S
    type: references
  - target: 01KNZ55P1P0TZTWKT0K9YCACJN
    type: references
  - target: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
    type: references
  - target: 01KNZ68KN59XANY9TX9WE0BYJH
    type: references
  - target: 01KNZ55PB02X6BJN52R032R18K
    type: references
  - target: 01KNZ68KB81AGB3MWRNS87EKXK
    type: references
  - target: 01KNZ68K8H5MWD58PP5KVHD6QF
    type: references
  - target: 01KNZ68KG6N588XFB9A13H7RQZ
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
  - target: 01KNZ6GW6232G68HHVGR9ANYZM
    type: related
created: 2026-04-11T21:21:19.472422+00:00
modified: 2026-04-11T21:21:19.472427+00:00
---

# Top Research Gaps in Performance Test Generation — Ranked Synthesis

*A consolidated hub note that ranks the most promising research/tooling opportunities identified across the notes in this vault's performance test generation lane. Written to be actionable — each gap has a plausible tool architecture and a rough feasibility assessment.*

## Methodology for this ranking

Ranking axes used:

- **Leverage.** How much improvement does solving this gap produce across the whole pipeline? (1 = minor; 5 = transformative.)
- **Feasibility.** Is the required technology available and the engineering tractable? (1 = speculative; 5 = 2-engineer-month project.)
- **Adoption barrier.** How hard is it for a user to actually use the solution? (1 = high friction; 5 = drop-in.)

Scores are my judgement based on reading the full vault. Interpret them as rough guidance, not measurement.

## Gap 1 — Session-mining → workload-model → load-test pipeline

**Leverage: 5 · Feasibility: 4 · Adoption: 3**

### The gap

No open-source tool today takes production access logs or distributed traces and produces a runnable load test with a realistic CBMG-style workload model. The *methodology* has been known since Menascé's 2001 book; the ingredient technologies (pandas, scikit-learn, networkx, k6) have all existed for a decade; nobody has put them together.

### What the tool would do

1. Read access logs or Jaeger/Tempo traces for a time window.
2. Cluster sessions into user types (HDBSCAN, k-means, whatever works).
3. Fit per-cluster CBMGs (Markov chain of endpoint transitions + think-time distribution).
4. Fit per-cluster arrival rates with time-of-day seasonality.
5. Render the combined model as a k6/Gatling/Locust script with per-cluster scenarios.
6. Optionally: drift detection against subsequent log windows, PR-based test regeneration.

### Why it hasn't been built

- It's a data-science project, not a test-tool project. Teams that build load tools don't usually have a data-scientist on staff. Teams that do data science don't build test tools.
- Every part of the pipeline needs per-team customisation (canonical URL patterns, session-boundary heuristics, PII scrubbing).
- The incremental value story is hard: the engineer has to trust the tool's model more than their own intuition.

### Estimated cost

Four-to-eight engineer-weeks for a functional v1. Two-to-three engineer-months for a robust tool with test-renderer plugins for k6 and Gatling. No fundamental research needed — it's all integration.

## Gap 2 — SLO-aware statistical oracle library

**Leverage: 5 · Feasibility: 4 · Adoption: 4**

### The gap

Every macrobenchmark tool (k6, Gatling, JMeter, Artillery) lets you assert thresholds but none of them does proper statistical testing on the results. There is no open-source library that takes two benchmark runs and says "these are statistically different at 95% confidence, here are the endpoints where the regression is real." Criterion.rs does this at the microbenchmark level; nothing does it at the macrobenchmark level.

Separately, no tool ingests a formal SLO document (OpenSLO, Nobl9, or just a YAML) and automatically synthesises the corresponding check/threshold/assertion block.

### What the tool would do

1. Accept two benchmark result sets (JSON/InfluxDB).
2. Run non-parametric tests (Wilcoxon, Mann-Whitney, bootstrap CIs) per endpoint per metric.
3. Accept an SLO spec and automatically configure thresholds for all covered endpoints.
4. Emit a regression report listing endpoints where the change is statistically significant.
5. Plug into k6/Gatling/JMeter as a post-processing step.

### Feasibility

High. The statistics are well-known. The tooling is building a small Python or Rust package and integrating with existing load-test output formats.

### Adoption barrier

Low. A drop-in "improve your regression detection" library has broad appeal.

## Gap 3 — Envoy tap / replay → rate-controlled synthetic workload

**Leverage: 4 · Feasibility: 3 · Adoption: 3**

### The gap

Envoy's tap filter and request-mirroring give clean, low-overhead capture of real traffic inside a service mesh. GoReplay does the same at the L7 level. None of them delivers the captured stream into a form that can be fed to a load generator with arrival-rate control, replay-divergence mitigation, and PII scrubbing as first-class features.

### What the tool would do

1. Read a capture from Envoy tap (gRPC sink), GoReplay, or a raw HTTP request log.
2. Apply a canonical intermediate representation (IR): per-request structured data, not raw bytes.
3. Apply PII scrubbing rules from a config file.
4. Apply state-management rules (token refresh, idempotency-key regeneration).
5. Emit a k6 scenario that replays at a configurable rate, open-loop or closed-loop.
6. Optionally: amplify by sampling distinct resource IDs rather than duplicating.

### What's hard

Replay divergence is inherent (see the dedicated concept note in this vault). The tool would mitigate it, not solve it. Users have to configure per-service state-management rules, which is the sticky part.

## Gap 4 — LLM-driven workload profile synthesis from access logs

**Leverage: 4 · Feasibility: 4 · Adoption: 5**

### The gap

Session mining from access logs is a data-science workflow that most teams won't do. An LLM can read a summarised log-stats file and propose a workload profile in natural language or as a YAML spec. That workflow is accessible to anyone who can write a prompt.

### What the tool would do

1. Pre-aggregate access logs into a compact stats file (endpoint rates, session-length percentiles, top user-agents).
2. Feed the stats file to an LLM with a system prompt that explains workload modelling.
3. LLM proposes a workload spec: cluster types, per-cluster arrival rates, per-cluster endpoint mix, think-time distributions.
4. Render the spec as k6/Gatling scenarios.
5. Optionally: explain the model to the engineer in natural language.

### Why this beats Gap 1 on adoption

Gap 1 requires data-science engineering. Gap 4 is a prompt template + a renderer. Ten times easier to ship, and the quality floor is set by the LLM's ability to read statistics (which is surprisingly good). The ceiling is lower — an LLM cannot do formal fit validation — but for most teams "plausible workload spec from the LLM" is strictly better than "RPS number from gut feel."

### Caveats

The LLM will confidently hallucinate wrong workload profiles for unusual services. A validation step that compares the proposed spec against the raw logs and flags inconsistencies is mandatory.

## Gap 5 — RESTler/DeepREST-style sequence generation with a *latency* reward

**Leverage: 4 · Feasibility: 3 · Adoption: 2**

### The gap

RESTler and DeepREST discover call sequences for correctness testing. Their underlying engines (producer-consumer inference, deep RL with curiosity reward) could be retargeted with a reward function based on latency variance or throughput collapse. That would be a **performance fuzzer at the session level** — finding sequences of API calls that cause the system to slow down.

No one has published this. The conceptual bridge is obvious; the engineering is non-trivial.

### What the tool would do

1. Parse an OpenAPI spec and infer producer-consumer dependencies (RESTler style) or train a curiosity-driven RL agent (DeepREST style).
2. Use a reward function that penalises paths with low latency variance and rewards paths that produce high-percentile outliers.
3. Output the top-K worst-case sequences as a test suite.
4. Optionally: use an LLM to explain *why* each sequence is slow (e.g., "this sequence creates 1000 orders then queries them all in one paginated GET").

### Why adoption is hard

It's a research tool before it's a product. The practical payoff is uncertain — the sequences found may be unusual enough that they don't occur in real traffic. Needs joint optimisation with realistic-workload constraints.

## Honorable mentions (not in top 5 but worth flagging)

- **Workload drift detector.** Compare current test suite to current production, emit drift scores. Cheap, useful.
- **LlamaRestTest-style local model for continuous perf test generation.** Cost-efficient path for high-volume use.
- **AsyncAPI → Kafka load generator.** The event-driven gap is wide open; no "k6 for Kafka" exists.
- **Pg_stat_statements → targeted load test generator.** Specific but high-leverage for database-heavy apps.
- **Sqlcommenter-driven query-to-endpoint attribution.** Infrastructure piece that unlocks the above.
- **SchemaThesis → k6 replay bridge.** Capture the property-based-generated requests at a controlled rate.
- **Trace-driven load generation (Tempo/Jaeger → k6 scenarios).** A Grafana-native tool nobody has built yet.
- **LLM-generated SLO documents from PRDs.** Upstream the hard part of oracle authoring.
- **Agentic perf-test pipeline** — multi-agent design with drift detector, oracle agent, script author, result explainer. Derivative of MASTEST/LogiAgent but targeted at perf.

## Anti-patterns to avoid

From reading the landscape, a few traps a new tool should sidestep:

1. **Building yet another recorder.** The JMeter/Gatling/k6 Studio recorder space is saturated. New work should assume the capture step is solved.
2. **Promising "LLM writes the test."** As the LLM frontier note documents, LLMs are bad at workload modelling. A tool that sells "AI writes your load test" will under-deliver. Better framing: "AI helps your engineer write a better load test."
3. **Ignoring the oracle problem.** A test without an oracle is useless. Any new tool needs a credible regression-detection story from day one.
4. **Closed-loop by default without explaining.** Inheriting k6's closed-loop default without educating the user perpetuates the most common perf-testing error.
5. **Specifying RPS as the primary interface.** RPS hides everything interesting. Tools should lead with workload specs, not rate specs.

## Citations

- This synthesis draws from every note in the test-generation lane of this vault. Specific claims are cited in those notes.