---
id: 01KNZ5F8S3YZFEGJX006A5WRA5
title: "Tsung — Erlang distributed multi-protocol load generator (XMPP, AMQP, HTTP)"
type: literature
tags: [tool, tsung, load-testing, erlang, xmpp, amqp, distributed, open-model, historical]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.859948+00:00
modified: 2026-04-11T20:59:25.859953+00:00
---

Source: https://en.wikipedia.org/wiki/Tsung — Wikipedia entry (direct docs unreachable: tsung.erlang-projects.org returns a TLS cert mismatch). Fetched 2026-04-12.

Tsung is a distributed multi-protocol load testing tool written in Erlang. Created by Nicolas Niclausse at IDEALX (later eBusiness Information), first release 2001 — it is one of the oldest OSS load generators still in production use. Version 1.8.0 released March 2023. GPL-2.0. Source at github.com/processone/tsung.

## Why Erlang

Erlang/OTP's actor model was purpose-built for concurrent I/O-heavy workloads — exactly the problem load generators solve. A Tsung session is an Erlang process; a single Beam VM can host hundreds of thousands of concurrent sessions with microscopic per-session overhead (no OS threads, no thread-stack memory, preemptive scheduling via reductions). This is the same architectural bet k6 makes with Go goroutines, two decades earlier.

## Test plan: XML

Tsung tests are described in an XML document:

```xml
<tsung loglevel="warning">
  <clients>
    <client host="localhost" maxusers="10000"/>
  </clients>
  <servers>
    <server host="example.com" port="80" type="tcp"/>
  </servers>
  <load>
    <arrivalphase phase="1" duration="10" unit="minute">
      <users arrivalrate="20" unit="second"/>
    </arrivalphase>
  </load>
  <sessions>
    <session name="http" probability="100" type="ts_http">
      <request> <http url="/" method="GET"/> </request>
      <thinktime value="2"/>
      <request> <http url="/about" method="GET"/> </request>
    </session>
  </sessions>
</tsung>
```

`arrivalphase` with `arrivalrate` is open-model by construction. This was a notable choice for 2001 — most of Tsung's contemporaries were closed-loop.

## Protocol breadth

Tsung's killer feature historically was supporting protocols nobody else touched: HTTP, WebDAV, XMPP/Jabber (Tsung was *the* XMPP load generator for over a decade and remains the default choice for large XMPP deployments), WebSocket, LDAP, MySQL, PostgreSQL, SOAP, AMQP, MQTT. It is still used for XMPP and AMQP load testing specifically because the alternatives are thin.

## Distributed execution

Tsung is distributed by default. The master runs on one machine and spawns Erlang node slaves over SSH onto the `<client>` hosts listed in the XML plan. This is leveraging Erlang distribution primitives, which Just Work across a LAN. Unlike JMeter's RMI, Erlang distribution is not brittle.

## Reports

A Tsung run produces a directory of log files and a stats dashboard generated via Perl scripts (`tsung_stats.pl`). The reports are ugly by modern standards — Gnuplot-era — but correct: latency quantiles, throughput, error rates, per-request-type breakdowns.

## Strengths

- Erlang distribution gives near-free multi-host scale.
- XMPP and AMQP support.
- Open-model arrival rate from day one.
- Runs forever on a small machine (the Beam VM memory footprint is tiny).

## Failure modes

- **XML-in-2026** — test maintenance is painful. There is no DSL, no type safety, no autocomplete.
- **Reports look like 2005** — BlazeMeter's slides about dashboards resonate for a reason.
- **Erlang barrier** — debugging a Tsung run requires reading Erlang stack traces.
- **Smaller community than JMeter/Gatling/k6** — questions are rarely answered on Stack Overflow.
- **No HTTP/2 without hacks** — HTTP support is HTTP/1.1 era.

## When Tsung is still the right answer

- XMPP and AMQP load tests where you need 100k+ concurrent long-lived sessions.
- Any scenario where an Erlang shop already runs Tsung.
- Educational value — Tsung's source is a compact demonstration of the Erlang actor approach to load generation.

## Relevance to APEX G-46

Tsung is a historical anchor in the G-46 competitive landscape — the proof that the "open-model, high concurrency, multi-protocol" design was achievable decades before the current generation of tools re-discovered it. APEX's workload schema should be expressible as a Tsung XML plan for users who want to reproduce a discovered worst-case input against XMPP or AMQP services.