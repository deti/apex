---
id: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
title: Session Mining from RUM and Server Logs — Building Workload Models from Real User Data
type: permanent
tags: [session-mining, rum, real-user-monitoring, access-logs, workload-characterization, user-journey, test-generation, concept]
links:
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ6QBH0YZYKPZNYDCZD5P2B
    type: related
  - target: 01KNZ5ZPSEK679QQYMHXF16WFF
    type: related
  - target: 01KNZ4VB6JHJSARKD8E9XVGVRC
    type: related
  - target: 01KNZ4VB6J56B59YB7SZDKTAKD
    type: related
  - target: 01KNZ68KN59XANY9TX9WE0BYJH
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:08:24.573389+00:00
modified: 2026-04-11T21:08:24.573394+00:00
---

# Session Mining from RUM and Server Logs

## The idea

Real User Monitoring (RUM) products (Datadog, Dynatrace, New Relic, CloudWatch RUM, Sentry) capture every page view, click, XHR, and timing metric from real users' browsers. Server-side access logs (nginx, ELB, application logs) record every request that reached the backend. Between them, you have a complete record of what users actually did.

**Session mining** is the process of grouping these raw events into user sessions and then summarising the sessions into a workload model that a load generator can consume. It is the practical bridge between "we have telemetry" and "we have a realistic load test."

## Data sources and what each offers

### Server-side access logs

- **Granularity:** every HTTP request, timestamped, with method, path, status, size, duration, referer, user-agent.
- **Coverage:** 100% of requests that reach the server.
- **Limits:** no client-side metrics (page-load time, layout stability, user interactions between requests). User identity is only what the server can see (IP, auth token).
- **Session reconstruction:** group by client IP + user-agent + time-window. Imperfect — shared NATs make one "session" a mixture of multiple users.
- **Availability:** essentially universal. Every production service has these.

### RUM telemetry

- **Granularity:** page views, custom events, JavaScript errors, browser timings, network waterfall, user interactions (clicks, scrolls, form submits).
- **Coverage:** a sample (RUM SDKs are sampled by default, typically 1–10%) of real users' sessions.
- **Limits:** RUM is client-side, so it misses what the backend did — it sees XHR calls as "network requests took T ms" without the internal spans. Session grouping is done by the SDK with explicit session IDs, so it's accurate.
- **Availability:** requires a RUM product (paid) and an SDK integration.

### Distributed traces

- **Granularity:** per-request span tree across services, with optional user ID / tenant tags.
- **Coverage:** sampled, typically 1% head-based or tail-based.
- **Limits:** bodies are not captured. Session reconstruction requires joining traces by user ID across time, which most tracing UIs don't support well.

## The session-mining pipeline

1. **Raw event stream in.** Pull from access logs (via logstash/fluentd to S3 or Kafka), RUM APIs (Datadog Session Replay API), or trace backends (Tempo/Jaeger query).
2. **Session boundary detection.** Group events by user. Use explicit session IDs if present; otherwise time-based (30 min of inactivity = new session).
3. **Canonical URL grouping.** Reduce URLs to endpoint patterns. `/orders/42` and `/orders/43` both map to `/orders/:id`. This is critical for the model not to have thousands of one-hit states. Typically uses a regex or a learned path tokeniser.
4. **Feature extraction per session.** Number of requests, duration, set of endpoints, status distribution, exit endpoint.
5. **Clustering.** Sessions → clusters (k-means, HDBSCAN). Each cluster is a "user type" — guest, registered-browsing, registered-purchasing, API-integration.
6. **Model fitting per cluster.** A Markov chain (CBMG-style) for navigation, plus marginal distributions for think time between requests and for payload-size-if-observed.
7. **Arrival-rate fitting.** Cross-cluster, fit the per-minute rate of new session starts by cluster. Usually time-of-day and day-of-week seasonality is the dominant signal.
8. **Emit workload spec.** Canonical JSON that describes clusters, CBMGs, think-time distributions, arrival rates.
9. **Render to a load-test tool.** k6 scenarios, Gatling simulations, JMeter thread groups. This is the last mile.

Step 9 is where every published tool falls short. Everything from 1 to 8 is doable with off-the-shelf data-science tools (pandas, scikit-learn, networkx). Step 9 is a tool-chain integration problem that nobody has solved in open source. Teams that do this end up writing their own renderer.

## What teams actually do today

Most teams don't do proper session mining. They do one of:

- **Peak-hour RPS.** Count requests per second at peak, double it, run the load test at that rate. Coarse but common.
- **Top-N endpoints.** Take the top 10 endpoints by volume, weight the load test proportionally. Better than peak-RPS but still no session structure.
- **One-off analysis per incident.** When an incident happens, do a manual analysis of the traffic that triggered it and write a specific test. Not reusable.

The full Menascé-style methodology is rare in practice because the tooling gap is real.

## Failure modes of session mining

1. **Session boundary errors.** Time-based grouping is wrong for slow users; explicit-ID-based grouping is wrong for users without cookies. Both make the fitted model biased.
2. **URL canonicalisation is non-trivial.** Getting from `/orders/42` to `/orders/:id` automatically requires either a well-defined API spec or a learned tokeniser; the tokeniser sometimes mis-groups `/users/42` with `/orders/42`.
3. **Clustering instability.** Re-running the pipeline on different time windows gives different clusters because of seasonal behaviour. Cluster labels drift, breaking downstream tooling that expects stable cluster IDs.
4. **Heavy tail in session length.** The log-normal-like distribution of session lengths has a long tail (a few users with 10,000 requests). The mean is dominated by the tail and is not representative. Load tests built from mean session length massively underestimate concurrent users.
5. **PII in access logs.** User IDs, emails, and tokens often appear in URLs and headers. Any mining pipeline has to strip these, and naive striping missing fields is a regular compliance incident.
6. **Selection bias from sampling.** Tail-based sampling (common in tracing) over-represents slow sessions. Sampling based on user ID hash is better for workload modelling.
7. **Synthetic users.** Access logs include monitoring bots, health checks, and CI requests. Without filtering, these distort the cluster structure massively.

## The toolmaker gap (ranked by value)

- **End-to-end session-mining → load-test generator.** Highest value, nobody has built it. An open-source pipeline that takes an S3 bucket of access logs and produces a runnable k6 script with CBMG-driven user types would be instantly adopted. The pieces exist; the integration does not.
- **Canonical URL grouping library.** Small but repeatedly rewritten. A library that takes raw URLs plus an optional OpenAPI spec and returns canonical patterns would save engineering time across every team.
- **Workload drift detector.** Compare the current workload model against a historical baseline, emit alerts. Requires only the pipeline above + a diff step.
- **LLM-assisted cluster labelling.** After clustering, prompt an LLM with representative sessions from each cluster to propose human-readable labels ("registered buyer," "window shopper") which would make the output much more actionable.

## Citations

- Datadog RUM docs: https://docs.datadoghq.com/real_user_monitoring/
- CloudWatch RUM: https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/CloudWatch-RUM.html
- Coralogix RUM guide: https://coralogix.com/guides/real-user-monitoring/
- Session replay background: https://docs.datadoghq.com/real_user_monitoring/ (session replay section)
- Menascé "Scaling for E-Business" (CBMG methodology): https://cs.gmu.edu/~menasce/ebook/toc.html
- Variable-length Markov chains for navigation: https://ieeexplore.ieee.org/document/4118703/
- E-commerce workload characterisation: https://cs.brown.edu/~rfonseca/pubs/menasce99e-com-char.pdf