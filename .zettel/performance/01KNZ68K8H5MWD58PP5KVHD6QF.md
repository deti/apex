---
id: 01KNZ68K8H5MWD58PP5KVHD6QF
title: Test Generation from Static Code Analysis — Targeted Load for N+1 and Hot Paths
type: permanent
tags: [static-analysis, code-analysis, n-plus-one, hot-path, test-generation, targeted-load, concept]
links:
  - target: 01KNZ68KB81AGB3MWRNS87EKXK
    type: related
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:13:15.793556+00:00
modified: 2026-04-11T21:13:15.793562+00:00
---

# Test Generation from Static Code Analysis

## The idea

Instead of driving load blindly from API specs or access logs, use **static analysis of the application source** to find code patterns that are likely to be performance-sensitive (N+1 queries, deeply nested loops, regex use on user input, unbounded allocations, tree rebalancing operations) and generate *targeted* load tests that exercise exactly those paths. This is a much sharper tool than generic load testing — you're not trying to cover the whole service, you're trying to prove that specific risky code paths can or can't handle load.

This is philosophically analogous to the SlowFuzz / PerfFuzz / Badger tradition already in the vault, but applied at the **HTTP endpoint** level rather than the function level: starting from the pathological code and working backward to the HTTP calls that reach it.

## The analysis-to-test pipeline

1. **Parse the application source into an AST or CFG.** Tree-sitter, LSP servers, or framework-specific tools (for Rails: Brakeman; for Django: bandit + bandit-django) give you the raw graph.
2. **Enumerate HTTP handlers.** For each handler, collect the set of functions transitively called. Most web frameworks have conventions that make handler enumeration easy (Express routes, Django URL conf, Spring `@RequestMapping`).
3. **Scan handler-reachable code for suspicious patterns.**
   - **N+1 query pattern.** Any loop whose body contains a database call. Rails' `bullet` gem is the canonical detector.
   - **Unbounded regex over user input.** Any regex compiled from an input string or applied to request bodies. A known-risky regex flavour (`*`, `+`, backrefs) is a stronger signal.
   - **Recursion.** Any direct or indirect recursion. Combined with user-controlled input size, this is a complexity attack surface.
   - **Sort/tree/dictionary operations on collections.** Any `sorted()`, `list.sort()`, tree-insert, map-insert whose size is user-controlled.
   - **Deeply nested loops.** Any triple-nested `for` where the inner iteration bound is not a constant.
   - **Large allocation with size from input.** Any `new ArrayList(n)`, `malloc(n*sizeof...)`, Python list multiplication where n is user-provided.
4. **Trace from pattern to reaching input.** For each suspicious site, find the parameter in the HTTP handler that controls the pattern's worst-case behaviour. This is the classic taint-tracking problem.
5. **Generate a load test that maximises that parameter.** If the parameter is a list size, send a request with a 10,000-element list. If it's a string length, send a 1 MB string. If it's a regex input, send a crafted ReDoS catastrophic-backtracking input.
6. **Measure.** Run the generated test, compare latency to a baseline with normal inputs, and flag regressions.

## What tools do pieces of this today

- **Bullet (Rails).** The reference N+1 detector. It's runtime-instrumentation, not static analysis, but it identifies the pattern and the request that triggered it. A generator could read Bullet's output and produce a test for each finding.
- **pg_stat_statements (Postgres).** Finds the slowest queries aggregated over time. A generator that starts from the top queries and traces back to the HTTP calls that issue them (via ORM query-source tracking) would produce a test suite pointed at the actual DB hot spots. pg_stat_statements-based perf analysis is standard; *generating tests* from it is not.
- **Datadog / Dynatrace "slowest query" profilers.** The commercial APM products track per-endpoint query fan-out and expose it in dashboards. They could emit test-generator specs but don't.
- **CodeQL / Semgrep / regex-detector tools.** Static-analysis frameworks that already pattern-match code. The existing security-oriented rules (injection, auth bypass) could be extended with perf-oriented rules (N+1, ReDoS, unbounded allocation). Semgrep has some perf rules but they're thin.
- **API fuzzing tools with white-box instrumentation (EvoMaster).** Already covered. These do dynamic analysis driven by coverage; they don't target known perf patterns.

## What's missing

No open-source tool connects the full pipeline:

1. Scan source code for known perf anti-patterns.
2. Trace each match to the reaching HTTP endpoint and parameter.
3. Generate a load-test case specifically parameterised to trigger the pattern.
4. Run it and produce a per-pattern regression report.

This is the clearest white-space opportunity for a perf-focused analog of SAST tools. Given that Semgrep and CodeQL have already solved the pattern-matching infrastructure, the remaining work is the pattern library and the test-emitter.

## Database hot-path analysis — a close sibling

The analogous problem at the DB layer is well-trod:

- **EXPLAIN ANALYZE** gives you the query plan and costs for a specific query. Running EXPLAIN on top pg_stat_statements queries gives you the ranked list of worst offenders with plans.
- **pgBadger** is a log-analyzer that produces pg_stat_statements-like aggregates from raw Postgres logs. Useful when you don't have the extension enabled.
- **Slow query logs** in MySQL are analogous.

A pipeline from "slow query list" → "which endpoint triggers this query" → "generate a load test that maximises that endpoint's rate" is implementable but requires query-to-endpoint attribution, which is the hard part. Modern APM tools (Datadog, New Relic) do it at runtime via trace context. Generating tests from it is the missing piece.

## Failure modes

1. **Pattern-matching is lossy.** Many real perf bugs are data-dependent in ways a static pattern cannot find (e.g., a query is fine with small tables and explodes at 1M rows — the code is the same).
2. **Taint tracking is hard.** Getting from a suspicious code site to the HTTP parameter that controls it requires inter-procedural analysis that breaks on dynamic dispatch, ORM abstractions, and generic collection-processing code.
3. **False positive rate.** Many loops-with-DB-calls are intentionally bounded to small N. A test that hits them with N=10,000 is not a realistic workload — it's finding a bug that doesn't exist in the operating regime.
4. **Orthogonal to workload realism.** A targeted load test for a specific bug is useful for regression detection but should not be confused with a realistic workload test. The two complement each other.
5. **Framework specificity.** Every pattern library has to be re-implemented per framework (Rails vs. Django vs. Spring vs. Express). CodeQL's cross-language model helps but not fully.

## Why this lane is important for the APEX fleet

The existing APEX spec (G-46) emphasises fuzzing-driven perf-bug detection. The code-analysis lane is the *other half* of that picture: deterministic targeting of known anti-patterns that doesn't need a fuzzing budget and gives guaranteed coverage of a curated pattern library. A serious perf-test-generation tool should have both modes.

## Citations

- Bullet: https://github.com/flyerhzm/bullet
- pg_stat_statements guide (Aiven): https://aiven.io/docs/products/postgresql/howto/identify-pg-slow-queries
- pg_stat_statements optimisation guide (Tigerdata): https://www.tigerdata.com/blog/using-pg-stat-statements-to-optimize-queries
- Semgrep: https://semgrep.dev/
- CodeQL: https://codeql.github.com/
- Brakeman (Rails SAST, some perf patterns): https://brakemanscanner.org/
- Cybertec slow-query detection guide: https://www.cybertec-postgresql.com/en/3-ways-to-detect-slow-queries-in-postgresql/