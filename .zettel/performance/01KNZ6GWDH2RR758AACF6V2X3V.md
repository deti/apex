---
id: 01KNZ6GWDH2RR758AACF6V2X3V
title: "Large Language Models Based Fuzzing Techniques: A Survey (Huang et al., 2024)"
type: literature
tags: [llm, fuzzing, survey, huang, arxiv-2024, test-generation]
links:
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ5SMCPQXVBGR1SZJCT57KV
    type: related
  - target: 01KNZ5SMAD6NJG3EYE06C67A6S
    type: related
  - target: 01KNZ6GWG119PXENNVS99TD8PJ
    type: related
  - target: 01KNZ6QBEK839FPJSX8KWET9TQ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:47.313554+00:00
modified: 2026-04-11T21:17:47.313560+00:00
source: "https://arxiv.org/abs/2402.00350"
---

# Large Language Models Based Fuzzing Techniques: A Survey

*Huang, Zhao, Chen, Ma. ArXiv:2402.00350, February 2024 (v1), revised through v3 in 2024. One of the first dedicated surveys of LLM-driven fuzzing. Relevant to this research because perf test generation sits exactly at the intersection of fuzzing and load testing.*

## Scope of the survey

The paper catalogues how LLMs have been applied to fuzzing across:

- **Software systems.** Compilers, interpreters, parsers, protocol implementations.
- **Input generation.** LLMs as grammar-aware mutators or generators.
- **Seed generation.** Using LLMs to produce initial corpus for traditional fuzzers.
- **Feedback loops.** How LLMs are integrated with coverage-guided or symptom-guided fuzzing.
- **Evaluation and challenges.** What works, what doesn't, what's missing.

The survey is the best single entry point for understanding the 2023–2024 LLM fuzzing landscape.

## Key findings I'll carry forward

1. **LLMs shine for structured inputs.** Programming languages, formal specifications, protocol messages, SQL queries — anything with a rigid grammar. LLMs generate inputs that are syntactically valid at much higher rates than random or grammar-only fuzzers, and they exploit implicit semantic patterns (common identifiers, realistic value ranges) that hand-written generators miss.
2. **LLMs struggle for deeply binary / unstructured inputs.** Raw binary formats, packed data structures, proprietary file headers — LLMs have seen very little of these in training and produce low-validity outputs. Traditional byte-level AFL fuzzing remains the SOTA for these.
3. **Autoprompting and self-evolving prompts are the frontier.** Fuzz4All (see dedicated note) shows that the loop where the LLM itself is asked to improve its prompt can beat hand-written strategies. This pattern generalises.
4. **Cost and latency are real constraints.** LLM API calls are slow (hundreds of ms) and expensive (cents per thousand calls) compared to microsecond AFL executions. Fuzzing campaigns that naively replace every mutation with an LLM call are cost-prohibitive. Hybrid architectures (LLM for seed generation, AFL for iteration) are more practical.
5. **Evaluation is non-standard.** Different papers use different metrics (coverage, bugs found, runtime), making direct comparison hard. The survey calls for standardised benchmarks.

## What this tells us about performance-test generation specifically

Performance test generation is a *relative* of fuzzing: both involve producing inputs to exercise a system. Several survey findings translate directly:

- **Structured-input strength.** OpenAPI specs, GraphQL schemas, protobuf definitions are all structured. LLMs should be good at generating loads from them. Empirically they are (DeepREST, LLM-augmented RESTler).
- **Unstructured binary weakness.** Perf tests of binary protocols (gRPC-over-protobuf without .proto, raw WebSocket, custom message formats) are harder for LLMs.
- **Autoprompting for workload profiles.** The Fuzz4All pattern of self-evolving prompts has a natural analog: evolve the workload profile against a latency-regression objective. No one has published this yet.
- **Cost constraint is even worse for load.** A single LLM call per test case is fine; a single LLM call per request in a 10k RPS load test is unaffordable. Any LLM-driven load generator has to use LLM calls sparingly — for scenario design, not per-request synthesis.

## Gaps the survey identifies

1. **Lack of unified evaluation benchmarks.** No agreed-on "fuzzing olympics" for LLM-driven fuzzers. The survey recommends one.
2. **Poor treatment of language-specific constraints.** LLMs can generate syntactically valid inputs but often miss semantic constraints (type consistency, resource limits).
3. **Limited integration with feedback.** Most LLM fuzzers are one-shot; the coverage-guided feedback loop is only present in a few (Fuzz4All, FuzzGPT).
4. **Scalability concerns.** Very few papers evaluate on real industrial-scale codebases.

## Adversarial reading

1. **The survey is early.** Written in early 2024, it misses several strong 2024/2025 papers (DeepREST, LlamaRestTest, MASTEST, agentic multi-tool variants). It's best read as a snapshot of the 2023 landscape plus the first wave of 2024 papers.
2. **Citation padding.** Like many surveys, it lists works without always ranking their importance. A reader has to do their own weighting.
3. **No direct perf coverage.** The survey is fuzzing-centric. Performance-test generation is mentioned only glancingly. The connections to perf have to be inferred.
4. **Cost arguments are dated.** API costs have dropped significantly since early 2024 (Claude Haiku, GPT-4o-mini, Llama 3.1 locally). The cost-per-call argument is weaker now than when the survey was written.

## Why it matters

For anyone writing new LLM-driven tooling for test generation, this survey is the compulsory starting point for understanding what's been tried, what worked, what didn't. The main value is the taxonomy — knowing whether a new idea fits into an existing bucket or occupies novel space.

## Citations

- https://arxiv.org/abs/2402.00350
- https://arxiv.org/html/2402.00350v3 (most current version)
- Complementary: Large Language Models for Software Testing (roadmap): https://arxiv.org/html/2509.25043v1
- Fuzz4All (cited in the survey): https://arxiv.org/abs/2308.04748