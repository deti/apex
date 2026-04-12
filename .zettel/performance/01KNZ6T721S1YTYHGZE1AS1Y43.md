---
id: 01KNZ6T721S1YTYHGZE1AS1Y43
title: Netflix Kayenta — Automated Canary Analysis with Mann-Whitney Judging
type: literature
tags: [kayenta, netflix, canary-analysis, spinnaker, mann-whitney, progressive-delivery, gating, open-source]
links:
  - target: 01KNZ6T74GY2FH1SD84MM81JYG
    type: related
  - target: 01KNZ67FDMCEA0MKZ8GZ841NDT
    type: related
  - target: 01KNZ706H6QYSH7ABYNJ98K150
    type: related
  - target: 01KNZ72G5SVY6JH66N7BP825C6
    type: related
  - target: 01KNZ6T75SFGWWGZPNF1AXR09K
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:22:53.121936+00:00
modified: 2026-04-11T21:22:53.121942+00:00
---

*Source: Netflix Tech Blog — "Automated Canary Analysis at Netflix with Kayenta" — Michael Graff, Chris Sanden et al. (2018). Also: cloud.google.com/blog/products/gcp/introducing-kayenta-an-open-automated-canary-analysis-tool-from-google-and-netflix, github.com/spinnaker/kayenta, spinnaker.io/docs/guides/user/canary/judge/.*

Kayenta is an open-source automated canary analysis (ACA) service developed jointly by Netflix and Google and shipped as part of the Spinnaker continuous delivery platform. It turns canary deployment — the pattern where a new version runs alongside the old version on a small fraction of traffic — into a **statistical gate**: pass/fail judgments on whether the canary is good enough to promote, made by a mechanical "judge" running rank-based tests.

## The canary gating problem

A canary is a technique from progressive delivery: deploy new version V2 to a small fraction (say 1%) of instances, route a matching fraction of production traffic to it, observe metrics, and either promote V2 (if it looks healthy) or roll back. The hard question is **how do you know it looks healthy?**

Traditional answers:
- **Human eyeballing.** A release engineer watches a dashboard for 15 minutes. Error-prone, slow, doesn't scale across many services.
- **Fixed thresholds.** "Error rate must be below 0.1%." Doesn't adapt to baseline drift; produces false alarms when the baseline is noisy.
- **Pairwise comparison.** "Canary error rate ≤ baseline error rate + 10%." Flaky on small samples.

Kayenta's approach: **statistical comparison of two time-series, judged mechanically, returning a pass/marginal/fail verdict**.

## Architecture

Spinnaker's canary stage:
1. Deploy the **baseline** — a fresh instance of the *current* production version (not the existing production fleet, to avoid comparing to a steady-state that may include warm caches and historical state).
2. Deploy the **canary** — a fresh instance of the new version.
3. Route a small fraction (~1%) of production traffic to both, in parallel.
4. Run for a duration (5–60 minutes typical).
5. Kayenta fetches metrics from a metrics backend (Prometheus, Atlas, Datadog, Stackdriver, SignalFx, Graphite, Wavefront, NewRelic).
6. Kayenta's **Canary Judge** compares the baseline and canary metric time series, returns a score.
7. Spinnaker promotes, holds, or rolls back based on the score.

Crucially, the baseline is a **fresh** deployment, not production. This controls for the "warm cache" confounder where production has been running for days with warm JITs and filled caches that a freshly deployed canary hasn't yet populated. Both the canary and the baseline are freshly deployed so they are comparably cold. This is an important design decision that teams rolling their own canary systems often miss.

## The Canary Judge — Mann-Whitney U + metric aggregation

The judge is the heart of Kayenta. Netflix's production judge (`NetflixACAJudge`) uses the **Mann-Whitney U test** as its default metric classifier (see dedicated note on Mann-Whitney).

