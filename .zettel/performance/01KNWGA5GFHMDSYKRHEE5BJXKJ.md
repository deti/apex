---
id: 01KNWGA5GFHMDSYKRHEE5BJXKJ
title: "Tool: Google OSS-Fuzz"
type: literature
tags: [tool, oss-fuzz, google, fuzzing, continuous, open-source]
links:
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
created: 2026-04-10
modified: 2026-04-10
source: "https://google.github.io/oss-fuzz/"
---

# OSS-Fuzz — Continuous Fuzzing for Open-Source Software

*Source: https://google.github.io/oss-fuzz/ — fetched 2026-04-10.*

## Overview

OSS-Fuzz is a free service that applies fuzzing techniques to identify security vulnerabilities and stability bugs in open-source software. The initiative emerged from Google's experience discovering thousands of Chrome vulnerabilities through guided fuzzing.

## Core Description

"Fuzz testing is a well-known technique for uncovering programming errors in software." The service combines modern fuzzing methods with distributed execution, working alongside organisations like the Core Infrastructure Initiative and OpenSSF.

## Supported Technologies

The platform operates multiple fuzzing engines including **libFuzzer, AFL++, Honggfuzz, and Centipede**, paired with sanitisers. Currently compatible with "C/C++, Rust, Go, Python, Java/JVM code, JavaScript and Lua," supporting both x86_64 and i386 architectures.

## Project Success Metrics

As of August 2023, OSS-Fuzz has achieved significant impact: "over 10,000 vulnerabilities and 36,000 bugs across 1,000 projects" have been identified and remediated.

## Historical Context

Launched in 2016 following the Heartbleed vulnerability discovery, OSS-Fuzz addressed the need for accessible fuzzing capabilities that were previously manual and resource-intensive for developers.

## Relevance to APEX G-46

OSS-Fuzz is notable for surfacing **performance / DoS findings** through its ordinary fuzzing campaigns. The G-46 spec calls out two recent examples:

- **2024 — High-severity ReDoS in Keycloak** — discovered by OSS-Fuzz, escalated to a CVE.
- **2024 — Processing slowdown in Cert-Manager** — an OSS-Fuzz issue demonstrating exploitable DoS.

These are noteworthy because they are *not* crashes, sanitiser violations, or memory errors — they are slowdowns, caught incidentally when the fuzzer's timeout threshold fires. OSS-Fuzz's timeout detection is the crudest form of resource-guided fuzzing: any input that takes too long triggers a report. APEX G-46's resource-guided fuzzer should be an order of magnitude more sensitive, because it optimises for *maximum* slowdown rather than merely flagging anything past a threshold.

The OSS-Fuzz engine list (libFuzzer, AFL++, Honggfuzz, Centipede) is also the reference set of engines APEX must remain compatible with if it wants to integrate upstream with oss-fuzz projects — same target harness API, same sanitiser coverage format, same corpus directory layout.
