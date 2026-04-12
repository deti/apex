---
id: 01KNZ6QBH0YZYKPZNYDCZD5P2B
title: Menascé & Almeida — Capacity Planning for Web Services (Book)
type: literature
tags: [menasce, almeida, book, capacity-planning, workload-characterization, performance-modeling, textbook]
links:
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6J6R3V3GVBWSAKW8JC
    type: related
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:21:19.392451+00:00
modified: 2026-04-11T21:21:19.392457+00:00
source: "https://cs.gmu.edu/~menasce/webservices/toc.html"
---

# Capacity Planning for Web Services: Metrics, Models, and Methods (Menascé & Almeida, 2001/2002)

*Daniel A. Menascé (George Mason University) and Virgilio A. F. Almeida. Prentice Hall, 2001. With its predecessor Scaling for E-Business (2000) and the more fundamental-theory Performance by Design (2004), this is the canonical textbook tradition for server-side capacity planning and workload characterisation. The methodology in this book is what every serious perf engineer should be building against and what almost no modern tool actually implements.*

## Why this book and not one of the newer ones

The book is old (2001, reprinted 2002) and addresses pre-cloud web services. But the methodology is not about specific tools — it's about the *discipline* of turning observed traffic into predictive models that can be used both for capacity planning and for realistic test generation. That discipline is exactly what the 2024 tooling space mostly ignores.

Almost every modern load-test tool (k6, Gatling, JMeter, Artillery, Locust, ghz) lets you specify "X virtual users" or "Y requests per second." None gives you first-class primitives for the steps Menascé's methodology actually requires:

1. Characterising the workload from observed data.
2. Fitting a model to the observations.
3. Validating the model against the data.
4. Forecasting future load.
5. Comparing model predictions to measured system capacity.

## Chapter structure and what to mine

### Chapter 5 — Planning the Capacity of Web Services

Lays out the full methodology as a recipe:

1. Understanding the environment.
2. Workload characterisation.
3. Workload model validation and calibration.
4. Performance and availability model development.
5. Model validation and calibration.
6. Workload forecasting.
7. Performance and availability prediction.
8. Cost model development.
9. Cost prediction.
10. Cost/performance/availability analysis.

This 10-step process is the scaffolding of a grown-up perf practice. No tool today automates more than 2–3 of the steps. A tool that covered all 10 — even partially — would be an entirely new category of product.

### Chapter 6 — Understanding and Characterizing the Workload

The chapter every test-generation researcher should read. Teaches the discipline of:

- **Specification of standpoint.** Whose perspective is the workload measured from? (Client? Server? Intermediate tier?)
- **Parameter selection.** Which dimensions of the raw data matter? (Requests per second is almost always less useful than session duration distribution or per-endpoint mix.)
- **Monitoring.** How to collect raw data without distorting what you're measuring.
- **Data analysis.** Reducing raw logs into manageable distributions.
- **Model construction.** The actual fit step — clustering into representative populations, fitting distributions per population, parameterising the CBMG.

This is where the Customer Behavior Model Graph (CBMG) is introduced as the natural state-space model for a workload.

### Chapter 12 — Workload Forecasting

Covers strategies for projecting how the workload will evolve: exponential smoothing, moving averages, non-linear fits. The "how much load will we see next quarter" question. Directly applicable to sizing tests for future capacity, not just current.

## Menascé's other books (worth knowing)

- **Scaling for E-Business** (2000). The CBMG appears first here with e-commerce examples. Table of contents is at https://cs.gmu.edu/~menasce/ebook/toc.html.
- **Capacity Planning for Web Performance** (earlier). A more introductory version.
- **Performance by Design** (2004, with Almeida and Dowdy). The deepest treatment of the analytical models — queueing networks, operational laws (Little's Law, Utilization Law, Response Time Law), mean-value analysis. Less practical, more theoretical, and where you get the mathematical tools.

## Core ideas to carry into perf test generation

1. **Workloads are distributions, not rates.** A test spec that says "1000 RPS" hides all the interesting structure. The real spec is "a mixture of N user classes, each with their own CBMG and inter-arrival distribution."
2. **Workload characterisation is a data-science step.** It belongs in the test-generation pipeline as a first-class phase, not an afterthought.
3. **Validation is as important as fitting.** A load test whose workload model has not been validated against production data is generating untrustworthy results. Fit without validation is no better than guessing.
4. **Forecasting is part of capacity planning.** Tests should not only match current load — they should test projected future load. Nobody's tools let you say "run today's test at next quarter's projected mix."
5. **Little's Law is the atomic constraint.** Throughput × Average Response Time = Mean Number In System. This algebraic identity connects the three variables that every load test measures. Any test result that violates it is wrong. The LLM-generated tests that forget this produce infeasible configurations.

## Why modern tools don't implement this

My tentative explanation: the methodology is heavyweight, and the industry shifted in the mid-2010s toward "move fast" testing philosophies (Chaos Monkey, continuous deployment, canary analysis) that treat production as the real test environment. The Menascé approach feels "academic" next to "just ship it and monitor." But both are needed. Chaos engineering without workload characterisation finds the easy bugs but misses the long-tail capacity issues that Menascé's methodology catches.

A modern, tool-backed revival of Menascé's methodology — powered by distributed tracing (which gives you the per-operation data he needed), LLM-assisted parameter selection, and continuous integration — is the highest-leverage research gap in this whole lane.

## Citations

- Book page (GMU): https://cs.gmu.edu/~menasce/webservices/toc.html
- Scaling for E-Business page: https://cs.gmu.edu/~menasce/ebook/toc.html
- Earlier book page: https://cs.gmu.edu/~menasce/webbook/
- E-commerce workload characterization methodology paper: https://cs.brown.edu/~rfonseca/pubs/menasce99e-com-char.pdf
- Fractal characterization of web workloads: https://cs.gmu.edu/~menasce/papers/web-engineering.pdf
- Amazon (for reference): https://www.amazon.com/Capacity-Planning-Web-Services-Metrics/dp/0130659037