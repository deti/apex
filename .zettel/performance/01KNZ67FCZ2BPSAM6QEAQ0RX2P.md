---
id: 01KNZ67FCZ2BPSAM6QEAQ0RX2P
title: MongoDB signal-processing-algorithms — open-source E-divisive library
type: literature
tags: [mongodb, change-point-detection, e-divisive-means, python, open-source, tool]
links:
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNZ67FCNDFZMA3XVZ7K6DNKF
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:12:39.071308+00:00
modified: 2026-04-11T21:12:39.071309+00:00
---

*Source: github.com/mongodb/signal-processing-algorithms — Python library, PyPI: `signal-processing-algorithms`.*

The open-source building block that MongoDB extracted from their Evergreen CI. It implements the algorithms described in Daly et al. 2020 (E-divisive means change-point detection) plus related energy-statistics tools. Available on PyPI for `pip install signal-processing-algorithms`.

## What the library provides

From the README and source:

> "A suite of algorithms implementing Energy Statistics, E-Divisive with Means and Generalized ESD Test for Outliers in Python."

Core entry point:

```python
from signal_processing_algorithms.energy_statistics import energy_statistics
change_points = energy_statistics.e_divisive(series, pvalue=0.01, permutations=100)
```

- `series`: a 1-D array of observed values (e.g., per-commit benchmark timings).
- `pvalue`: significance threshold for the permutation test used to confirm a candidate change-point.
- `permutations`: number of label shuffles to perform when estimating significance. 100 is the default; higher values reduce variance in the p-value estimate but cost CPU proportional to the number of permutations.

Returns a list of change-point indices into `series`.

## What it doesn't provide

Deliberately, it is *only* the algorithms — not the dashboard, not the triage workflow, not the data-fetching layer. The intent is that you plug this into your own CI infrastructure (Jenkins, GitHub Actions, Argo Workflows, whatever) and build UI around it. MongoDB's actual Evergreen integration is not open-sourced, only the numerical core.

## The E-divisive implementation

The algorithm is described in Matteson and James (2014). Briefly:
1. For each candidate split position `τ`, compute an energy statistic `Q(τ)` that measures how different the distribution is before and after `τ`.
2. Pick the `τ*` that maximises `Q(τ)`.
3. Permute the observation labels `permutations` times and re-compute `Q(τ*)`; count how often the permuted statistic exceeds the observed. That fraction is the p-value.
4. If the p-value is below threshold, accept the change-point and recurse on both sub-segments.

## Generalized ESD test for outliers

Also included is the Rosner (1983) Generalized Extreme Studentized Deviate (ESD) test for outliers, which MongoDB uses to flag anomalous individual runs before change-point detection (so a single bad run doesn't masquerade as a shift). This is an important practical preprocessing step that is often overlooked.

## Adversarial commentary

- **No documentation of non-determinism.** The permutation test is stochastic — two calls with the same series can return different change-points if permutations are low. The library doesn't explicitly seed RNGs, which is a footgun.
- **Performance.** E-divisive is O(n²) in the series length, plus O(permutations * n²) for significance testing. For long histories (years of daily commits) the cost adds up. MongoDB's approach is to run CPD incrementally over a rolling window, not on the whole history each time.
- **API surface is minimal.** Production users typically end up wrapping this library with glue code to handle windowing, outlier filtering, and alert aggregation. See Hunter (Fleming et al. 2023) for a more opinionated wrapper.
- **No multivariate support.** Matteson and James's original method supports multivariate series natively; the MongoDB implementation is univariate.

## Connections

- Implements Daly et al. 2020 (MongoDB CPD paper).
- Underlies Hunter (Fleming et al. 2023), which modifies the significance test.
- Complementary to `ruptures` (Truong et al.), another Python CPD library, and to Netflix's `kats` time-series toolkit.

## Reference

github.com/mongodb/signal-processing-algorithms — MIT license.
