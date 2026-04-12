---
id: 01KNZ5SMKSGB6GP479FNDRP1H3
title: Self-Healing Test Scripts — Can LLMs Repair Drifted Tests?
type: permanent
tags: [self-healing, test-maintenance, llm, drift, performance-testing, ux-automation, concept]
links:
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ68KN59XANY9TX9WE0BYJH
    type: related
  - target: 01KNZ6GWG119PXENNVS99TD8PJ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:05:05.657666+00:00
modified: 2026-04-11T21:05:05.657672+00:00
---

# Self-Healing Test Scripts — Can LLMs Repair Drifted Tests?

## The problem self-healing tries to solve

UI and API automation tests are brittle: they break when the target changes even trivially. A renamed CSS class, a reshuffled JSON field, a new required header — and a week's worth of tests go red. Test-maintenance cost is repeatedly cited as the top reason test automation initiatives fail. The hope: an AI layer can detect the failure, understand the change, and repair the test automatically.

Two levels of drift to distinguish:

- **Test-code drift.** The test is still the right thing to do, but the locator or field name is stale. Self-healing repairs the code.
- **Workload-model drift.** The traffic profile the test represents no longer matches production — new endpoints became popular, old ones became rare, sessions got longer. Self-healing needs to update the *test design*, not the code. This is the harder, less-discussed problem.

Most published self-healing work is about the first problem. The second problem is where most of the value is for performance testing.

## State of the art on test-code self-healing

### UI-level (browser tests)

- **testRigor** — commercial; uses LLMs plus a DOM-like abstraction layer to repair failing selectors. Markets "stable tests even if the UI changes."
- **refluent** — commercial; similar positioning.
- **Robot Framework self-healing agents** — open-source, MarketSquare/robotframework-selfhealing-agents on GitHub. Repairs broken locators using LLM calls; roadmap includes other failure classes.
- **Zero-cost DOM accessibility tree extraction** — ArXiv 2603.20358. Argues that LLM-based self-healing has a real economic problem: at 300 tests/day the API cost hits $1,350–2,160/month, so the paper presents a *structured* (non-LLM) self-healing approach as a cheaper alternative. This is a useful signal that self-healing-with-LLMs is not free and teams are building alternatives.

### API-level (REST/integration tests)

Less published work here. The typical pattern: when a test fails because the response schema changed, the LLM inspects the diff between old and new schema, proposes updates to the test's assertions and request body, and re-runs. Commercial tools exist (TestSprite, ACCELQ); open-source work is thin.

### Performance-test-specific self-healing

Even thinner. The dominant failure modes in perf tests are:

- **Auth drift** — the token refresh endpoint changed, the whole test stops working. An LLM can in principle inspect the login flow and fix the test.
- **Endpoint drift** — an endpoint was renamed or its request shape changed. Same.
- **Threshold drift** — the p95 SLO changed from 200 ms to 150 ms. The test's `thresholds` block needs updating. An LLM could read the SLO doc and produce the config.
- **Ramp/scenario drift** — production traffic shape changed. The test's `scenarios` block has an outdated RPS or VU count. An LLM *with access to production telemetry* could notice and propose an update; without telemetry it cannot.
- **Data drift** — the test's data file points to users that no longer exist. An LLM with access to the user database could refresh; without it cannot.

The pattern: LLM-based self-healing for perf tests is rate-limited by how much *production signal* the LLM has access to. A self-healing system wired into observability (k6 → Grafana → LLM agent with read access to metrics) is much more powerful than one without.

## Why LLMs are good at this narrowly

Self-healing is an instance of "read error message, read test code, propose a diff." LLMs are very good at this pattern because:

- Error messages are a compact, high-signal input.
- The required edit is usually small and localised.
- There is a clear oracle — does the repaired test pass?
- The search space is small — usually just variations on a locator or field name.

This is roughly why "autorepair a unit test that breaks after a compatible API change" is the most successful published LLM code-editing task.

## Why LLMs are bad at this broadly

1. **Silent behaviour change.** If the API's response changed in shape but the test passes anyway (because the assertions only check status), the test is now subtly wrong. LLM self-healing doesn't trigger because there's no error to react to. The test *should* have been updated; it wasn't.
2. **Hallucinated fixes.** LLMs sometimes "repair" a test by loosening assertions (`expect(x).toBe(42)` → `expect(x).toBeDefined()`). The test passes but covers nothing. This is a well-known pathology in unit-test self-healing papers — and it is *worse* in perf testing, where loosening a p99 threshold hides real regressions.
3. **No production-traffic awareness.** Test-code self-healing does not notice when the *workload profile* is stale. That requires continuous comparison of test workload to production workload, which is a separate engineering discipline.
4. **Cost.** Referenced above: not cheap at scale. Cost scales linearly with failing tests, which means a CI environment with flaky tests burns LLM budget on repairs that shouldn't happen.
5. **Trust and review.** Auto-committing LLM-proposed test repairs without human review creates a slow drift where tests gradually become more permissive than the engineering team intended. The production-grade answer is to make repairs PRs for human review, which defeats part of the velocity gain.

## The research gap — self-healing workload models

The interesting open problem: a self-healing system that monitors production traffic, compares the current load-test workload to the observed production workload, and proposes updates to the test design (new scenarios, updated rates, updated data) when drift exceeds a threshold.

Components needed:

- A workload-comparison metric. (What does "my test is 20% drifted from production" mean?)
- Access to production telemetry (RUM, access logs, traces).
- An LLM-driven synthesis step that proposes a new test spec.
- Human-in-the-loop review because auto-update without review is too risky.

None of this has been assembled into a published paper or open-source project as of 2024.

## Citations

- Self-healing e2e tests with LLMs (itnext): https://itnext.io/self-healing-e2e-tests-reducing-manual-maintenance-efforts-using-llms-db35104a7627
- testRigor: https://testrigor.com/blog/self-healing-tests/
- Robot Framework self-healing: https://github.com/MarketSquare/robotframework-selfhealing-agents
- QA Wolf taxonomy of self-healing AI types: https://www.qawolf.com/blog/self-healing-test-automation-types
- Zero-cost DOM accessibility paper: https://arxiv.org/pdf/2603.20358
- AccelQ self-healing overview: https://www.accelq.com/blog/self-healing-test-automation/
- Refluent: https://medium.com/refluent/how-to-implement-self-healing-tests-with-ai-640b0c8139a4