For each configured metric:
1. Pull time series from baseline and canary.
2. Run Mann-Whitney U to test whether the canary's distribution is stochastically different from the baseline's.
3. Classify as Pass / High / Low / Nodata based on the p-value and configured direction (e.g., for error rate, only "canary higher" is bad; for throughput, only "canary lower" is bad).
4. Assign a metric weight.

Then aggregate across metrics:
- Compute the **canary score** as `(weighted sum of passing metrics) / (total weight) × 100`.
- If 9/10 metrics pass equally-weighted, score = 90.

The score is then classified as Success / Marginal / Failure against configured thresholds (e.g., `Success ≥ 95`, `Marginal [75, 95)`, `Failure < 75`).

The pipeline proceeds: Success → promote, Marginal → manual approval or continue to longer canary, Failure → roll back.

## Why Mann-Whitney (and not a t-test)

Kayenta's choice of Mann-Whitney is deliberate. Latency and error-rate distributions are non-normal, right-skewed, and often multimodal (GC pauses, backend retries). A t-test would make a Gaussian assumption that is usually wrong; Mann-Whitney's rank-based approach is robust to these pathologies.

The trade-off, also discussed in the Mann-Whitney note: Mann-Whitney has no direct effect-size output, so a very small but significant shift — common with large `n` on production traffic — can flag as "Fail" even if the shift is within business-acceptable bounds. Kayenta partially addresses this by allowing configured **tolerances** and **direction filters**.

## What makes Kayenta different from MongoDB-style CPD

Kayenta is **prospective**: it judges a *proposed* change before it's promoted. Daly/Hunter-style change-point detection is **retrospective**: it finds regressions in a time series after they've landed. Both are complementary:
- Canary analysis catches regressions before they reach full production — but only for bugs that manifest under a small traffic sample in a short time window.
- Change-point detection catches regressions after they land — including bugs that only show up under full load or over longer time spans.

A mature CI/CD practice uses both.

## Adversarial commentary

- **Mann-Whitney with very large n flags trivial shifts.** A 5-minute canary on a production service with 10k RPS generates 3M samples per side; Mann-Whitney p-values are tiny for even 0.1% shifts. Kayenta mitigates with tolerances but the default behaviour surprises new users.
- **No explicit effect size.** Users have to ask "how big is the difference?" separately. Adding Cliff's delta / Vargha-Delaney A-hat to Kayenta's default output has been discussed but not yet implemented.
- **Traffic steering assumes the load balancer is honest.** If sticky sessions or shard affinity causes the canary to see a skewed subset of requests, the comparison is invalid. Netflix addresses this at the load balancer layer (Eureka, Zuul) but it's a tight coupling.
- **Short canary windows miss slow-burn bugs.** A memory leak that takes 2 hours to OOM is invisible to a 15-minute canary. Longer canary windows reduce deployment velocity.
- **Per-metric decisions don't compose well.** Nine metrics passing and one failing produces a score of 90% regardless of which metric fails. A single critical metric (e.g., payment success rate) should probably be a hard veto, but Kayenta's aggregation is by default weighted-averaging. Weights have to be set carefully.
- **Open-source adoption is uneven.** Kayenta is tightly tied to Spinnaker; using it outside Spinnaker is possible but painful. Teams on Argo CD, Flux, or home-grown delivery systems have largely built their own analogues (Argo Rollouts, Flagger, see dedicated notes).

## Connections

- Mann-Whitney U (dedicated note) — statistical core.
- Effect size / Cliff's delta (dedicated note) — what Kayenta is missing.
- Argo Rollouts / Flagger (dedicated note) — Kubernetes-native alternatives.
- Daly et al. 2020 — retrospective counterpart.
- Google SRE Workbook chapter on implementing SLOs — SLOs as gates fit naturally on top of Kayenta's output.

## References

- Netflix Tech Blog 2018 — "Automated Canary Analysis at Netflix with Kayenta"
- Spinnaker docs — "How canary judgment works" (spinnaker.io/docs/guides/user/canary/judge/)
- github.com/spinnaker/kayenta
