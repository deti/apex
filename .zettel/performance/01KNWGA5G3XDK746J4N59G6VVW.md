---
id: 01KNWGA5G3XDK746J4N59G6VVW
title: "Incident: Cloudflare July 2 2019 Outage (ReDoS)"
type: literature
tags: [incident, cloudflare, redos, waf, postmortem, regex, 2019]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
  - target: 01KNWGA5G80W4ESMANJM0M2XAV
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://blog.cloudflare.com/details-of-the-cloudflare-outage-on-july-2-2019/"
---

# Cloudflare Outage, July 2 2019 — ReDoS in a WAF Rule

*Source: https://blog.cloudflare.com/details-of-the-cloudflare-outage-on-july-2-2019/ — fetched 2026-04-10.*

This post-mortem is the best-known production ReDoS incident in recent history. A single bad regex in a WAF rule took Cloudflare's global network offline for 27 minutes, and the root-cause analysis covers not just the regex but an entire defence-in-depth failure chain.

## The Problematic Regex

```
(?:(?:\"|'|\]|\}|\\|\d|(?:nan|infinity|true|false|null|undefined|symbol|math)|\`|\-|\+)+[)]*;?((?:\s|-|~|!|{}|\|\||\+)*.*(?:.*=.*)))
```

The critical problematic portion was `.*(?:.*=.*)`, which created excessive backtracking.

## Backtracking Problem Explained

The regex engine uses a greedy matching approach with backtracking. When matching `.*.*=.*` against input like `x=x`, the first `.*` consumes the entire string, then the engine must backtrack repeatedly to find valid matches. This creates non-linear performance degradation:

- Matching `x=x`: **23 steps**
- Matching `x=` plus 20 x's: **555 steps**

"With 20 `x`'s after the `=` the engine takes 555 steps to match!" The problem worsens dramatically with longer strings, demonstrating catastrophic backtracking.

## Timeline (UTC)

- **13:31** — Engineer merged the problematic pull request.
- **13:37** — TeamCity built and tested the rules, passing all checks.
- **13:42** — Automated deployment began via Quicksilver.
- **13:45** — First PagerDuty alert for WAF failure.
- **14:00** — WAF identified as the culprit.
- **14:02** — Decision made to use global WAF termination.
- **14:07** — Global WAF termination executed.
- **14:09** — Traffic and CPU returned to normal levels.
- **14:52** — WAF re-enabled globally after verification.

Net effect: 27 minutes from first alert to recovery, ~40 minutes from deploy to recovery.

## Root Causes (11 Convergent Vulnerabilities)

The post-mortem identified multiple concurrent failures:

1. Poorly written regex with excessive backtracking potential.
2. CPU-protection mechanism accidentally removed during prior refactoring.
3. Regex engine lacked complexity guarantees.
4. Test suite did not measure CPU consumption.
5. SOP permitted non-emergency global deployments without staged rollout.
6. Rollback procedure required two complete WAF builds.
7. Global traffic-drop alert fired too slowly.
8. Status-page updates were delayed.
9. Team could not access internal systems due to the outage itself.
10. Employee credentials timed out due to security policies.
11. Customers could not reach Dashboard/API through Cloudflare's edge.

## Remediation Actions

Cloudflare implemented seven key fixes:

1. "Re-introduce the excessive CPU usage protection that got removed" (completed immediately).
2. Manual inspection of all 3,868 WAF rules for backtracking issues (inspection complete).
3. "Introduce performance profiling for all rules to the test suite" (ETA: July 19).
4. Switch to RE2 or Rust regex engines with runtime guarantees (ETA: July 31).
5. Change SOP to staged rollouts while preserving emergency deployment capability.
6. Create emergency ability to take Dashboard/API offline from Cloudflare's edge.
7. Automate Cloudflare Status-page updates.

Long-term, the team planned to migrate from the Lua WAF to a new firewall engine for improved performance and additional protections.

## Relevance to APEX G-46

This incident is the "why" for the entire G-46 feature. Every box on Cloudflare's remediation list is something APEX could have automated *before* the outage:

- **#3** ("performance profiling for all rules to the test suite") — exactly what APEX's resource profiling layer gives you.
- **#4** ("switch to RE2 or Rust regex engines with runtime guarantees") — APEX's ReDoS detector should surface this as a recommended mitigation, and its complexity estimator should verify RE2's linear claim empirically.
- **#2** ("manual inspection of all 3,868 WAF rules") — APEX's ReDoS analysis automates this.

The `.*(?:.*=.*)` pattern should be on every ReDoS regression corpus as a canonical test case. So should the input `x=` + long suffix of `x`s.
