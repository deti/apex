---
id: 01KNZ706C8534Q8VXTRTE1F6TB
title: Locust — Python-Based Load Testing with User Behaviour Classes
type: literature
tags: [locust, python, load-testing, user-classes, distributed, test-generation]
links:
  - target: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
    type: related
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:26:09.032649+00:00
modified: 2026-04-11T21:26:09.032656+00:00
source: "https://locust.io/"
---

# Locust — Python-Based Load Testing with User Behaviour Classes

Locust is an open-source load-testing tool written in Python, originally from Jonatan Heyman and team. Canonical home at locust.io. Distinguished from k6, JMeter, and Gatling by being Python-first and by having **user behaviour classes** as the central abstraction.

## The user-class model

Instead of scenarios or thread groups, Locust asks you to define a Python class per user type:

```python
from locust import HttpUser, task, between

class ShoppingUser(HttpUser):
    wait_time = between(1, 5)

    @task(3)
    def view_catalog(self):
        self.client.get('/catalog')

    @task(1)
    def view_product(self):
        self.client.get('/products/42')

    @task
    def checkout(self):
        self.client.post('/checkout', json={'items': ['42']})
```

A Locust user runs its tasks at probabilistic weights (the numbers in `@task(N)`) with the declared `wait_time` between task invocations. You spawn M users of this class; each one runs its own sequence independently.

This is structurally closer to a **CBMG (Customer Behavior Model Graph)** than k6 or Gatling's VU-as-script model. Each user class is essentially a probabilistic next-task chooser — a one-step Markov chain where the state is implicit in "which task last ran."

## Strengths for realistic workloads

- **Weighted task selection.** The `@task(N)` decorator gives a built-in probabilistic mix, which is closer to real user behaviour than a fixed script.
- **Multi-class workloads.** You define multiple user classes (e.g., `BrowsingUser`, `CheckoutUser`, `ApiUser`) and Locust runs them in parallel with configurable counts. This is the native way to express a multi-population workload — closer to a CBMG model than any other mainstream tool.
- **Python flexibility.** Any Python library is available. Session state, custom auth, complex data generation, DB-driven test data — all natural to write.
- **SequentialTaskSet.** Locust supports ordered task sequences for flows that must run in order (login → browse → buy).
- **Distributed mode.** Master/worker architecture for scaling across many machines.

## Limits

1. **Python performance ceiling.** Locust's per-user Python overhead means a single worker typically tops out around 1000–5000 RPS. k6 and Gatling, being Go and JVM respectively, push much higher on single nodes. Locust makes up for it with horizontal scaling.
2. **No first-class open-loop executor.** Locust is fundamentally closed-loop (each user waits for response, then thinks, then sends next request). `constant_pacing` helps but does not implement Gil Tene's CO correction rigorously. For accurate tail-latency measurement, Locust is inferior to wrk2 / k6 ramping-arrival-rate.
3. **Weighted task selection is memoryless.** It's a one-step Markov chain, not a full CBMG. You can't say "after viewing 3 products, 80% go to cart." Workarounds exist (manual state tracking) but are awkward.
4. **Learning curve is Python.** Teams on non-Python backends may find it culturally out of step.
5. **Weaker out-of-the-box analytics.** k6 and Gatling produce prettier reports. Locust's built-in UI is functional but basic.

## Why Locust matters for test generation

Among the mainstream tools, Locust's user-class abstraction is the closest thing to a native workload model. If you're generating a load test from a CBMG or a session-mining pipeline, emitting Locust user classes is arguably the cleanest mapping — one class per cluster, per-task weights derived from observed transition probabilities.

This insight doesn't appear anywhere I've seen in the literature. It seems like a concrete, implementable path for a session-mining → load test generator: use Locust as the target runtime *because* its abstraction matches workload-modelling vocabulary better than k6's VU model does.

## Failure modes of the Locust user-class approach

- **Closed-loop measurement error.** Covered above.
- **No per-class arrival rate.** You control user count per class, not arrival rate per class. To hit a specific RPS for a given class you need to back-compute the user count from Little's Law.
- **Weight drift.** The per-task weights are hand-maintained. If production traffic shifts, the weights drift — workload drift embodied.

## Citations

- https://locust.io/
- https://github.com/locustio/locust
- Docs: https://docs.locust.io/
- k6 vs Locust comparison posts (numerous; search is fine)