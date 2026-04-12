---
id: 01KNZ5F8Q36WVG78WJ525WHDR0
title: "NBomber — .NET/F# load generator with open-model Inject simulation"
type: literature
tags: [tool, nbomber, load-testing, dotnet, csharp, fsharp, open-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.795801+00:00
modified: 2026-04-11T20:59:25.795808+00:00
---

Source: https://nbomber.com/ — NBomber project homepage and docs, fetched 2026-04-12.

NBomber is a .NET load testing framework written in F# that is scriptable from both C# and F#. First released in 2018, it targets the .NET ecosystem (enterprise Windows / ASP.NET Core shops) where JVM-based tools like Gatling are a cultural non-starter.

## Scenario and step model

The unit of work is a `ScenarioProps` built with `Scenario.Create("name", ...)`. A scenario contains an async step function that performs one operation and returns a `Response` object (OK or Fail, with latency and size). NBomber schedules the step function according to the configured load injection strategy.

Minimal C# example:

```csharp
var scenario = Scenario.Create("http_scenario", async ctx =>
{
    var response = await httpClient.GetAsync("https://api.example.com/items");
    return response.IsSuccessStatusCode
        ? Response.Ok(sizeBytes: response.Content.Headers.ContentLength ?? 0)
        : Response.Fail();
})
.WithLoadSimulations(
    Simulation.Inject(rate: 100, interval: TimeSpan.FromSeconds(1),
                      during: TimeSpan.FromMinutes(5))
);

NBomberRunner.RegisterScenarios(scenario).Run();
```

## Load simulations

- `KeepConstant(copies, during)` — closed-model, fixed concurrent step executors.
- `Inject(rate, interval, during)` — **open-model** constant arrival rate. This is the correct default for SLO work.
- `InjectRandom(minRate, maxRate, interval, during)` — open-model with jittered rate.
- `RampingInject(rate, interval, during)` — linear ramp in arrival rate.
- `RampingConstant(copies, during)` — linear ramp in concurrent executors.

NBomber's explicit open/closed distinction is clean — similar to Gatling and better than Locust/JMeter defaults.

## Protocol support

Core is HTTP via `NBomber.Http`. Additional plugins: `NBomber.WebSockets`, `NBomber.Grpc`, `NBomber.Redis`, `NBomber.Sql` (with Dapper), `NBomber.MQTT`. Because step functions are arbitrary C#/F#, any .NET client library can be used — MongoDB, Kafka, Azure Service Bus, RabbitMQ.

## Reports

Built-in report formats: TXT (summary), HTML (static interactive charts), CSV, MD. The HTML report includes latency percentiles, RPS-over-time, and per-step breakdowns. Real-time metrics can be pushed to InfluxDB via `NBomber.Sinks.InfluxDB` and visualised in Grafana.

## Cluster mode

NBomber supports distributed load generation via a coordinator + agents model, where a coordinator orchestrates multiple agent processes. Cluster mode is part of NBomber Enterprise in current licensing (was OSS in earlier versions).

## OSS vs Enterprise

OSS is free for personal / OSS use. Commercial use (Business $99/mo, Enterprise $199/mo as of the source fetch) adds: cluster mode, NBomber Studio (visual test designer), Grafana dashboards, and priority CI/CD integration.

## Strengths

- Native .NET — integrates with the rest of a .NET solution, debugged with Visual Studio.
- Open-model `Inject` is first-class.
- Step model composes well — multi-step flows are ordinary C# async.
- HTML report is decent out of the box.

## Failure modes

- **License shift** — features that were OSS in early 2.x are now Enterprise. Teams that started on free NBomber may find themselves priced out.
- **Async void pitfalls** — typical .NET async bugs (forgotten `await`, deadlocks on sync-over-async) corrupt measurements.
- **Smaller community** than k6/Gatling/JMeter; fewer Stack Overflow answers, fewer plugins.
- **Single-host ceiling** — without cluster mode (now Enterprise), you are running N `dotnet run` processes and merging by hand.

## Relevance to APEX G-46

NBomber is the canonical .NET entry in the load-generator landscape. For APEX, its value is as a target code generator: "given a worst-case HTTP input I discovered, emit an NBomber step function that reproduces it". The explicit open-model `Inject` maps cleanly onto G-46's workload schema.