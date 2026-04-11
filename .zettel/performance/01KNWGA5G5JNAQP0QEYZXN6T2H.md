---
id: 01KNWGA5G5JNAQP0QEYZXN6T2H
title: "Incident: Stack Overflow July 20 2016 Outage (ReDoS)"
type: literature
tags: [incident, stack-overflow, redos, postmortem, regex, 2016]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: supports
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://stackstatus.tumblr.com/post/147710624694/outage-postmortem-july-20-2016"
---

# Stack Overflow Outage, July 20 2016 — ReDoS in Homepage Post Rendering

*Source: https://stackstatus.tumblr.com/post/147710624694/outage-postmortem-july-20-2016 — fetched 2026-04-10.*

## Overview

A 34-minute outage occurred starting at 14:44 UTC on 20 July 2016. The incident took:
- 10 minutes to diagnose,
- 14 minutes to code a fix, and
- 10 minutes to deploy it.

## Root Cause

A malformed post containing roughly **20,000 consecutive whitespace characters** triggered problematic regex behaviour on the homepage, causing high CPU consumption on web servers. Since the load balancer used the homepage for health checks, the affected servers were taken out of rotation, making the entire site unavailable.

## The Problematic Regex

```
^[\s\u200c]+|[\s\u200c]+$
```

This pattern was meant to trim Unicode spaces (including the zero-width non-joiner `\u200c`) from the start and end of a line. A simplified version exposing the same issue: `\s+$`.

## Technical Explanation

When the regex engine encountered 20,000 consecutive spaces followed by a non-space character, it performed massive backtracking. The engine checked whether spaces belonged to the `\s` character class repeatedly, resulting in approximately **199,990,000 character-class checks** (`20,000 + 19,999 + 19,998 + ... + 1`). This O(n²) performance degradation caused the severe slowdown.

The specific shape — `\s+$` matching against `<many spaces><non-space>` — is a polynomial ReDoS. Each time the match fails at the end of the run, the engine backs off one character and retries.

## Resolution

The problematic regular expression was replaced with a **substring function** (trimming whitespace via explicit index walking), eliminating the backtracking issue.

## Relevance to APEX G-46

The Stack Overflow incident is the perfect counter-example to "exponential ReDoS is the only dangerous kind". This was O(n²), not exponential, yet completely sufficient to take down a top-50 global website. Any APEX ReDoS detector that only flags exponential patterns would have missed it.

The lesson: **polynomial ReDoS is a first-class problem**, and the detector/complexity-estimator combination needs to catch both polynomial (O(n²), O(n³)) and exponential (O(2ⁿ)) cases. The empirical complexity estimator described in the G-46 spec covers this naturally — any regex whose empirical fit is super-linear should be flagged, regardless of the specific growth rate.

Additional lesson from the health-check coupling: APEX should probably surface a "performance blast radius" signal. A function used in a health check path is much more dangerous than the same function called only in a background job. This is a reachability / criticality overlay the static analysis can provide.
