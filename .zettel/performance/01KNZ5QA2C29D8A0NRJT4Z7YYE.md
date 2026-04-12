---
id: 01KNZ5QA2C29D8A0NRJT4Z7YYE
title: "Loader.io — SendGrid's free-tier cloud load testing service"
type: literature
tags: [tool, loader-io, saas, load-testing, sendgrid, free-tier, small-scale]
links:
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:03:49.324949+00:00
modified: 2026-04-11T21:03:49.324951+00:00
---

Source: https://loader.io/ — Loader.io product page, fetched 2026-04-12.

Loader.io is a free-tier cloud load testing service operated by SendGrid (now part of Twilio). It is the smallest and simplest entry in the commercial load-testing SaaS landscape — deliberately aimed at small web apps and APIs rather than enterprise performance practice.

## Model

Loader.io provides three test types:
- **Clients per test** — N clients launched over the test duration (closed-model).
- **Clients per second** — rate-based arrival model for N seconds (open-model).
- **Maintain client load** — ramping version of clients-per-test.

Test targets must be registered and verified (via an HTTP token file or DNS TXT record) before testing. This prevents the service being used as a free DDoS tool — a real and interesting constraint that bigger services handle with legal agreements.

## Features

- Web UI test builder.
- REST API for programmatic test creation (documented; enables CI integration).
- Real-time monitoring during runs.
- Shareable, publicly linkable reports with graphs of response times and error rates.
- Multi-step scenarios (GET → POST → …) via the web UI.
- Headers, query params, POST bodies configurable per step.

## Pricing tiers

Free tier: limited concurrent clients and test duration (historically ~10k clients, 60 seconds). Paid tiers raise the limits. Specific pricing is not fixed and has varied over the product's history; Loader.io's positioning has always been "free for most small projects".

## Strengths

- Free tier exists and is not a 30-day trial — it is a real free tier.
- Domain verification blocks abuse, protecting both targets and the service's reputation.
- Zero setup: sign up, verify a domain, run a test in minutes.
- Public shareable reports are a good collaboration mechanism for small teams.

## Failure modes

- **Free tier is too small** for any meaningful enterprise work. p99 confidence intervals on short 60-second runs are noisy.
- **Limited scripting** — no dynamic data, captures, or session state beyond simple chaining.
- **No explicit coordinated-omission handling.**
- **Owned by SendGrid/Twilio** — roadmap velocity depends on Twilio's priorities; the product has had long quiet periods.
- **Not competitive with BlazeMeter / Azure Load Testing / Grafana Cloud k6** for serious work.

## When it is the right choice

- Indie developer checking if a side-project handles 1k concurrent users.
- Small-business website pre-launch smoke test.
- Tutorials and learning — the verification step and web UI make it a decent teaching tool.

## Relevance to APEX G-46

Loader.io is primarily a completeness data point in the SaaS landscape. It represents the "entry level" — what load testing looks like when the audience is non-enterprise developers who want to verify basic capacity. APEX's target audience is much more ambitious, but the verification-token pattern is an interesting bit of anti-abuse design worth noting.