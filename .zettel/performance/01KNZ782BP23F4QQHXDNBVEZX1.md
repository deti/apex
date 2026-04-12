---
id: 01KNZ782BP23F4QQHXDNBVEZX1
title: Besbes et al. 2025 — Mozilla Perfherder Dataset of Performance Measurements and Alerts
type: literature
tags: [besbes, costa, mujahid, mierzwinski, castelluccio, mozilla, perfherder, dataset, 2025, msr, benchmark-data, zenodo]
links:
  - target: 01KNZ6T75SFGWWGZPNF1AXR09K
    type: related
  - target: 01KNZ6T765RV5SX44WKJKCEPVJ
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:30:27.062730+00:00
modified: 2026-04-11T21:30:27.062734+00:00
---

*Source: Mohamed Bilel Besbes, Diego Elias Costa, Suhaib Mujahid, Gregory Mierzwinski, Marco Castelluccio — "A Dataset of Performance Measurements and Alerts from Mozilla (Data Artifact)" — arXiv:2503.16332, March 2025. Dataset on Zenodo, DOI 10.5281/zenodo.14642238.*

One of the few publicly-available **industrial-scale datasets** of real CI performance measurements with **labelled regression alerts and linked bug reports**. This is a significant resource for researchers developing new change-point detection, anomaly detection, or ML-based regression detection methods — the lack of real-world datasets was a long-standing obstacle to progress in this area.

## What the dataset contains

From the paper:

- **5,655 performance time series** — each corresponds to one Firefox benchmark (a specific metric on a specific platform).
- **17,989 performance alerts** — the output of Perfherder's automated regression detection system (t-test on post-change windows, see Perfherder note).
- **One year of data**: May 2023 to May 2024.
- **Bug annotations**: alerts are linked to Bugzilla bugs where sheriffs triaged them, so there's ground-truth for "was this alert a real regression?".
- **Testing metadata**: platform, browser configuration, test type (Talos, Raptor, AWFY), etc.

The dataset is built by scraping the public Perfherder API and linking alerts to their bug records. It is released on Zenodo under an open license.

## Why this is a big deal

Performance regression detection research has suffered from a chronic problem: **no real-world data**. Papers proposing new algorithms have historically evaluated on:
- Small synthetic time series (Matteson-James style).
- Open-source project microbenchmarks with short histories.
- Artificially-injected regressions.
- Industrial data that the authors can see but can't release.

None of these approaches capture the full messiness of a real CI system: multi-year histories, heterogeneous benchmarks, platform-specific noise, gradual drifts, sheriff labelling noise, commit-range ambiguity. The Besbes et al. dataset is the first publicly-available approximation of all of these.

## Intended uses

The paper enumerates research directions the dataset enables:
1. **Change-point detection algorithm evaluation.** Run PELT, E-divisive, Bayesian change-point, etc. against the dataset and compare to Perfherder's sheriff-confirmed alerts. Which algorithm finds the most real regressions with the fewest false positives?
2. **Anomaly detection ML research.** Supervised learning from the ground-truth bug labels.
3. **Time-series forecasting** as a basis for anomaly detection.
4. **Regression prioritisation.** Given a batch of alerts, which should a sheriff look at first?
5. **Cross-platform analysis.** Are regressions typically visible on all platforms or a subset?
6. **Drift characterisation.** How do benchmark distributions drift over one year of hardware, OS, and Firefox changes?

## Why it matters for CI perf gating research

Anyone proposing a new CI perf gating technique can now evaluate against real-world data rather than synthetic tests. This allows:
- Apples-to-apples comparison between algorithms.
- Detection of algorithm weaknesses that only show up on real data (e.g., autocorrelation, non-stationarity, slow drifts).
- Honest false-positive rate estimates.
- Quantification of the "baseline drift" problem that threshold-based approaches suffer from.

For toolmakers, the dataset is also a good benchmark for new tool design: if your tool doesn't beat Perfherder's t-test-plus-threshold on this dataset, you don't have evidence it's actually better.

## Adversarial commentary

- **Perfherder's own labelling is noisy.** Sheriffs miss real regressions and label some flakes as real. Ground-truth from a real system is not perfect ground-truth.
- **Survivor bias in the dataset.** Benchmarks that were too noisy were removed from the suite or de-prioritised in the sheriff workflow. The dataset reflects "benchmarks Mozilla still cares about" not "benchmarks a fresh project would have."
- **Talos-specific noise profile.** Mozilla uses a dedicated hardware pool; their noise floor is lower than on typical cloud CI runners. Algorithms that look good on this dataset may perform worse on noisier CI.
- **One year is short.** Slow drifts with timescales longer than a year cannot be studied. Hardware refreshes happen infrequently and may appear as single large shifts rather than studies of gradual degradation.
- **Firefox-specific benchmarks.** Generalisation to, e.g., database systems (which have different bench characteristics) is not guaranteed. The dataset is one project, one type of workload.
- **Does not release the Perfherder alerting algorithm's internals.** Researchers comparing against "Perfherder's" detector have to re-implement it from docs, introducing implementation differences.

## Open questions the dataset enables

- What fraction of real regressions does t-test-plus-threshold miss?
- What fraction of sheriff-confirmed alerts does E-divisive also detect? With what lag?
- Can an LLM-based classifier predict whether an alert is actionable before a human sees it?
- Is there latent structure (e.g., clusters of correlated benchmarks) that a multivariate change-point detector could exploit?
- What is the false-positive rate of rolling-window Cohen's d gating on this data?

## Connections

- Mozilla Perfherder / Talos (dedicated note) — the system that generated the dataset.
- Daly et al. 2020 / Hunter 2023 — algorithms that could be evaluated against this.
- Collberg & Proebsting 2016 — the repeatability paper; Besbes et al. is an example of the kind of reusable artifact Collberg argued for.
- Chen & Revels 2016 — robust-statistics alternative whose claims could be tested here.

## Reference

Besbes, M. B., Costa, D. E., Mujahid, S., Mierzwinski, G., Castelluccio, M. (2025). *A Dataset of Performance Measurements and Alerts from Mozilla (Data Artifact)*. arXiv:2503.16332. Zenodo DOI 10.5281/zenodo.14642238.
