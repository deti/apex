---
id: 01KNZ5F50KTSH9742RX4CFT45K
title: tcpreplay and tcpliveplay — Packet-Level Traffic Replay
type: literature
tags: [tcpreplay, tcpliveplay, pcap, traffic-replay, network-testing, test-generation]
links:
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:22.003603+00:00
modified: 2026-04-11T20:59:22.003608+00:00
source: "https://tcpreplay.appneta.com/"
---

# tcpreplay and tcpliveplay — Packet-Level Traffic Replay

tcpreplay is the venerable Unix tool (Aaron Turner, 2001) for replaying captured `.pcap` files at wire speed onto an interface. It is a *packet-level* replay tool, not a session-level one: it does not understand HTTP or any application protocol. You load a pcap and it sends the bytes back out.

## The tcpreplay family

- **tcpreplay** — replays pcap file at user-specified rate (original speed, max NIC speed, or arbitrary Mbps / packets-per-second) onto an interface.
- **tcprewrite** — rewrites fields (MAC, IP, ports, checksums) in a pcap file so it can be replayed into a different topology.
- **tcpliveplay** — the less well-known but most relevant sibling: it replays a pcap against a *live* server and actively re-establishes TCP sessions, handling sequence numbers and retransmits. Unlike tcpreplay which blasts bytes without caring whether anyone is listening, tcpliveplay cares.
- **tcpbridge** — bridge two interfaces with optional packet manipulation.

## Use for performance testing

tcpreplay is the classic tool for:

- Stress-testing firewalls, IDS/IPS, and load balancers with replayed real packet captures. Because a lot of perf bugs in network appliances are triggered by specific byte patterns, replaying real traffic is much higher-fidelity than synthesising HTTP with a load generator.
- Regenerating packet-level traffic shapes for L4 benchmarks — TCP handshake rate, SYN flood characteristics, large/small packet distribution.
- Testing kernel network-stack changes against historic traffic.

It is **not** a good tool for application-level performance testing. HTTP APIs need a tool that understands HTTP; replaying the raw packet bytes to a web server does not elicit a meaningful session because the server's response TCP sequence numbers will not match what the client pcap expected. That's why tcpliveplay exists — but tcpliveplay is significantly slower and still operates at the transport layer, so it cannot rewrite HTTP headers, substitute auth tokens, or reshape payloads.

## Fidelity vs. GoReplay

tcpreplay operates one layer below GoReplay. Trade-offs:

- **Fidelity:** tcpreplay is higher-fidelity at the network layer (real TCP options, real IP flags, real MSS). GoReplay reconstructs the HTTP request and generates a fresh TCP session, losing all L3/L4 detail but gaining L7 editability.
- **Editability:** GoReplay wins — you can modify HTTP bodies/headers mid-stream with middleware. tcpreplay edits require tcprewrite and re-computing checksums.
- **Scale:** tcpreplay wins at packet rate (it can saturate a 100 Gb/s NIC) because it doesn't do per-packet parsing. GoReplay tops out much earlier because it reconstructs HTTP.
- **Test target:** tcpreplay is for testing network devices. GoReplay is for testing web services.

## Failure modes

1. **Session state cannot match.** A pcap replayed to a new server cannot have the same TCP sequence numbers, ACK numbers, or server-generated session IDs. tcpliveplay papers over this at the TCP layer but higher-layer state (HTTP cookies, JWTs) is still wrong.
2. **Rate shaping is wall-clock only.** tcpreplay has no notion of "emulate N concurrent users" — it replays at a specified rate based on the original capture's timestamps or at a fixed pps.
3. **NAT and certificate issues.** Replayed traffic with embedded IP addresses that don't match the test environment fails; tcprewrite can fix this but it's a manual step. TLS traffic is effectively unreplayable because the handshake has unique randoms.
4. **No PII scrubbing primitive.** Working at the packet layer, tcpreplay does not offer the kind of HTTP-body scrubbing middleware GoReplay has. You'd have to write a custom pcap rewriter.

## Why it still matters in 2024+

tcpreplay is the ground-truth tool for anyone writing *new* replay tools. It defined the model of "capture → rewrite → replay" that everything else (including GoReplay, AWS VPC Mirror, Envoy tap filter) is a variant of. If you are designing a new replay system, you should know tcpreplay's design trade-offs cold.

## Citations

- https://tcpreplay.appneta.com/ (canonical home, maintained by AppNeta)
- https://github.com/appneta/tcpreplay
- Man page: https://linux.die.net/man/1/tcpreplay
- tcpliveplay background: https://tcpreplay.appneta.com/wiki/tcpliveplay.html