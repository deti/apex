---
id: 01KNZ5SM642DR52PJ1CDNEZ101
title: LLM-Driven Performance Test Generation — Capability Boundaries (2023–2025 Frontier)
type: permanent
tags: [llm, gpt-4, test-generation, performance-testing, frontier, prompt-engineering, oracle-problem, concept]
links:
  - target: 01KNZ5SMAD6NJG3EYE06C67A6S
    type: references
  - target: 01KNZ5SMCPQXVBGR1SZJCT57KV
    type: references
  - target: 01KNZ5SMF1GAFA93P6D8TQFM4Z
    type: references
  - target: 01KNZ5SMHEAYVDMJWJ9NAZBPCD
    type: references
  - target: 01KNZ5SMKSGB6GP479FNDRP1H3
    type: references
  - target: 01KNZ6GWG119PXENNVS99TD8PJ
    type: references
  - target: 01KNZ6QBEK839FPJSX8KWET9TQ
    type: references
  - target: 01KNZ6GWDH2RR758AACF6V2X3V
    type: references
  - target: 01KNZ68KDQCP6HGTQEBGQW26VC
    type: references
  - target: 01KNZ68KJMZSSAZVFAB3ZNXNTJ
    type: related
  - target: 01KNZ72G2VNNP6JHWQAK0HJTXM
    type: related
  - target: 01KNZ68KN59XANY9TX9WE0BYJH
    type: related
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: related
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ4VB6JDWZF3NFVSD5ATJV8
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:05:05.220883+00:00
modified: 2026-04-11T21:05:05.220889+00:00
---

# LLM-Driven Performance Test Generation — What Works, What Doesn't, Where the Frontier Is

*A hub note for the LLM lane. Survey-style, grounded in specific papers and observed behaviours; individual papers get their own notes.*

## The current landscape (as of early 2024)

LLM-assisted test generation research has exploded since GPT-4 (March 2023). A systematic literature review by Pizzorno et al. (2024) identified 115+ publications on LLM-based test generation between May 2021 and late 2024, with a clear inflection point in 2023. The overwhelming majority target **unit-level correctness tests** (JUnit, pytest), not *performance* tests. There is a small but growing set of papers on **REST API testing** and an even smaller set touching **performance directly**.

No peer-reviewed paper as of Q1 2024 proposes an end-to-end LLM pipeline that generates production-grade *performance* test scripts with correct workload profiles. The closest we have is industry tooling (Grafana k6 Studio autocorrelation, NeoLoad MCP, Tricentis agentic workflows) that uses LLMs for narrower sub-tasks.

## What LLMs demonstrably do well

From a mix of peer-reviewed results and hands-on experience reported in industry blog posts:

1. **Translating a natural-language description into a k6/JMeter/Gatling skeleton.** A prompt like "write a k6 script that hits /checkout at 100 RPS with a random UUID as order ID" produces a syntactically correct runnable script almost every time with GPT-4-class models. This is the "boilerplate k6 from OpenAPI+prose" capability.
2. **Translating an OpenAPI spec into a test scenario scaffold.** Comparable to openapi-generator's k6 target but with better handling of edge cases like oneOf/anyOf schemas, because the LLM can reason about semantics. Papers in the "LLM+RESTler" space report better parameter value generation than rule-based tools.
3. **Extracting realistic-looking parameter values.** DeepREST (ASE 2024) and the MDPI 2024 "REST API Fuzzing Using API Dependencies and LLMs" both use LLMs to generate parameter values given endpoint names and descriptions. They report markedly better-than-random values for path segments, enum-ish strings, and date/time fields.
4. **Self-repair of broken scripts.** Given an error message and the failing test code, GPT-4 can propose a fix — this is the basis of self-healing test systems (see note on self-healing).
5. **Correlation rule inference.** k6 Studio's "autocorrelation" feature uses an LLM to find dynamic values in a HAR recording and generate extractor rules. Narrow but useful.
6. **Ad-hoc query generation for exploratory perf investigations.** Prompt: "my p99 spiked at 14:00 UTC on endpoint X, here's a trace, what could cause this?" produces plausible hypotheses that aren't always right but usually include the right answer in top-3.

