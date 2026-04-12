---
id: 01KNZ5F557YM1X8Q2ZBXZEBXRM
title: AWS VPC Traffic Mirroring — Cloud-Native Packet Shadowing
type: literature
tags: [aws, vpc-traffic-mirroring, packet-mirror, cloud, production-traffic, test-generation]
links:
  - target: 01KNZ6WJ0CFDHHQSA1951PSSFF
    type: related
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:22.151145+00:00
modified: 2026-04-11T20:59:22.151150+00:00
source: "https://docs.aws.amazon.com/vpc/latest/mirroring/what-is-traffic-mirroring.html"
---

# AWS VPC Traffic Mirroring

VPC Traffic Mirroring is an AWS-managed feature that copies network traffic from an ENI (Elastic Network Interface) of an EC2 instance to a mirror target. It operates out-of-band at the VPC network layer with no software agent on the production host. Announced in 2019.

## Architecture

- **Mirror source:** an ENI attached to a Nitro-based EC2 instance. Earlier instance families did not support mirroring; modern ones do.
- **Mirror target:** an ENI of another EC2 instance, an NLB with a UDP listener, or a Gateway Load Balancer with a UDP listener.
- **Mirror filter:** rules that select which traffic to mirror (protocol, source, destination, ports). Inbound and outbound can be mirrored independently.
- **Mirror session:** ties source+target+filter and includes session number, VXLAN network identifier, and packet length limit.

Traffic is encapsulated in VXLAN (UDP 4789 by default) and forwarded to the target. The target is responsible for decapsulation and whatever processing follows.

## Use for performance testing

The canonical AWS blog post ("Mirror production traffic to test environment with VPC Traffic Mirroring", networking blog, 2020-ish) is the reference for building a replay path:

- Production instance ENI → mirror session → NLB with UDP listener → EC2 instance running a decapsulator + HTTP replayer (typically custom Go code using gopacket).
- The replayer re-issues HTTP requests (after VXLAN decapsulation and TCP stream reassembly) against the staging target.
- `ForwardPercentage` parameter controls how many requests are replayed.

AWS publishes reference source at aws-samples/http-requests-mirroring that shows exactly this pipeline.

## Why organisations use it instead of GoReplay

1. **No agent on production.** The capture is done by the AWS hypervisor. No root on the production host, no LD_PRELOAD, no pcap permissions. This is the single biggest adoption driver for security-conscious teams.
2. **Scales with the host.** The hypervisor does the copy; no CPU tax on the guest.
3. **Native to AWS RBAC.** Mirror session creation is an IAM action — you get audit trail and least-privilege for free.
4. **Works across the whole fleet.** You can mirror traffic from hundreds of ENIs to a central replay target without deploying any software across the fleet.

## Failure modes

1. **Nitro-only, AWS-only.** Not portable. No equivalent in on-prem vSphere, no equivalent in GCP (GCP has "Packet Mirroring" which is similar but incompatible), only partial equivalent in Azure (vTap, in preview for a long time).
2. **TLS still unreadable.** VPC mirroring gives you packets. HTTPS traffic is encrypted. For HTTP-level replay you must either mirror behind the TLS terminator (ALB → target), which means mirroring *instances* that see plaintext, or somehow share TLS keys with the replayer (a compliance nightmare).
3. **VXLAN overhead on the mirror target.** The decapsulator has to keep up with peak traffic. If the mirror target can't, you lose mirrored packets silently. Tight coupling between production scale and mirror-processing capacity is easy to get wrong.
4. **Packet-size truncation.** The default mirror session truncates large packets. Your replay can silently miss request bodies larger than the limit.
5. **Replayer complexity.** The hard part is not capture — it's the custom Go/C++ service that reassembles TCP streams from mirrored packets and re-issues HTTP requests. AWS provides sample code, but "production-grade" means you own it. Compare to GoReplay which ships ready-made.
6. **Same state-divergence problem as every replay tool.** Mirroring to a staging environment that does not have the same database rows as prod results in massively different latency distributions because the working set is different.
7. **Cost.** VXLAN traffic is billed as data transfer. At scale this can be material — thousands of dollars per month for a moderately loaded service.

## Why it still matters

For regulated environments (financial services, healthcare) where installing packet-capture software on production hosts is forbidden by policy, VPC Traffic Mirroring is often the *only* path to realistic replay. It buys organisational feasibility at a substantial technical and economic cost.

## Citations

- Docs: https://docs.aws.amazon.com/vpc/latest/mirroring/what-is-traffic-mirroring.html
- AWS blog: https://aws.amazon.com/blogs/networking-and-content-delivery/mirror-production-traffic-to-test-environment-with-vpc-traffic-mirroring/
- Reference sample code: https://github.com/aws-samples/http-requests-mirroring
- InfoQ article on mesh + VPC mirror: https://www.infoq.com/articles/microservices-traffic-mirroring-istio-vpc/
- GCP Packet Mirroring (for comparison): https://cloud.google.com/vpc/docs/packet-mirroring