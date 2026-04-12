---
id: 01KNZ5F52X5746A9ASY0W6DKDS
title: Envoy tap filter and request mirroring — Service-Mesh Shadow Traffic
type: literature
tags: [envoy, service-mesh, tap-filter, request-mirroring, shadow-traffic, istio, test-generation]
links:
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F557YM1X8Q2ZBXZEBXRM
    type: related
  - target: 01KNZ6WJ0CFDHHQSA1951PSSFF
    type: related
  - target: 01KNZ5F57EZAMVV3P5991NVHJ9
    type: related
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:22.077447+00:00
modified: 2026-04-11T20:59:22.077453+00:00
source: "https://www.envoyproxy.io/docs/envoy/latest/configuration/http/http_filters/tap_filter"
---

# Envoy Tap Filter and Request Mirroring — Service-Mesh-Native Traffic Shadowing

Envoy, the L7 proxy that powers Istio, Consul Connect, and numerous other service meshes, provides two distinct mechanisms that together subsume most of what a dedicated replay tool does for HTTP workloads inside a Kubernetes cluster. These are increasingly the default path for production traffic replay in cloud-native shops that already run a service mesh.

## 1. Request mirroring (`request_mirror_policies`)

Configured per-route in Envoy's RDS. When a mirror policy is attached, Envoy sends a copy of every request matching the route to a secondary upstream cluster in addition to the primary. Responses from the mirror cluster are discarded. The original client sees only the primary's response.

```yaml
route:
  cluster: primary-service
  request_mirror_policies:
    - cluster: shadow-service
      runtime_fraction:
        default_value:
          numerator: 10
          denominator: HUNDRED
```

Key properties:

- **Per-route fraction control.** You can mirror 1% of requests to route A and 100% of route B with one config change.
- **Fire-and-forget.** Mirroring is out-of-band on the critical path; the client's latency is unaffected (in practice there is a tiny tax for the async copy).
- **Host header suffix.** By default Envoy appends `-shadow` to the Host/Authority header of mirrored requests, which lets downstream services distinguish shadow from real traffic and skip side effects. This is the convention for "mark this request as a shadow" and is quietly one of the most important design details in the whole traffic-shadowing space.

### Istio layer

Istio exposes Envoy mirroring through the `VirtualService` CRD with a `mirror` field and `mirrorPercentage`. Istio's docs (istio.io/latest/docs/tasks/traffic-management/mirroring/) are the usual entry point and treat mirroring as a canary/shadow-testing feature rather than a load-test feature — but the mechanism is identical.

## 2. Tap filter (`envoy.filters.http.tap`)

The tap filter is more general: it lets you tap arbitrary HTTP traffic matching a condition and stream it out to an output endpoint. Outputs include the admin endpoint (for interactive debugging) or a gRPC `TapSink` service that can log or forward the taps elsewhere.

The tap configuration is a match tree that can reference headers, status codes, path patterns, and response bodies. Matches are incremental, so the tap decides whether to emit before the full request is buffered.

What this is good for:

- **Capturing a real sample of production traffic** into a file or storage, with fine-grained filters ("all POSTs to /api/checkout that returned 500").
- **Feeding a downstream load generator.** The captured requests can be converted into k6 scripts or replayed through a Go program.

## Why mesh-native shadowing is better than an external tool

1. **No network-tap privilege required.** tcpreplay and GoReplay need libpcap on a production node, which is a security-sensitive capability. Envoy already sees every byte because it is the proxy; tapping it requires only an Envoy config change.
2. **TLS is not an obstacle.** Envoy terminates TLS upstream of the mesh, so the tap sees plaintext HTTP by construction.
3. **Uniform sampling semantics.** `runtime_fraction` is exact in a way that "I'm capturing on one interface out of three behind an LB" is not.
4. **Per-route granularity.** The declarative match tree lets you pick exactly which endpoints to shadow without touching any application code.

## Failure modes

1. **Still a replay problem at the bottom.** Envoy can mirror perfectly, but the target cluster still has to handle requests that assume state it may not have. Mirrors to a cluster with a clean database return errors. The `-shadow` header convention is a partial fix: it requires the target service to check the header and skip side effects, which is *another* code change the engineering team has to implement on every service. Most don't, and cross-team coordination is the real cost.
2. **Mirrored POST/PUT/DELETE still hit downstreams.** Unless the target is air-gapped, the shadow calls still talk to databases, caches, and other services. The `-shadow` header only helps services that check it.
3. **Fire-and-forget means no response diffing.** Envoy discards mirror responses. To do Diffy-style comparison you need a separate collector service that receives responses and diffs them; Envoy itself is not that collector.
4. **Head-of-line blocking if mirrors are slow.** Despite being async, if the mirror cluster is slow enough to back up, Envoy's worker threads spend cycles managing mirrored requests and it can affect primary latency. Production teams see this occasionally.
5. **Tap filter scaling.** The tap filter can generate a lot of data; streaming it all through the admin interface crushes the admin thread. Gated filters and gRPC sinks are the fix but require more infrastructure.

## Companion tools

- **nginx** has a comparable `mirror` module since 1.13.4, similar semantics, simpler config, no per-fraction rate.
- **HAProxy** does not have native traffic mirroring; historically the workaround is a Lua script.
- **AWS ALB/NLB** have no mirror; VPC Traffic Mirroring (see separate note) is the AWS-native answer.

## Toolmaker gap

Envoy gives you clean capture; nothing in the Envoy ecosystem turns the captured stream into a workload-model-aware load test with controlled arrival rate replay. The closest is GoReplay or custom replay tools reading the tap output. A declarative "tap → canonical intermediate representation → replay with rate/user controls" pipeline built on Envoy tap would be a natural APEX component.

## Citations

- Tap filter docs: https://www.envoyproxy.io/docs/envoy/latest/configuration/http/http_filters/tap_filter
- Traffic tapping operations: https://www.envoyproxy.io/docs/envoy/latest/operations/traffic_tapping
- Route mirroring sandbox: https://www.envoyproxy.io/docs/envoy/latest/start/sandboxes/route-mirror
- Istio mirroring task: https://istio.io/latest/docs/tasks/traffic-management/mirroring/
- Christian Posta on advanced shadowing with Istio: https://blog.christianposta.com/microservices/advanced-traffic-shadowing-patterns-for-microservices-with-istio-service-mesh/
- Known issue on shadow-suffix routing: https://github.com/envoyproxy/envoy/issues/9094