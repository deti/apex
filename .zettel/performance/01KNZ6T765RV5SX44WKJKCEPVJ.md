---
id: 01KNZ6T765RV5SX44WKJKCEPVJ
title: Chrome Pinpoint — Bisection-Based Perf Regression Attribution
type: literature
tags: [chrome, pinpoint, bisection, chromium, performance-regression, perf-waterfall, sheriff, attribution]
links:
  - target: 01KNZ6T75SFGWWGZPNF1AXR09K
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:22:53.253812+00:00
modified: 2026-04-11T21:22:53.253814+00:00
---

*Sources: chromium.googlesource.com/chromium/src/+/HEAD/docs/speed/bisects.md, /perf_trybots.md, /addressing_performance_regressions.md, /perf_regression_sheriffing.md.*

Pinpoint is Chrome's internal performance bisection service. It solves a problem downstream of regression *detection*: once you know a regression happened somewhere in a range of commits, **which commit is responsible?** Pinpoint automates the bisection. Together with Chrome's perf waterfall dashboard and the Chromium perf sheriff rotation, it forms one of the most engineered examples of CI perf infrastructure in the industry.

## Why bisection is a separate problem

Change-point detection (Daly/Hunter) and threshold alerting (Perfherder, Kayenta) tell you "a regression landed somewhere in this time window." But the time window often contains **many commits**, especially when:
- Perf tests run on a sparse schedule (not every commit).
- Multiple upstream dependencies (e.g., V8, Skia, ANGLE) roll their own commits into Chromium as single commits, each containing many internal changes.
- Long batches of commits are tested together for throughput.

The Chromium docs make this explicit:

> "Since perf tests on chromium's continuous build are very long-running, they cannot run on every revision, and separate repositories like v8 and skia sometimes roll multiple performance-sensitive changes into chromium at once, so a tool is needed that can bisect the root cause of performance regressions over a CL range."

## How Pinpoint works

At a high level, Pinpoint is a web service that:
1. Takes a perf test, a metric, and a CL (commit) range as input.
2. Runs the test at both endpoints of the range on dedicated perf lab hardware.
3. Performs **binary-search bisection** on the CL range: pick the midpoint CL, build Chromium at that CL, run the test, see whether the metric is closer to the good or bad endpoint, recurse.
4. Continues until it converges on a single CL (or small cluster) responsible for the shift.
5. Files a regression bug with the blamed CL attached.

Key details from the Chromium docs:

> "Pinpoint wraps run_benchmark and provides the ability to remotely run A/B benchmarks using any platform available in the lab, and will run a benchmark for as many iterations as needed to get a statistically significant result."

Two things worth emphasising:
- **Variable iteration count.** Pinpoint runs as many iterations as needed to get statistical significance at each bisection step, rather than a fixed count. This means noisy benchmarks cost more (more iterations to converge) but do converge eventually.
- **A/B at every step.** Each midpoint is compared against both endpoints as an A/B pair, not against a running baseline. This controls for environmental drift during the bisection run.

## The perf waterfall

Pinpoint sits alongside the **Chromium perf waterfall** — a dashboard showing per-metric perf test results across commits on integration branches. The waterfall is the detection layer (simple threshold / t-test, conceptually similar to Perfherder). Pinpoint is the attribution layer. When the waterfall flags a regression, a sheriff can file a Pinpoint job to bisect.

From the docs:

> "Issues in the list will include automatically filed and bisected regressions that are supported by the Chromium Perf Sheriff rotation. A 'Master' refers to a logical group of bots that are running tests and are monitored by perf sheriffs."

Each "master" corresponds to a hardware configuration (e.g., Pixel 4 with specific Android build, or Linux desktop with specific GPU). A sheriff watches their master's waterfall, files Pinpoint jobs on alerts, triages the output.

## Perf try bots — pre-merge counterpart

Complementing Pinpoint (post-merge bisection), Chrome provides **perf try bots** that developers can invoke on a pre-merge patch. The try bot runs the same perf test on the same lab hardware against a Chromium build that includes the developer's patch. This is a pre-merge A/B test: "before this change" vs "with this change" on the exact same machine.

> "It's best to run a perf tryjob, since the machines in the lab are set up to match the device and software configs as the perf waterfall, making the regression more likely to reproduce."

Reproducibility depends on the perf try bots using **the same hardware pool** as the post-merge waterfall. If a regression shows up on the waterfall but the dev's perf try bot can't reproduce it, the first suspect is machine-pool heterogeneity — hence Chrome's investment in a dedicated perf lab with tight machine-configuration matching.

## Why this architecture beats "just use CPD"

Change-point detection (Daly/Hunter) is fine if you run benchmarks often enough that the responsible commit is isolated in a short window. When benchmark cost forces you to sparse scheduling, CPD narrows to a range but not a commit, and you still need bisection. Pinpoint is the piece that closes the gap. Conceptually:

```
CI waterfall → (threshold or CPD) → alert on range [CL_i, CL_j]
                                           ↓
                                        Pinpoint → bisect → blame specific CL
                                           ↓
                                        File bug
```

Most CI perf systems stop at the "alert on range" step and rely on human bisection. Pinpoint automates the human step, which is the main reason Chrome's performance team scales to the size it does.

## Adversarial commentary

- **Perf lab hardware is expensive.** Pinpoint assumes availability of identical hardware to run bisection jobs. For smaller projects this is infeasible; bisection on heterogeneous cloud runners inherits layout noise and is much less reliable.
- **Statistical significance at each bisection step adds runtime.** A deep bisection (log₂ of the range) × noisy benchmarks (dozens of iterations per step) can take hours or days. Chrome absorbs this cost; smaller teams can't.
- **Bisection assumes a monotone "bad" direction.** If the regression is actually bimodal (some commits make it slower, some faster, and the end-to-end effect is a net slowdown), binary search can misattribute. Chrome mitigates this by restricting Pinpoint to short ranges where monotonicity is plausible.
- **Multi-commit rolls (V8, Skia) are a special case.** When the CL range is a single "roll" that imports hundreds of upstream commits, Pinpoint's output is "the roll was responsible" — which is correct but useless for fixing. Chrome handles this with a second bisection pass on the rolled repository.
- **Pinpoint is not open-source** as a deployable service, although Chrome's telemetry and run_benchmark tooling are. Replicating the architecture requires reimplementing the service, which is non-trivial.

## Connections

- Daly et al. 2020 (CPD) — complementary: CPD finds the range, Pinpoint finds the commit.
- Perfherder sheriff rotation (dedicated note) — same operational model.
- Bisect-based tools in other domains: `git bisect run`, `mozregression` (Mozilla's cousin for Firefox).
- Mytkowicz et al. 2009 — why the perf lab hardware must be tightly controlled for Pinpoint to work.

## References

- Chromium docs: chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/speed/
  - `bisects.md`, `perf_trybots.md`, `addressing_performance_regressions.md`, `perf_regression_sheriffing.md`
- Perf Dashboard: chromeperf.appspot.com
