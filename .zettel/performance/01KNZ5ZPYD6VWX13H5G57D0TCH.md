---
id: 01KNZ5ZPYD6VWX13H5G57D0TCH
title: ShadowReader (Edmunds) — Serverless Load Test Replay from Production Logs
type: literature
tags: [shadowreader, edmunds, load-testing, log-replay, aws-lambda, serverless, production-traffic, test-generation]
links:
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:08:24.653734+00:00
modified: 2026-04-11T21:08:24.653740+00:00
source: "https://github.com/edmunds/shadowreader"
---

# ShadowReader — Serverless Load Testing by Replaying Production Logs

ShadowReader is an open-source tool from Edmunds.com that replays website traffic by reading production access logs and issuing equivalent HTTP requests from AWS Lambda. Repository: github.com/edmunds/shadowreader. Announced on opensource.com in 2019.

## Approach and differentiator

All the replay tools that preceded ShadowReader (tcpreplay, GoReplay, Envoy tap, VPC mirror) capture live traffic at the network or proxy level. ShadowReader's insight is that you don't need to be in the network path — **access logs are already a record of the request stream.** If your production service writes access logs (it does), you can parse the logs and reconstruct requests from them.

This has several practical advantages:

1. **No capture agent.** Zero touch on production hosts. You only need log pipeline access, which every organisation already has.
2. **Arbitrary historical replay.** You can replay traffic from any time period — last hour, last week, last Black Friday. Live-capture tools can only replay what was captured at the time.
3. **Decoupled time and space.** The replay is serverless (Lambda) and can be parallelised across thousands of workers for arbitrarily high rate.
4. **Natural amplification.** Replay at N× the original rate by running more Lambdas. Since log-parsed requests are distinct entries in the file, amplification doesn't duplicate the exact same cache-hit keys — it replays real variety.
5. **PII handling is upstream.** Access-log PII scrubbing is a mature problem solved by log pipelines (Datadog, Splunk, internal scrubbers). ShadowReader inherits whatever scrubbing is applied before the logs land in S3.

## Architecture

From the repo and the opensource.com article:

- **Log ingestion.** Production access logs are shipped to S3 via Kinesis Firehose.
- **Parser.** A Lambda reads the S3 log files, parses each line into a request object (method, path, headers minus the ones that don't survive replay).
- **Orchestrator.** A stepwise state machine dispatches worker Lambdas, each of which issues a batch of requests against the target.
- **Target.** Usually a staging copy of the production service.
- **Metrics.** CloudWatch collects request counts, latencies, error rates.

The tool is opinionated on AWS Lambda but the approach generalises — you could implement the same pipeline with Kubernetes Jobs or Nomad.

## Limitations

1. **Only GETs cleanly.** Access logs typically don't include request bodies, so POST/PUT requests can be replayed only at the URL level without meaningful payloads. For a pure read-heavy service this is fine; for a transactional service it misses most of what matters.
2. **Headers are reconstructed, not replayed.** The log has the User-Agent and maybe Referer; it does not have Authorization or cookies. Replays hit the target without auth and typically get a 401 storm unless the test target accepts unauthenticated GETs.
3. **Session state is lost.** There's no way to know from logs alone which requests came from which user; session replay in the Menascé sense is not possible unless the user ID is in the URL or a custom logged field.
4. **Schema drift.** Same as every replay tool. Replaying a week-old log against current code can hit deprecated endpoints.
5. **Cost.** Lambda invocations cost money. A 24-hour replay at production rate is not cheap.
6. **Read-only by construction.** The tool explicitly targets read-heavy workloads. Edmunds (a car-shopping site) has a lot of browse-heavy traffic that fits this well.

## Why it matters

ShadowReader is the only published tool I've found that **builds a load test from production access logs as a first-class workflow**. Every other tool in this space either does live capture or takes a HAR file. The log-as-workload insight is correct and underused.

The limitations (GET-only, no auth, no bodies) are severe enough that ShadowReader hasn't become widely adopted — the repo has modest activity and the opensource.com article is the main piece of documentation. But the **architecture template** is sound and any serious load-test-from-logs tool will rediscover it.

## What a modern successor would do

- Accept structured logs with optional header/body fields (modern application logs include Authorization where permissioned, request body hashes, trace IDs).
- Integrate with distributed trace data to fill in the internal span structure and reconstruct session boundaries.
- Use an LLM to propose auth refresh strategies based on observed login-related requests in the logs.
- Emit a canonical workload spec (see session-mining note) and render to the target load tool of choice, not specifically Lambda.

## Citations

- https://github.com/edmunds/shadowreader
- https://opensource.com/article/19/3/shadowreader-serverless
- Speedscale's 2026 replay guide discusses this class: https://speedscale.com/blog/definitive-guide-to-traffic-replay/