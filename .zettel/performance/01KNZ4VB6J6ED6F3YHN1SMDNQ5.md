---
id: 01KNZ4VB6J6ED6F3YHN1SMDNQ5
title: "RED Method — Rate, Errors, Duration (Tom Wilkie)"
type: literature
tags: [red-method, tom-wilkie, weaveworks, grafana, microservices, methodology, monitoring]
links:
  - target: 01KNZ4VB6JHP7W47HM7QREWW53
    type: related
  - target: 01KNZ4VB6JXAZA2TBRCD5DERK9
    type: related
  - target: 01KNZ666TDP7H8GRG9RF62384D
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://grafana.com/blog/2018/08/02/the-red-method-how-to-instrument-your-services/"
---

# The RED Method

*Author: Tom Wilkie (co-founder of Kausal, later engineering director at Grafana Labs; author of Loki, Mimir, Cortex).*
*Coined ~2015. Primary source: https://grafana.com/blog/2018/08/02/the-red-method-how-to-instrument-your-services/*

## The rule

For every *service*, track three metrics:

- **R — Rate**: the number of requests per second.
- **E — Errors**: the number of those requests that are failing.
- **D — Duration**: the amount of time those requests take.

That's the whole method. Three metrics per service, unified across every service in a microservices estate.

## Why it was coined

Tom Wilkie developed RED at Weaveworks after joining from the Kausal acquisition. The immediate need was a monitoring philosophy for a microservices platform where (1) there are dozens of services, (2) each has its own internal complexity, and (3) operators cannot know each service's internals deeply. USE Method works but requires enumerating hardware resources for each service — too much cognitive load at microservice scale. RED trades completeness for uniformity: every service is monitored the same three ways, and the uniformity makes cross-service comparison and alerting tractable.

## RED vs USE

Wilkie himself characterises RED and USE as *complementary views of the same system*:

- **USE** monitors *resources* (CPU, memory, disk, NIC, ...). Applied to hosts or infrastructure. Asks "is any resource stressed?".
- **RED** monitors *services* (from the caller's perspective). Asks "is any service unhappy, and if so how?".

A mature estate uses both: USE dashboards for infrastructure, RED dashboards for services. A stressed service can be caused by a stressed resource, and the two dashboards help you pivot from symptom (RED) to cause (USE).

## What RED catches well

1. **Requests-per-second dropping.** Rate going to zero on a normally busy service is a clear outage signal, often earlier than error-rate alarms.
2. **Error-rate spikes.** The standard "page the on-call" signal.
3. **Duration drift.** p99 climbing over days/weeks without errors being triggered — the classic pre-outage warning.
4. **Traffic redistribution.** Rate on one shard going up while others stay flat → uneven load balancer or hot key.
5. **Compatibility with SLIs and SLOs.** The three RED metrics map directly to the canonical SLIs in Google SRE's workbook: request rate ≈ traffic, error count ≈ errors, duration ≈ latency. An SLO based on the RED triplet is effectively "three of Google's four golden signals".

## What RED doesn't catch

1. **Saturation.** The fourth of the Google Golden Signals is absent from RED. Wilkie explicitly defers saturation to USE. Missing saturation means a service can be near-saturated and its RED metrics look fine *until* it falls off the cliff. For this reason, Weaveworks' dashboards often include per-service queue-depth metrics alongside RED.
2. **Sub-service decomposition.** RED is per-service. If service A makes 5 internal dispatches each with their own rate/errors/duration, RED at service A only sees the aggregate. Sub-operation breakdown requires additional instrumentation.
3. **Batch jobs and asynchronous processing.** RED assumes request/response. For a batch job, "rate of requests" and "duration of requests" are less meaningful; pipelines use different metrics (throughput, lag, queue depth).

## Implementation

RED metrics are trivial to instrument: a middleware that counts requests (Counter for `service_requests_total`), counts errors (labelled Counter), and records durations (Histogram for `service_request_duration_seconds`). In the Prometheus ecosystem this is four lines of Go or Python:

```
var (
    requests = prometheus.NewCounterVec(... "service_requests_total" ...)
    errors   = prometheus.NewCounterVec(... "service_errors_total" ...)
    duration = prometheus.NewHistogramVec(... "service_request_duration_seconds" ...)
)

func middleware(next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        start := time.Now()
        rr := &respWriter{w, 200}
        requests.With(...).Inc()
        next.ServeHTTP(rr, r)
        duration.With(...).Observe(time.Since(start).Seconds())
        if rr.status >= 500 {
            errors.With(...).Inc()
        }
    })
}
```

The Prometheus Go client library, OpenTelemetry, and every modern instrumentation library ship a RED-compatible HTTP middleware out of the box. Every microservice in a new codebase can have RED metrics in a day.

## Alerts and dashboards

RED's biggest win is dashboard uniformity. A standard microservice dashboard has three charts (rate, errors, duration) and three alert rules (rate drop, error rate > X, p99 > Y). Every service has the same layout. An on-call engineer can debug *any* service without reading its docs, because the shape of the data is fixed.

Typical alert set:

- Rate is < 10 % of baseline for 5 minutes → page (service is down or partitioned).
- Error rate > 1 % for 2 minutes → page.
- p99 duration > SLO target for 10 minutes → ticket or page depending on severity.
- Duration p50 drift of > 30 % week-over-week → weekly review.

## Relationship to the Four Golden Signals

Google SRE's "Four Golden Signals" (Latency, Traffic, Errors, Saturation; separate note) is the same idea minus one metric. Historically, Google came first (SRE book, 2016) but in practice the two philosophies were developed in parallel from shared origins. A standardised service dashboard should include all four: RED + one saturation metric.

## Anti-patterns

1. **RED without labels.** Counting all requests as "one service" loses the endpoint-specific signal. Use labels for route, method, status class.
2. **Histogram bucket choices too coarse.** Durations in seconds buckets `[0.1, 1, 10]` gives you three non-informative percentiles. Use fine-grained buckets matching your SLO target.
3. **Average duration instead of percentile.** See the percentile note — averages hide tails. Use histogram_quantile, not rate/count.
4. **Alerting on Rate alone.** Rate at normal levels with 100 % errors is not fine. Need to combine.
5. **RED on every internal sub-operation.** Metric cardinality explodes. Instead, apply RED at service boundaries and use tracing for sub-operation visibility.

## Adversarial reading

- The "simplicity" is a feature for microservices (uniform across services) and a bug for systems where each service is internally complex. A monolith has one RED triplet that is too coarse to diagnose anything.
- RED's "duration" is a distribution, not a scalar. Practitioners often alert on `histogram_quantile(0.99, ...)` but compare against fixed thresholds. This loses the distribution information. Alerting on distribution change (KS test, Jensen-Shannon) is more sensitive but harder to operationalise.
- RED as a monitoring philosophy is useful; RED as a *testing* philosophy adds little — a load test already produces rate/errors/duration, so "apply RED to your load test" is trivially true.

## References

- Wilkie, T. — "The RED Method: How to Instrument Your Services" — Grafana Labs blog, 2018-08-02 — [grafana.com/blog/2018/08/02/the-red-method-how-to-instrument-your-services](https://grafana.com/blog/2018/08/02/the-red-method-how-to-instrument-your-services/)
- Wilkie, T. — "The RED Method: Key metrics for microservices architecture" — Weave Online User Group talk, 2015.
- USE Method — `01KNZ4VB6JHP7W47HM7QREWW53`.
- Google SRE Four Golden Signals — `01KNZ4VB6JXAZA2TBRCD5DERK9`.
