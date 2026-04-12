---
id: 01KNZ5QA2X25PPVG6QHT8KNP4D
title: "NeoLoad — Tricentis's enterprise load tester (SAP, Oracle Forms, Citrix support)"
type: literature
tags: [tool, neoload, saas, commercial, tricentis, enterprise, sap, loadrunner-competitor]
links:
  - target: 01KNZ5SMHEAYVDMJWJ9NAZBPCD
    type: related
  - target: 01KNZ56MS27NFFMFBHZ0WPCV70
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:03:49.341368+00:00
modified: 2026-04-11T21:03:49.341370+00:00
---

Source: https://www.tricentis.com/products/performance-testing-neoload — Tricentis product page, fetched 2026-04-12.

NeoLoad is an enterprise-focused performance testing platform from Tricentis (which acquired it via the Neotys acquisition in 2021). It is one of the two canonical enterprise performance-testing incumbents alongside LoadRunner; the pitch is "LoadRunner's capabilities without the pricing model".

## Scripting model

NeoLoad supports two design modes:
- **No-code / Design Studio** — a GUI with browser-based recording, visual scenario composition, no programming required. This is the enterprise-QA default.
- **As-code YAML** — NeoLoad's scripts can be committed to git as YAML + JavaScript. Recent versions push this path heavily in response to developer-centric competitors like k6.

Scripts combine protocol recording (HTTP capture via proxy) with JavaScript customisation hooks for dynamic data, correlations, and assertions.

## Protocol breadth

NeoLoad's protocol support is the richest in the commercial space:
- HTTP/HTTPS, HTTP/2, HTTP/3
- WebSocket, Server-Sent Events
- **SAP GUI, SAP Fiori, SAP RFC, SAP IDoc**
- **Oracle Forms**
- **Citrix ICA**
- **Remote Terminal Emulation (RTE)** for mainframe green-screen apps
- JMS, MQTT, gRPC
- **Adobe Flex/AMF** (legacy)

SAP and Oracle Forms are the differentiators: these are enterprise workloads that OSS tools cannot script. Any org with a large SAP performance testing practice is either on NeoLoad or LoadRunner.

## RealBrowser

NeoLoad's real-browser testing module. Like LoadView's EveryStep and k6 browser, it runs actual browser instances as virtual users capturing DOM-level metrics. RealBrowser is positioned for "validate that performance regressions visible to users are captured".

## APM integration

First-party integrations with **New Relic, Datadog, Dynatrace, AppDynamics, Prometheus**. During a NeoLoad test, metrics from the APM correlate with load-test timelines in the NeoLoad dashboard — same architectural idea as Azure Load Testing's Azure Monitor integration.

## CI/CD and governance

CI integrations: Jenkins, Azure DevOps, GitHub Actions, GitLab. SLO gates via fail criteria. Results are stored in NeoLoad Web (the SaaS management layer) for historical trending, team dashboards, governance reporting. Enterprise features: LDAP/SAML SSO, role-based access, audit logs.

## AI-powered analysis

Tricentis markets "AI-powered performance analysis" — automated bottleneck identification, correlation between client and server metrics, suggested root cause. Detail on the underlying techniques is thin in public documentation.

## Strengths

- SAP, Oracle Forms, Citrix, RTE support — unique in the market.
- Real-browser + APM + protocol load in one tool.
- Enterprise governance features (SSO, RBAC, audit).
- Long-established enterprise support practice; vendor responds to support tickets.

## Failure modes

- **Proprietary pricing** — NeoLoad is expensive and historically required contacting sales. Per-user and per-VU models vary by quote.
- **Vendor lock-in** — scripts do not port to other tools.
- **Ecosystem is closed** compared to JMeter/k6. Plugin catalogue is small and vendor-gated.
- **As-code path is newer than the GUI path**; feature parity between the two is ongoing work.
- **Learning curve** for the full feature set; the no-code UI is simpler but complex tests end up in JS anyway.

## Relevance to APEX G-46

NeoLoad occupies the high end of the enterprise perf-testing landscape. For APEX, the interesting integration point is the "reproduction script" question: if APEX G-46 discovers a resource-exhaustion attack against a SAP endpoint, NeoLoad is realistically the only tool in the ecosystem capable of reproducing the attack at sustained load. A YAML-format export path from APEX to NeoLoad as-code would cover the enterprise-SAP use case that neither k6, Gatling, JMeter, nor BlazeMeter address.