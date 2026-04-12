---
id: 01KNZ5SMAD6NJG3EYE06C67A6S
title: DeepREST — Deep RL for REST API Test Generation (ASE 2024)
type: literature
tags: [deeprest, reinforcement-learning, rest-api, test-generation, ase-2024, llm, deep-learning]
links:
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ55NZFXHMGS5NN0TN020MR
    type: related
  - target: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
    type: related
  - target: 01KNZ55P1P0TZTWKT0K9YCACJN
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:05:05.357054+00:00
modified: 2026-04-11T21:05:05.357060+00:00
source: "https://arxiv.org/abs/2408.08594"
---

# DeepREST: Automated Test Case Generation for REST APIs Exploiting Deep Reinforcement Learning

*Corradini, Stallenberg, Pastore, Scoccia, et al., presented at ASE 2024 (39th IEEE/ACM International Conference on Automated Software Engineering). ArXiv:2408.08594.*

## One-line summary

DeepREST uses deep reinforcement learning with curiosity-driven exploration to uncover **implicit API constraints** that the OpenAPI spec does not document — constraints on call ordering, on value ranges, on parameter interactions — and uses an LLM to propose realistic input values.

## The problem DeepREST targets

Prior spec-driven REST testing (RESTler, EvoMaster, Schemathesis) relies on what the OpenAPI spec says. Those tools miss:

- **Implicit ordering dependencies.** Operation B can only succeed if operation A has been called first, even though the spec's OpenAPI `links` section doesn't say so. RESTler's producer-consumer inference catches this when names match; it misses subtler cases (stateful flags, feature-gated endpoints).
- **Implicit value constraints.** A `startDate` field must be before an `endDate` field. A `quantity` must not exceed an inventory field. These are business-logic constraints that JSON Schema cannot express.
- **Implicit input distributions.** What values are *realistic* for a string field named `sku` or `vehicleClass`? Pure schema fuzzing picks random strings; the spec doesn't say what a real SKU looks like.

## Approach

Two phases:

### Phase 1: Curiosity-driven RL exploration

A deep RL agent's action space is the set of OpenAPI operations. Its state is a representation of the API's current visible state (which endpoints have returned success, which returned errors, what responses were seen). Its reward is *curiosity* — reaching states that the agent has not seen before.

The curiosity reward drives the agent to try novel sequences of operations, discovering implicit ordering (operation B works only after operation A) by trial and error rather than by inference from the spec. Failed calls are still informative — they teach the agent "B doesn't work in this state."

### Phase 2: Exploit and generate input values

Once the agent has built up experience with which sequences reach deep states, it alternates exploration with **exploitation** — mutating successful sequences to probe parameter boundaries and test coverage. At this stage, an LLM is used to propose realistic parameter values given the endpoint's name, description, and parameter metadata. The paper reports that LLM-generated values are materially better than rule-based generators for semantically rich fields.

## Claimed results

- Higher branch/line coverage than RESTler and EvoMaster on the benchmark APIs used in the paper.
- Superior fault detection on a subset of services.
- Meaningful exploration depth — DeepREST reaches parts of the API that spec-only tools do not touch.

## Adversarial reading

1. **Reward shaping is the whole game.** Curiosity-driven RL works when novelty is a good proxy for progress. In REST APIs, it sometimes isn't — an agent can be "novel" by finding new 400 error messages and never reach the interesting business-logic states. The paper doesn't extensively characterise when curiosity-driven RL plateaus.
2. **LLM value generation is the lever, not the RL.** Much of the performance uplift comes from the LLM generating semantically sensible values. If you swap DeepREST's RL for an equally sophisticated search with the same LLM value generator, how much does the deep-RL component actually contribute? The paper doesn't run this ablation cleanly.
3. **Training cost.** Deep RL is sample-hungry. Each training step is a real HTTP call. Training against a production-scale API is expensive and time-consuming; the paper's benchmark APIs are small.
4. **No transfer between APIs.** The RL agent learns per-API. You have to retrain for every new API, losing any accumulated knowledge.
5. **No performance model.** Same limitation as every other REST test generator in this family: DeepREST optimises for coverage and correctness, not for load characteristics. It produces sequences, not workloads.
6. **LLM hallucination of values.** The LLM sometimes proposes values that are plausible-looking but wrong (a fake SKU that never existed). DeepREST can filter these by response codes but has no durable fix.

## Why this matters for performance generation

DeepREST is interesting to this research lane not because it does perf testing — it doesn't — but because it represents the **current SOTA for discovering implicit call-order constraints in REST APIs**. Any tool that wants to generate realistic multi-step performance workloads needs to solve exactly that problem (producer-consumer sequencing, stateful flows). DeepREST's RL + LLM combination is a plausible architecture for a *perf* tool that does the same thing with latency or throughput as the reward signal.

The natural extension: replace the curiosity reward with a latency-variance reward. Explore to find sequences that *trigger* high-variance responses. This would be a performance-fuzzing tool with the same architecture as DeepREST. No one has published this as of early 2024.

## Relation to other LLM-REST work

- **MASTEST** — multi-agent LLM approach, no RL.
- **LlamaRestTest** — smaller fine-tuned Llama models instead of GPT-4. Cost/performance trade-off study.
- **Fuzz4All** — universal fuzzing with LLMs, different problem (compiler testing) but related methodology.
- **RESTler + LLM hybrid (MDPI 2024)** — extends RESTler with LLM parameter values, much simpler than DeepREST but shares the "LLM values over spec-derived values" insight.

## Citations

- https://arxiv.org/abs/2408.08594 (arXiv abstract)
- https://arxiv.org/pdf/2408.08594 (PDF)
- ASE 2024 venue: https://conf.researchr.org/home/ase-2024