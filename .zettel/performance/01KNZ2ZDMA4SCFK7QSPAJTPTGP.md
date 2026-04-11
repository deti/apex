---
id: 01KNZ2ZDMA4SCFK7QSPAJTPTGP
title: "SRE Workbook Chapter 2: Implementing SLOs"
type: literature
tags: [sre, slo, sli, cuj, error-budget, google, workbook]
links:
  - target: 01KNZ2ZDM8YHSYRFSJ399TWDWY
    type: extends
  - target: 01KNYZ7YKPN5VE39GKTVDE9FB4
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://sre.google/workbook/implementing-slos/"
---

# SRE Workbook — Chapter 2: Implementing SLOs

*Source: https://sre.google/workbook/implementing-slos/ — fetched 2026-04-12.*
*Chapter author: Steven Thurgood (Google). One of the most-cited chapters in SRE literature.*

## The four prerequisites

Chapter 2 opens with the preconditions an organisation must meet before adopting error-budget-based SLOs:

1. **Stakeholder-approved SLOs aligned with product needs.** The SLO must reflect what customers care about, not what is easy to measure. Product management and reliability engineering must both sign off.
2. **Defensibility under normal circumstances.** The SLO must be achievable with the current architecture under expected load. If the SLO requires heroics to meet at baseline, the team will burn out.
3. **Organizational commitment to using error budgets for decision-making.** If product management can override an error-budget freeze on political grounds, the SLO is decoration. The organisation must commit in advance.
4. **A process for refining SLOs continuously.** SLOs are not set once; they evolve as the product, usage patterns, and infrastructure evolve.

## The five-step implementation recipe

### Step 1 — Identify Critical User Journeys (CUJs)

A **critical user journey** is a sequence of core tasks that represents an essential customer experience. For an e-commerce site: search, add to cart, checkout. For a search engine: query → results → click-through. CUJs matter because an SLO measured on "all requests" can look green while a specific high-value flow is actively failing.

CUJs are identified by:
- Following user flows through product analytics.
- Joining distinct log events to reconstruct session traces.
- Client-side instrumentation to capture the user's actual experience.

Critically, CUJs cross service boundaries. A single CUJ may involve 10 microservices, each with its own internal SLOs, and the CUJ SLO is not just the conjunction — it is a user-facing measurement that may require end-to-end instrumentation.

### Step 2 — Define SLI specifications and implementations

The chapter makes a sharp distinction:

- **SLI specification** — what the metric *means*. E.g., "Fraction of successful HTTP requests as seen by the user's browser, where 'successful' is HTTP status 200-299 or 304 and the response body matches expectation."
- **SLI implementation** — how the metric is *computed in practice*. E.g., "Ratio of `status_code{200-399}` to all responses in the nginx access log, sampled every minute, rolled up over 30-day window."

Specifications are durable; implementations change as measurement improves. Teams should document both, and should aim for the implementation to converge toward the specification over time.

### Step 3 — Measure SLIs

SLIs are always expressed as a **ratio of two numbers**: good events / total events. Examples:

- **Availability SLI** — good requests / total requests.
- **Latency SLI** — requests faster than threshold / total requests. (*Not* average latency — average is not robust and does not compose.)
- **Freshness SLI** — data updated in window / data expected to be updated.
- **Correctness SLI** — correct outputs / total outputs (requires ground truth).
- **Coverage SLI** — records processed / records received.
- **Durability SLI** — records retained at time-T / records written.

The ratio form is load-bearing: it makes the SLI dimensionless, window-comparable, and natively aggregatable (with appropriate caveats).

### Step 4 — Categorise services by type

Three categories, each with an associated SLI palette:

- **Request-driven services** — availability, latency. The canonical "backend" case.
- **Data pipelines** — freshness, correctness, coverage.
- **Storage systems** — latency, availability, durability, correctness.

This matters because the default SLI for a given category is different. Storage systems don't have user-perceivable "latency" until someone actually reads, so measuring it needs a synthetic probe.

### Step 5 — Set targets and iterate

The opening SLO should be **based on current performance, rounded to a manageable target**. Example: if availability is currently 99.83%, start at 99.5% or 99.8%, not 99.99%. Rationale:

- 99.9% is credible immediately and the team can adjust without panic.
- Tightening the SLO later is painless; loosening it is political.
- Error budget = `1 - SLO`. A 99.5% SLO gives a 3.6-hour monthly budget; 99.99% gives 4.3 minutes. The latter is inhuman for any team not already operating at that level.

## The decision matrix

Chapter 2 ends with a three-dimensional decision matrix for SLO health:

| SLO met? | Toil high? | Customer satisfied? | Action |
|---|---|---|---|
| Yes | No | Yes | Status quo; relax processes slightly. |
| Yes | Yes | Yes | Invest in automation to reduce toil. |
| Yes | — | **No** | **Tighten the SLO** — current bar too loose. |
| No | No | Yes | Loosen the SLO — measurement too strict. |
| No | Yes | No | Invest in reliability; potential all-hands. |

The Yes/Yes/No row is the quiet failure mode: the team is hitting its SLO but users are still unhappy, which means the SLO is measuring the wrong thing.

## Documentation deliverables

Effective SLO programs produce several documents:

- **SLI implementation specifications** — durable, version-controlled.
- **Error budget policy** — what escalates, when, and to whom. Appendix B of the Workbook is the canonical template.
- **Dashboards** — budget consumption rate and compliance trend, not just the instantaneous SLI.
- **Review cadence** — weekly early on, monthly at steady state.

The chapter concludes: *"Without SLOs, there is no need for SREs."*

## Relevance to APEX G-46

1. **Map the CUJ concept to APEX's "entry points".** APEX already enumerates public API entry points during test generation. Each entry point *is* a candidate CUJ for performance SLO verification. The chapter's recommendation to cross service boundaries supports APEX including full end-to-end tests (not just single-function benchmarks) in performance runs.

2. **Ratio-of-events framing for APEX regression detection.** Instead of "the median got 2x slower", APEX can report: "in N repeated runs, M% were within SLO threshold in the baseline and M'% are within threshold now". This composes across commits and across test suites. It is also the shape of a burn-rate alert the user can cut-and-paste into their production monitor.

3. **Start lenient, tighten later.** APEX's `apex perf baseline` command should default to a lenient threshold (e.g., 2x or 95-percentile of recent runs) rather than a tight one. The chapter's advice on starting loose and tightening over time applies directly.

4. **The five-step recipe is the `apex perf init` wizard.** APEX can literally walk a user through step 1 (identify entry point), step 2 (specify SLI), step 3 (record baseline), step 4 (categorise — is it request-driven? data-pipeline?), step 5 (set target) and emit a YAML config from the answers.

5. **Yes/Yes/No as an evaluation criterion for APEX itself.** If APEX-generated performance tests all pass but users still hit perf incidents, APEX's generator is missing the real CUJs. Worth auditing periodically against production postmortems.

## References

- Thurgood et al. — "Implementing SLOs" — SRE Workbook Ch. 2 — [sre.google](https://sre.google/workbook/implementing-slos/)
- SRE Workbook ToC — `01KNZ2ZDM8YHSYRFSJ399TWDWY`
- SRE Book SLO chapter — `01KNYZ7YKPN5VE39GKTVDE9FB4`
