---
id: 01KNZ5SMCPQXVBGR1SZJCT57KV
title: Fuzz4All — Universal Fuzzing with Large Language Models (ICSE 2024)
type: literature
tags: [fuzz4all, llm, fuzzing, icse-2024, universal-fuzzer, autoprompt, test-generation]
links:
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ6GWDH2RR758AACF6V2X3V
    type: related
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:05:05.430199+00:00
modified: 2026-04-11T21:05:05.430205+00:00
source: "https://arxiv.org/abs/2308.04748"
---

# Fuzz4All — Universal Fuzzing with Large Language Models

*Xia, Deng, Zhang, et al., University of Illinois Urbana-Champaign. Published at ICSE 2024 (46th International Conference on Software Engineering). ArXiv:2308.04748. Code at https://github.com/fuzz4all/fuzz4all.*

## One-line summary

Fuzz4All is the first LLM-based fuzzer that works on *multiple input languages*. Instead of writing a hand-crafted grammar or mutator for each language, Fuzz4All uses an LLM as both an input generator and mutator, producing diverse and syntactically valid inputs for any language the LLM has seen in training.

## The key technical contributions

### Autoprompting

LLM fuzzing needs a prompt that elicits productive inputs. Naive prompts produce repetitive, coverage-poor outputs. Fuzz4All's **autoprompting** step uses the LLM itself to iteratively refine the fuzzing prompt based on what kinds of inputs have proven useful. This gives a per-target prompt without human tuning.

### Fuzzing loop

The outer loop alternates between **generation** (ask the LLM for new inputs given the current prompt) and **mutation** (ask the LLM to mutate a previously useful input). This is analogous to the coverage-guided loop in AFL/LibFuzzer but with the LLM replacing the byte-level mutator. Coverage or bug-finding feedback is used to update the prompt, which then drives the next round.

## Evaluation

- **Targets:** 9 systems consuming 6 languages — C, C++, Go, SMT2, Java, Python. Targets include GCC, Clang, Z3, CVC5, OpenJDK, Go compiler, Qiskit (quantum computing).
- **Baselines:** language-specific fuzzers (Csmith for C, YARPGen for C++, GoFuzz, etc.).
- **Results:** higher code coverage than every language-specific baseline across all six languages. 98 previously unknown bugs found during the evaluation, 64 confirmed by developers.

This is genuinely impressive because the language-specific baselines embed years of human engineering effort per language, and Fuzz4All beat them with a single LLM pipeline.

## Why this matters for performance test generation

Fuzz4All is a *compiler/SMT/interpreter* fuzzer — it finds bugs by producing diverse input programs. It is not a performance test generator and not an API test generator. But its architecture is the clearest published evidence that LLM-driven fuzzing loops can *compete with and beat* hand-crafted generators for structured-input problems.

For performance, two implications:

1. **The autoprompt pattern is transferable.** A performance-fuzzing tool (one that finds inputs triggering worst-case latency, à la PerfFuzz or SlowFuzz) could use Fuzz4All-style autoprompting to generate candidate slow inputs without hand-written mutators.
2. **LLMs as universal grammar-aware generators.** Most perf tools hard-code a grammar (OpenAPI → k6 only works for REST, ghz only for gRPC). A Fuzz4All-analog for workload generation could take *any* API description (REST, GraphQL, gRPC, AsyncAPI, SQL) and produce a load test by treating the description as a prompt and iterating.

## Failure modes / limits

1. **LLM-bounded syntactic correctness.** Fuzz4All gets ~90% syntactically valid outputs (varies by language). The remaining 10% are thrown away — which means a lot of LLM API spend produces nothing. Language-specific grammar fuzzers are more expensive per input to write, cheaper per valid input to run.
2. **LLM-bounded novelty.** Once the LLM has been prompted into a region, it gets stuck producing similar variations. Autoprompting helps but is not a cure.
3. **Bug-detection oracle is still the same old crash/sanitiser bug.** Fuzz4All inherits the AFL-style oracle. It does not solve the oracle problem; it just automates the mutator.
4. **LLM training-data coverage.** Fuzz4All works for languages the LLM has seen. For a brand-new DSL with no public examples, the LLM can't generate valid inputs. Most industry DSLs are in this bucket.
5. **Cost.** Fuzz4All runs on paid LLM APIs. The paper reports costs in the low-dollars-per-hour range with GPT-4, but this adds up for long campaigns and is higher than running AFL locally.

## Takeaway

Fuzz4All is the strongest published evidence that LLMs can *replace* hand-written grammar-aware mutators for code-like inputs. It does not directly address performance testing, but it is the cleanest architectural template for an LLM-driven perf-fuzzing tool, and it's the natural starting point for anyone building a universal workload generator.

## Citations

- ArXiv: https://arxiv.org/abs/2308.04748
- ICSE 2024 paper: https://dl.acm.org/doi/abs/10.1145/3597503.3639121
- Project page: https://fuzz4all.github.io/
- Code: https://github.com/fuzz4all/fuzz4all
- LLM fuzzing survey (context): https://arxiv.org/html/2402.00350v3