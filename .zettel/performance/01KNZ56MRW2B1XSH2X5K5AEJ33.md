---
id: 01KNZ56MRW2B1XSH2X5K5AEJ33
title: Gatling — JVM/Scala load generator with explicit open/closed injection profiles
type: literature
tags: [tool, gatling, load-testing, jvm, scala, java, akka, pekko, netty, open-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ6GW3GYN9ZDT3A9JTVJFEW
    type: related
  - target: 01KNZ6GW8J1VQ2XPF9E1V1BFHQ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.228317+00:00
modified: 2026-04-11T20:54:43.228320+00:00
---

Source: https://docs.gatling.io/reference/ — Gatling reference documentation, fetched 2026-04-12.

Gatling is a load testing tool originally written in Scala, first released in 2012 by Stéphane Landelle (ex-eBusiness Information). Version 3.x supports Java, Kotlin, and Scala DSLs; JS/TS SDKs arrived with Gatling 4 in 2024. The runtime is on the JVM and until Gatling 3.7 was built on Akka; recent versions have migrated to Pekko following the Akka license change. The HTTP client is Netty-based.

## Simulation model

A Gatling test is a `Simulation` class. Inside it you define one or more `ScenarioBuilder`s (user behaviour) and attach them to `injectionProfiles` (workload shape). The simulation is compiled (not interpreted) before execution — test scripts are real Scala/Java/Kotlin that get `javac`'d or `scalac`'d.

Minimal Java example (verbatim from the reference docs structure):

```java
public class BasicSimulation extends Simulation {
  HttpProtocolBuilder httpProtocol =
    http.baseUrl("https://computer-database.gatling.io")
        .acceptHeader("application/json");

  ScenarioBuilder scn = scenario("Scenario")
    .exec(http("request_1").get("/computers"))
    .pause(5);

  {
    setUp(
      scn.injectOpen(
        rampUsers(100).during(10),
        constantUsersPerSec(20).during(60)
      )
    ).protocols(httpProtocol)
     .assertions(global().responseTime().percentile(95).lt(800));
  }
}
```

## Injection profiles

Gatling distinguishes open- and closed-model explicitly via `injectOpen` vs `injectClosed`:

**Open**: `atOnceUsers(n)`, `rampUsers(n).during(d)`, `constantUsersPerSec(rate).during(d)`, `rampUsersPerSec(r1).to(r2).during(d)`, `stressPeakUsers(n).during(d)`, `heavisideUsers(n).during(d)`. These schedule new virtual users to *arrive* at a specified rate.

**Closed**: `constantConcurrentUsers(n).during(d)`, `rampConcurrentUsers(n1).to(n2).during(d)`. These keep a fixed population active regardless of response time.

The explicit open/closed split in the API is a Gatling strength — it forces the test author to think about the workload model.

## Scenarios and chains

Scenarios compose `exec`, `pause`, `feed`, `doIf`, `repeat`, `during`, `asLongAs`, and `randomSwitch` blocks. Session variables (`#{varname}`) carry state across requests. Feeders (CSV, JSON, JDBC, Redis, custom) inject per-user test data. Checks extract data and assert on responses: `check(status().is(200), jsonPath("$.id").saveAs("id"))`.

## Protocol support

HTTP/HTTPS (HTTP/1.1, HTTP/2) is the core. Additional protocols: WebSocket, Server-Sent Events, JMS, MQTT, and gRPC. SQL (JDBC) is supported via feeders but not as a load target in OSS. Gatling Enterprise adds more.

## Assertions and reports

`assertions(...)` at the `setUp` level act as pass/fail gates that drive the exit code for CI. Metrics: response time (min, max, mean, standard deviation, percentiles), KO rate, request rate. The OSS runner generates a static HTML report into `target/gatling/` with Highcharts visualisations (requests over time, response-time distribution, percentile-over-time plots).

## Gatling Recorder

A proxy-based recorder that captures browser traffic and emits a `.scala`/`.java` Simulation. HAR-file import is also supported. This is the on-ramp for "I have a web app, generate a first test for me".

## Gatling Enterprise

Commercial offering (formerly FrontLine). Adds: web UI for test management, distributed execution with injector scaling, real-time dashboards, multi-user collaboration, integration with enterprise SSO, historical trend dashboards, and LDAP/Active Directory auth. OSS remains free but distributed runs require Enterprise.

## Strengths

- Type-safe DSL — refactors are a real compile step; typos catch at build time.
- Explicit open/closed model in the API.
- Genuinely good HTML reports for a free tool.
- Netty under the hood — sustains high RPS on a single injector.

## Failure modes

- JVM cold-start + GC noise — warmup matters. First few thousand requests are slower than steady-state.
- Scala/Java compilation loop is slow relative to k6's "edit and re-run".
- Distributed execution is Enterprise-only in OSS 3.x. The community workaround is to run N injectors in parallel and merge `simulation.log` files manually; this is error-prone.
- Session object is copied per user; large per-user state will OOM at scale.
- Steep learning curve for Scala-unfamiliar teams even with the Java/Kotlin DSLs.

## Relevance to APEX G-46

Like k6, Gatling is a validation-workload tool, not a worst-case-input generator. It sits in the "sustain the load once you know the worst cases" half of the performance practice. Its explicit open/closed injection profiles make it the cleanest reference model for what G-46's own workload-description schema should look like.