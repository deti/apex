---
id: 01KNZ6T74GY2FH1SD84MM81JYG
title: Argo Rollouts & Flagger — Kubernetes Progressive Delivery with Analysis Templates
type: literature
tags: [argo-rollouts, flagger, kubernetes, progressive-delivery, canary-analysis, prometheus, analysis-template, gating]
links:
  - target: 01KNZ6T721S1YTYHGZE1AS1Y43
    type: related
  - target: 01KNZ706H6QYSH7ABYNJ98K150
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:22:53.200419+00:00
modified: 2026-04-11T21:22:53.200424+00:00
---

*Sources: argo-rollouts.readthedocs.io/en/stable/features/analysis/, github.com/argoproj/argo-rollouts, docs.flagger.app, github.com/fluxcd/flagger.*

Argo Rollouts and Flagger are the two dominant Kubernetes-native progressive delivery controllers. Both take the Kayenta idea — gate promotions on metric-based analysis — and implement it as Kubernetes CRDs that sit on top of whatever metrics backend the cluster already has. For CI perf gating on Kubernetes workloads, they are the de-facto tooling.

## The shared model

Both tools operate on a **rollout** custom resource that wraps a Kubernetes Deployment with a progressive strategy:
- **Canary**: route a small percentage of traffic to the new version, grow over time.
- **Blue/green**: run both versions in parallel, cut over atomically.
- **A/B with analysis**: add explicit metric-gating between phases.

At each stage transition, the controller runs a **metric analysis**: query the metrics backend, check if results satisfy a condition, and either proceed, hold, or roll back.

## Argo Rollouts — AnalysisTemplate

Argo Rollouts defines analysis as a separate CRD (`AnalysisTemplate`), which is referenced from a `Rollout`'s progressive strategy. An AnalysisTemplate looks like:

```yaml
apiVersion: argoproj.io/v1alpha1
kind: AnalysisTemplate
metadata:
  name: success-rate
spec:
  metrics:
  - name: success-rate
    interval: 5m
    count: 5
    successCondition: result[0] >= 0.95
    failureLimit: 3
    provider:
      prometheus:
        address: http://prometheus.monitoring:9090
        query: |
          sum(irate(http_requests_total{service="{{args.service-name}}",code!~"5.."}[5m])) /
          sum(irate(http_requests_total{service="{{args.service-name}}"}[5m]))
```

Key fields:
- **`interval`** — how often to poll the metric (here, every 5 minutes).
- **`count`** — how many measurements to take (5 measurements = 25 minutes of analysis).
- **`successCondition`** — expression evaluated against each measurement; if true, pass.
- **`failureLimit`** — after this many failures, abort and roll back.
- **`provider`** — Prometheus here, but also: Datadog, NewRelic, Wavefront, CloudWatch, Graphite, InfluxDB, Kayenta (yes, Argo Rollouts can delegate to Kayenta), Job, and web-based HTTP queries.

Supported metric providers: Prometheus, Datadog, NewRelic, Wavefront, CloudWatch, Graphite, InfluxDB, **Kayenta**, Job (run a K8s Job and check its exit code), Web (HTTP endpoint returning JSON).

## Flagger — MetricTemplate

Flagger uses a similar pattern with two CRDs (`MetricTemplate` and `Canary`) and a slightly different templating syntax:

```yaml
apiVersion: flagger.app/v1beta1
kind: MetricTemplate
metadata:
  name: error-rate
spec:
  provider:
    type: prometheus
    address: http://prometheus:9090
  query: |
    sum(rate(http_requests_total{namespace="{{ namespace }}",
                                  app="{{ target }}",
                                  status!~"5.."}[{{ interval }}])) /
    sum(rate(http_requests_total{namespace="{{ namespace }}",
                                  app="{{ target }}"}[{{ interval }}]))
```

Flagger's provider list is smaller — Prometheus, Datadog, CloudWatch, NewRelic, Graphite, plus custom webhooks.

Flagger integrates with more service meshes out of the box (Istio, Linkerd, App Mesh, Open Service Mesh, Gloo, NGINX, Skipper, Traefik, Contour, Kuma) and uses them for traffic splitting. Argo Rollouts has a similar but slightly shorter list.

## What "metric analysis" does for perf gating

For **performance**-specific gating, the canonical pattern is:

1. Define a `MetricTemplate` / `AnalysisTemplate` that queries a latency histogram (e.g., `histogram_quantile(0.99, sum(rate(http_request_duration_seconds_bucket[5m])) by (le))`).
2. Set a `successCondition` like `result[0] < 250` (p99 under 250ms).
3. Reference it in the Rollout's canary steps.
4. On deployment, traffic progressively shifts; each step runs the analysis; if the canary's p99 exceeds 250ms for more than `failureLimit` intervals, roll back.

This gives you a **perf SLO as a deployment gate** — the SLO is the oracle (see dedicated note on SLOs as oracles).

## Differences from Kayenta

- **Kayenta does statistical comparison** (Mann-Whitney U on time series of baseline vs canary).
- **Argo Rollouts / Flagger do threshold checks** on a single time series (the canary).

This is a significant philosophical difference. Kayenta answers "is the canary statistically different from the baseline?" Argo Rollouts answers "does the canary meet a fixed threshold?" The Kayenta model adapts to baseline drift; the threshold model does not.

You can combine the two: Argo Rollouts supports delegating to Kayenta as a provider, so you get threshold-based steps plus a Kayenta-judged step before full promotion. In practice many teams start with threshold-based Argo Rollouts / Flagger and add Kayenta only when the complexity is justified.

## Adversarial commentary

- **Thresholds don't adapt.** Flagger's and Argo Rollouts' default is threshold-based, which inherits all the problems of fixed-threshold pairwise comparison (see Welch's t-test note and Daly 2020). A week of high traffic drifts the baseline and the threshold fires falsely; a week of low traffic makes the gate trivially passable. Teams typically paper over this by making thresholds overly generous, which loses sensitivity to real regressions.
- **No effect-size reporting.** Like Kayenta, no Cliff's delta / Cohen's d output.
- **Polling-based metric collection has latency.** The controller queries Prometheus every `interval` seconds; a sudden regression in the canary takes at least `interval + polling delay` to be detected. For fast-moving bugs, this adds minutes of bad user experience.
- **Query construction is error-prone.** A typo in a PromQL label selector silently returns zero samples and the analysis passes. Always test your `AnalysisTemplate` queries in a dry run against production metrics before relying on them as gates.
- **`failureLimit` is binary.** The condition either fails or passes; there's no graded score like Kayenta's 0–100. Composing multiple metrics into a single promotion decision requires either chaining multiple analysis steps or writing a bespoke composite metric upstream in Prometheus.
- **Rollouts analysis is decoupled from PR validation.** You test the binary once it's in staging or production. Pre-merge perf validation needs a different tool (microbenchmark gate in CI, Bencher.dev, CodSpeed).

## Connections

- Kayenta (dedicated note) — statistical-comparison alternative.
- SLOs as perf oracles (dedicated note) — threshold-based analysis templates are essentially SLO checks.
- Google SRE Workbook chapter 5 — burn-rate alerting is a more sophisticated model that Argo Rollouts does not implement but could be built on top of.
- Bencher.dev / CodSpeed — pre-merge counterparts to the deployment-time gating done here.

## References

- Argo Rollouts docs: argo-rollouts.readthedocs.io/en/stable/features/analysis/
- Flagger docs: docs.flagger.app
- github.com/argoproj/argo-rollouts, github.com/fluxcd/flagger
