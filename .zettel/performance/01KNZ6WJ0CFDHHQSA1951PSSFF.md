---
id: 01KNZ6WJ0CFDHHQSA1951PSSFF
title: aws-samples/http-requests-mirroring — Reference Implementation of VPC-Mirror HTTP Replay
type: literature
tags: [aws, vpc-traffic-mirroring, replay, go, reference-implementation, production-traffic]
links:
  - target: 01KNZ5F557YM1X8Q2ZBXZEBXRM
    type: related
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:24:09.868904+00:00
modified: 2026-04-11T21:24:09.868910+00:00
source: "https://github.com/aws-samples/http-requests-mirroring"
---

# aws-samples/http-requests-mirroring — Reference HTTP Replay on VPC Traffic Mirroring

The reference implementation AWS publishes for the VPC Traffic Mirroring → HTTP replay pipeline. Open-sourced as aws-samples/http-requests-mirroring. Companion to the AWS blog post "Mirror production traffic to test environment with VPC Traffic Mirroring."

## What it is

A Go program that:

1. Listens as the target of a VPC Traffic Mirror session on UDP 4789 (VXLAN).
2. Decapsulates the mirrored packets.
3. Reassembles TCP streams.
4. Parses HTTP requests from the reassembled streams.
5. Forwards a configurable percentage of parsed requests to a specified target URL.
6. Optionally records requests to a file for later offline replay.

The key parameter is `ForwardPercentage`, which controls what fraction of mirrored requests are forwarded. At 100 it's a shadow; at 10 it's a sample; at 0 it's capture-only.

## Why this specific tool matters

It is the *cleanest reference implementation* of the full mirror → replay pipeline on AWS. Reading the source is the fastest way to understand what you have to build yourself if you want to use VPC Traffic Mirroring in a production shadow-testing workflow. The code shows:

1. **gopacket usage for VXLAN.** How to decode the UDP-wrapped Ethernet frames that AWS mirror sessions emit.
2. **TCP reassembly.** Using the `reassembly` package from gopacket to turn packet streams into byte streams.
3. **HTTP parsing.** Standard `net/http` once you have a byte stream.
4. **Rate limiting and forwarding.** How to fan out at a controlled rate.

For organisations that can't use GoReplay or Envoy tap (see dedicated notes) because they're doing platform-level mirroring from the AWS control plane, this repo is effectively the starting point.

## Adversarial reading

1. **Reference code != production code.** The repo is a sample. It doesn't handle all the edge cases you'd hit in a real fleet: HTTPS, HTTP/2, streaming bodies, connection reuse, etc. AWS is clear about this; users who treat it as drop-in production code get bitten.
2. **Single-host scaling ceiling.** A single Go process listening on UDP 4789 can handle maybe 50k-100k mirrored packets per second. For a larger production traffic volume you need multiple instances and a mirror-session load balancer in front.
3. **Still doesn't fix replay divergence.** Every issue described in the replay-divergence concept note applies. The tool does the mirror/replay plumbing correctly, but the state-management, auth-refresh, and side-effect problems are the user's responsibility.
4. **HTTPS blindness.** Packets captured from the VPC mirror are raw Ethernet. HTTPS is still encrypted. To make this work on TLS traffic, you must mirror *after* TLS termination — meaning from an instance that's already the terminator (ALB target) rather than directly from application pods.
5. **Maintenance status.** The repo is an AWS sample, which AWS periodically updates but doesn't treat as a supported product. Issues sit longer than they would for a first-party service.

## Why engineers should still read it

Even if you're not using AWS:

- The decapsulation code is a mini-course in parsing VXLAN and TCP reassembly.
- The replay-rate and forwarding design shows the trade-offs of a single-process replay architecture.
- The gap between "we have the packets" and "we have a working shadow test" is explicit in the code, which is useful for estimating the cost of building your own replay pipeline.

The aws-samples repo is an unusually honest piece of code about how much work real production replay is. Reading it should deflate any engineer's optimism that "we'll just replay prod traffic."

## Citations

- https://github.com/aws-samples/http-requests-mirroring
- Companion blog: https://aws.amazon.com/blogs/networking-and-content-delivery/mirror-production-traffic-to-test-environment-with-vpc-traffic-mirroring/
- gopacket: https://github.com/google/gopacket