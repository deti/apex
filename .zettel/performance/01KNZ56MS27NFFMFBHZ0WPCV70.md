---
id: 01KNZ56MS27NFFMFBHZ0WPCV70
title: "Apache JMeter — Java load generator, enterprise incumbent, XML test plans"
type: literature
tags: [tool, jmeter, load-testing, java, apache, enterprise, closed-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ6GW3GYN9ZDT3A9JTVJFEW
    type: related
  - target: 01KNZ5QA1WE3N05354FR7S4FS7
    type: related
  - target: 01KNZ5QA0QMKGRG7ED5FK893NJ
    type: related
  - target: 01KNZ706EPCBY2YCY9VDDG2WMT
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.234879+00:00
modified: 2026-04-11T20:54:43.234881+00:00
---

Source: https://jmeter.apache.org/usermanual/get-started.html — Apache JMeter user manual, fetched 2026-04-12.

Apache JMeter is the oldest continuously-maintained open-source load generator still in wide use. First released in 1998 by Stefano Mazzocchi at the Apache Software Foundation, it is a 100% pure Java application that predates most of the modern load-testing vocabulary ("virtual users", "scenarios", "arrival rate"). Version 5.6 remains actively maintained.

## Architecture

A JMeter test plan is an XML document (`.jmx`) that describes a tree of nodes: a root test plan, thread groups, samplers, logic controllers, assertions, listeners, config elements, pre/post processors, and timers. The runtime walks this tree and executes samplers under the thread group's concurrency model.

JMeter runs in two modes. The GUI mode (`jmeter`) uses Swing and is for building, debugging, and small-scale interactive use. The non-GUI / CLI mode (`jmeter -n -t plan.jmx -l results.jtl`) is the only supported mode for actual load generation. The user manual is explicit: *"GUI mode should only be used for creating the test script, CLI mode (NON GUI) must be used for load testing."* The Swing UI's own overhead corrupts measurements at meaningful load.

## Thread groups

JMeter's load model is thread-based and closed by default: a thread group has N threads, a ramp-up time, and a loop count. Each thread executes the samplers sequentially, in a loop. The "concurrent user count" is literally the OS thread count, which is why JMeter is memory-hungry (thread stacks) and why practical single-host limits are typically 1k–5k threads before the JVM falls over.

Variants: **Ultimate Thread Group**, **Stepping Thread Group**, **Concurrency Thread Group**, and **Arrivals Thread Group** (all from the JMeter Plugins project) provide closed-model ramping and, in the case of Arrivals Thread Group, open-model rate-based injection. The built-in OSS Thread Group does not natively model arrival rate.

## Samplers

Samplers are protocol adapters. Built-in: HTTP/HTTPS, JDBC, JMS (point-to-point and pub-sub), FTP, SMTP, LDAP, TCP, OS process. Via plugins: MQTT, AMQP, gRPC, Kafka, Cassandra, MongoDB, WebSocket. The HTTP sampler is the most-used and has both Java and Apache HttpClient implementations with HTTP/2 added in 5.6.

## Logic controllers, assertions, timers

- **Logic controllers**: Loop, If, While, ForEach, Transaction, Throughput, Runtime, Switch, Random, Module Controller. Compose multi-step flows.
- **Assertions**: Response Assertion, JSON Assertion, XPath Assertion, Duration Assertion, Size Assertion, JSR223 Assertion (Groovy). These mark samples pass/fail.
- **Timers**: Constant Timer, Gaussian Random Timer, Uniform Random Timer, Poisson Random Timer, Constant Throughput Timer, Precise Throughput Timer. The Precise Throughput Timer is JMeter's closest thing to a k6 `constant-arrival-rate` executor — it implements open-model timing on top of the closed-model thread group by gating requests across threads.

## Distributed mode

JMeter ships a master/slave RMI protocol. Slaves run `jmeter-server`, master connects to them via `-R host1,host2,...`. The master aggregates results. The protocol is old, uses Java serialization, and is notoriously brittle across firewall-NAT boundaries. Modern practice is to run independent non-GUI JMeter processes and merge `.jtl` result files post-hoc (often via BlazeMeter or Taurus).

## Reporting

Listeners capture samples into `.jtl` (CSV or XML). The built-in "Generate Summary Results" and "View Results Tree" listeners are debug tools, not load-test reports — running them under load causes OOM. The supported approach is `jmeter -g results.jtl -o report-dir`, which emits a static HTML report (dashboards, percentiles over time, throughput over time) using the non-GUI reporter introduced in 3.0.

## Plugins ecosystem

JMeter Plugins (https://jmeter-plugins.org) is the de facto plugin repository, providing additional samplers, listeners, throughput shapers, and visualisation. InfluxDB Backend Listener is the usual live-dashboard route into Grafana.

## Why JMeter is still used

Enterprise inertia is real: every big-bank/big-telco performance team has 10+ years of `.jmx` files, a JMeter skill base, and CI jobs that consume `.jtl` output. BlazeMeter is effectively "JMeter-as-a-service" and is widely licensed. Azure Load Testing accepts JMeter scripts natively. Taurus (an Open Source YAML wrapper around JMeter/Gatling/k6/Locust) treats JMeter as a first-class backend. The install base is enormous.

## Failure modes

- **GUI mode used for load generation** — the number one rookie mistake. Measurements are invalid.
- **Listeners enabled under load** — View Results Tree will OOM at any interesting load.
- **Closed-model thread group misread as open-model** — ramp-up and thread-count do not specify request rate; response time does. Results under-report the tail.
- **Heap tuning** — default JVM heap is too small; `-Xmx` tuning is required.
- **RMI distributed mode flakiness** — if the master can't talk back to the slaves (typical in cloud/NAT), the test silently loses samples.
- **JSR223 Groovy in pre/post processors** that recompiles per-sample rather than using cached scripts (`groovy` compiled language checkbox) — huge CPU overhead.

## Strengths

- Protocol breadth: JDBC, JMS, LDAP, FTP, SMTP out of the box is unmatched.
- Mature reporting via the non-GUI HTML reporter.
- Enormous plugin ecosystem and community knowledge base.
- Accepted by every enterprise testing platform.

## Relevance to APEX G-46

JMeter is the reference "traditional load testing" tool cited in G-46's competitive landscape. APEX does not attempt to replace JMeter for soak/endurance/stress workload validation. The interesting integration point is consuming `.jtl` output as a baseline for APEX's regression-detection comparison phase.