---
id: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
title: Locust — Python/gevent load generator with User classes and custom load shapes
type: literature
tags: [tool, locust, load-testing, python, gevent, distributed, closed-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ706C8534Q8VXTRTE1F6TB
    type: related
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ5F8TK2AR78RGESX54MZKQ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.241294+00:00
modified: 2026-04-11T20:54:43.241295+00:00
---

Source: https://docs.locust.io/en/stable/ (WebFetch blocked; content reconstructed from training knowledge and cross-referenced via Azure Load Testing documentation, fetched 2026-04-12).

Locust is an open-source Python-based load testing tool first released in 2011 by Jonatan Heyman. It is developer-oriented ("scriptable in real code, not XML") and has been one of the two canonical open-source load generators alongside JMeter for most of the 2010s. Version 2.x is the current line.

## Concurrency model

Locust uses `gevent` — cooperative greenlets scheduled on a single OS thread per process. This is what makes a single Locust worker comfortably push tens of thousands of concurrent simulated users without thread-stack overhead. The cost is that any blocking I/O that is not monkey-patched by gevent (native extensions, some gRPC clients) will pin the whole worker. `from gevent import monkey; monkey.patch_all()` is implicit at startup.

## User classes and tasks

The unit of load is a `User` subclass. Tasks are methods decorated with `@task` (optionally weighted) or grouped into a `TaskSet`. `HttpUser` provides an HTTP client (`self.client`, a `requests.Session` subclass) with integrated metrics collection.

```python
from locust import HttpUser, task, between

class WebsiteUser(HttpUser):
    wait_time = between(1, 5)

    def on_start(self):
        self.client.post("/login", json={"user":"x","pw":"y"})

    @task(3)
    def view_item(self):
        self.client.get("/item/42", name="/item/[id]")

    @task
    def view_cart(self):
        self.client.get("/cart")
```

`wait_time` strategies: `between(min, max)` (uniform), `constant(n)`, `constant_pacing(n)` — the last one is the closest thing to an open-model pacing semantic Locust offers, and it's still per-user rather than a global arrival rate. Locust is fundamentally a closed-model load generator.

## FastHttpUser

`HttpUser` uses `python-requests`, which is ergonomic but overhead-heavy (a single request allocates thousands of Python objects). `FastHttpUser` uses `geventhttpclient`, which is 5–6x faster and the right choice for high-RPS scenarios. The tradeoff is a smaller API surface and some `requests` features are not supported.

## Distributed mode

Locust runs master/worker over ZeroMQ. `locust --master` starts the coordinator; `locust --worker --master-host=...` joins a worker. The master never generates load — all load comes from workers. Scale is mostly "add more workers"; there is no sharding of scenarios, each worker runs all `User` classes. A reasonable distributed setup is the canonical Locust deployment pattern.

## Web UI, headless, and load shapes

The built-in web UI shows live stats and lets you start/stop tests and adjust user counts during a run. Headless mode (`--headless -u 500 -r 10 -t 10m`) is required for CI. **Custom load shapes** are implemented as a `LoadTestShape` subclass whose `tick()` method returns `(user_count, spawn_rate)` tuples — this is how you implement stage-based ramps or Poisson-like arrival patterns in Locust.

## Metrics and reporting

Per-endpoint request count, failure count, median, mean, min, max, and percentiles (50, 66, 75, 80, 90, 95, 98, 99, 99.9, 99.99, 100). Output: stdout table, CSV (`--csv`), HTML (`--html`), and a Prometheus exporter (`locust_exporter`). The built-in reporting is functional but not pretty; most teams pipe to InfluxDB/Prometheus and render in Grafana.

## locust-plugins

Community package `locust-plugins` adds: Postgres/BigQuery result storage, Kafka producer/consumer users, Playwright browser user, websocket user, `constant_total_ips`, and the `RescheduleTaskOnFail` pattern. The ecosystem is smaller than JMeter's but healthy.

## Strengths

- Python is the scripting language — data scientists and ML engineers can actually write load tests.
- gevent gives high per-worker concurrency.
- Real code (not XML or YAML) means real version control diffs.
- Custom load shapes are a clean escape hatch.

## Failure modes

- **Closed-model by default** — Locust measures what it pushes, not what the system was supposed to receive. Hidden coordinated omission.
- **HttpUser is slow** — reach for FastHttpUser if you need >1k RPS per worker.
- **gevent blocking bugs** — a single non-patched blocking call (native extension, `psycopg2` without `psycogreen`) stalls all greenlets in the worker.
- **GIL is not the limit, but CPU is** — Python still hits CPU ceiling well below what k6 or Gatling achieve per machine. Plan on ~3–5x more workers than an equivalent k6 setup.
- **Master is a bottleneck** in very large distributed runs — it aggregates all worker stats over ZeroMQ.

## Relevance to APEX G-46

Locust is the Python-ecosystem entry in the G-46 competitive landscape. It shares all the limitations APEX targets: no input generation, no complexity estimation, no SLO verification beyond what you hand-write. The Python-native model is appealing as a glue layer: APEX could emit Locust `User` classes from discovered worst-case inputs.