## What LLMs demonstrably do badly

This is the interesting part. The same models that nail boilerplate fail at the distributional / statistical work that defines a good perf test:

1. **Picking a realistic arrival process.** Asked to write a k6 script "that models realistic user load," an LLM will typically emit a closed workload (N VUs with `sleep(1)` think time). It almost never picks `ramping-arrival-rate` scenarios unless explicitly prompted. It almost never distinguishes open vs. closed workloads (the distinction from Schroeder et al.'s NSDI 2006 paper). This is a *conceptual gap*, not a syntax gap.
2. **Think-time distributions.** Ask an LLM for realistic think time between user actions and you get `sleep(1)` (constant) or `sleep(Math.random() * 3)` (uniform). Real user think-time distributions are log-normal or Pareto. The model has seen the correct answer in training data but defaults to the easy wrong answer.
3. **Percentile-aware oracles.** Asked to add assertions, LLMs gravitate to "status is 200" or "response time < 500ms" — mean-based thresholds. p95/p99 thresholds pulled from an SLO document are rare in the output unless the prompt explicitly specifies them. SLO-aware perf testing is not in the LLM's default playbook.
4. **Stateful session modelling.** LLMs handle single-request scripts well. Multi-step flows with correlation, retries, and per-VU state fall apart at about 5 steps. The context limit is not the problem — reasoning about cross-request state is.
5. **Workload distribution from log data.** Given a sample of access logs and asked "what is the arrival-rate distribution?" an LLM produces words, not a Poisson fit. It cannot actually run statistics; it can describe them.
6. **Concurrency bugs and races in the test script itself.** LLMs write k6 scripts that have subtle VU-state-sharing bugs that only fire at high VU counts, because the model has not seen enough load-test failure modes to recognise them.
7. **Dimensional analysis.** Asked to generate a test that holds 10,000 concurrent users on a service running on a single 2-core machine, the LLM writes the test without flagging that it's impossible. No sanity-check on the physical limits.
8. **Think time and arrival rate interaction (Little's Law).** Asked to generate a test with "1000 users at 50 RPS each" an LLM may or may not realise this implies 50000 RPS total and 20 ms mean think time — the arithmetic is trivial but the model rarely does it spontaneously and cannot validate feasibility.

## The gap pattern

LLMs have absorbed *syntactic* knowledge of perf testing (k6 APIs, JMeter elements, Gatling DSL) and *shallow semantic* knowledge (what a VU is, what a scenario is). They have not absorbed the *statistical* reasoning that distinguishes a good load test from a bad one — because that reasoning rarely appears verbatim in their training data. It exists in textbooks (Menascé's capacity planning books, Jain's Art of Computer Systems Performance Analysis) but not in the kind of corpus LLMs are heavily trained on.

This is a **specific-corpus gap**, not a fundamental capability gap. Fine-tuning on performance-testing-specific data could close it, but nobody has done this publicly.

## Self-healing tests — the most mature sub-application

The one sub-area where LLMs are production-deployed is self-healing: when a test breaks because an API changed, the LLM can propose a patch. Commercial products: testRigor, refluent, Cadmus Group. Robot Framework has an open-source self-healing plugin (MarketSquare/robotframework-selfhealing-agents). A DOM-accessibility-tree zero-cost alternative (no LLM calls) has been proposed (arXiv:2603.20358) specifically because LLM-based self-healing has a non-trivial economic constraint: at 300 tests/day running daily, one production system estimated $1,350–$2,160/month in API token costs.

## NeoLoad MCP — the commercial frontier

Tricentis's NeoLoad ships a Model Context Protocol (MCP) server that lets Claude or other MCP clients issue natural-language commands like "run checkout test at 2000 concurrent users" or "summarise the latest test run." This is not test *generation* — it is test *orchestration* — but it's the only commercial offering that makes LLM-driven perf testing a first-class feature as of 2024. Worth noting because MCP itself is a new protocol (late 2024) that opens the door to agentic workflows in perf tooling.

## What's genuinely novel in the 2024 papers

- **DeepREST (ASE 2024):** combines deep reinforcement learning with curiosity-driven exploration of REST APIs. Not an LLM paper per se, but it's the state of the art that LLM approaches have to beat.
- **MASTEST (2024 preprint):** multi-agent LLM system for REST API testing. Each agent has a role (planner, executor, oracle). Demonstrates that agentic patterns can improve on single-prompt generation but have large cost and reliability overheads.
- **LlamaRestTest (2025):** uses a small fine-tuned language model (Llama-family) rather than GPT-4, showing that sufficiently fine-tuned small models can match GPT-4 on narrow REST testing tasks at much lower cost. Relevant for the economics argument above.
- **Agentic LLMs for REST API Test Amplification (2025):** comparative study of agentic LLM approaches for *amplifying* an existing test suite with additional cases. Amplification is arguably easier than greenfield generation and may be the niche where LLMs are durably useful.
- **Fuzz4All (ICSE 2024):** not API testing, but the closest thing to a proof-of-concept that LLMs can drive a *universal* fuzzing loop. Relevant because perf fuzzing (finding inputs that trigger worst-case latency) is conceptually an instance of the same pattern.

## Toolmaker gaps — the top candidates for research

1. **LLM + Markov workload model.** Prompt the LLM with a sample of access logs and ask it to *propose a workload model* (arrival rate, top endpoints, think-time distribution) that the engineer can then render into a k6 script. The LLM does the "make sense of a corpus" step that current tools don't.
2. **LLM-guided correlation inference from HAR.** Beyond k6 Studio's autocorrelation — use the LLM to explain *why* a correlation exists, which helps the engineer understand whether it's durable.
3. **LLM as performance oracle consultant.** Given an SLO document and a test result, the LLM suggests whether the test validates the SLO and what's missing. Narrow, useful, implementable today.
4. **LLM-generated negative workload tests.** Given a service and its traffic profile, ask the LLM "what pathological input would cause this service to fall over?" — essentially combining a ReDoS/complexity literature prior with the service's actual API. Fuzz4All-style.
5. **LLM-driven self-healing specifically for *workload model* drift.** When production traffic distribution changes (new endpoint popular, new payload shape), the LLM updates the load test. Most self-healing work today targets *test code* drift, not *workload* drift, and these are different problems.

## Citations

- DeepREST paper: https://arxiv.org/abs/2408.08594
- Fuzz4All (ICSE 2024): https://arxiv.org/abs/2308.04748 and https://fuzz4all.github.io/
- LLM-based fuzzing survey: https://arxiv.org/html/2402.00350v3
- REST API fuzzing + LLMs (MDPI 2024): https://www.mdpi.com/2673-4591/120/1/42
- MASTEST multi-agent paper: https://arxiv.org/html/2511.18038v1
- LlamaRestTest: https://arxiv.org/html/2501.08598v2
- Agentic test amplification: https://arxiv.org/html/2510.27417
- RESTifAI: https://arxiv.org/html/2512.08706
- NeoLoad MCP launch blog: https://www.tricentis.com/blog/neoload-mcp-ai-performance-testing
- k6 Studio autocorrelation: https://grafana.com/docs/k6-studio/
- Zero-cost DOM accessibility alternative: https://arxiv.org/pdf/2603.20358
- LLM + load testing feature overview: https://medium.com/@sginsbourg/performance-and-load-testing-with-jmeter-or-6k-in-the-ai-ml-era-cc4015fdd755
- Schroeder et al., open vs closed workload: https://www.usenix.org/legacy/events/nsdi06/tech/schroeder/schroeder.pdf