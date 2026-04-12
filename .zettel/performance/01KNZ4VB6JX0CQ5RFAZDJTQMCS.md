---
id: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
title: "Schroeder, Wierman, Harchol-Balter: Open Versus Closed — A Cautionary Tale (NSDI 2006)"
type: literature
tags: [load-testing, workload-model, open-system, closed-system, partly-open, queueing, scheduling, mpl, think-time, schroeder, cmu]
links:
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: extends
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWGA5GS097K0SDS74JJ97X6
    type: related
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ6GWB2T03BDYP2SNGG8XJR
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6JDWZF3NFVSD5ATJV8
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ56MRW2B1XSH2X5K5AEJ33
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.usenix.org/legacy/event/nsdi06/tech/full_papers/schroeder/schroeder.pdf"
---

# Schroeder, Wierman, Harchol-Balter — "Open Versus Closed: A Cautionary Tale"

*Source: https://www.usenix.org/legacy/event/nsdi06/tech/full_papers/schroeder/schroeder.pdf — fetched 2026-04-12.*
*Venue: 3rd USENIX Symposium on Networked Systems Design and Implementation (NSDI), May 2006.*
*Authors: Bianca Schroeder (CMU), Adam Wierman (CMU), Mor Harchol-Balter (CMU).*

This is the single most important paper on load-test workload modeling. If a performance engineer reads exactly one paper before running a load test, this should be it. The thesis is that the choice of **open** versus **closed** workload model — which most tools make silently and most testers do not think about — changes measured response times by **an order of magnitude or more**, reverses the benefit of scheduling policies, and invalidates cross-study comparisons.

## The three models (verbatim definitions from §2)

**Closed system.** There is some *fixed number of users* N — the multiprogramming level (MPL). Each user repeats forever: (a) submit a job, (b) receive the response, (c) think for some time Z. **A new request is triggered only by the completion of a previous one.** At any instant N_think users are thinking and N_system users are in the service (queued or running), with N_think + N_system = N. The canonical example is a classical interactive mainframe timeshare — and, as the paper emphasizes, almost every modern HTTP load tool (WebStone, Siege, Hammerhead, the MS WAST tool, RUBiS, TPC-W, TPC-C…). The closed loop is inherent in the tool: a virtual user waits for its response before sending the next request.

**Open system.** There is a stream of arriving users with average rate λ. Each user submits *one* job, waits, receives the response, and leaves. **A new request is triggered only by a new user arrival, never by a completion.** The number of outstanding jobs is unbounded (can range from 0 to ∞). Examples of open-model tools: httperf, SPECWeb96, SPECmail2001, NS traffic generator, Sclient.

**Partly-open system.** Users arrive from outside (like open). After each request completes, the user *stays* with probability p (possibly after a think time) and *leaves* with probability 1 – p. The number of requests per visit is Geometric with mean 1/(1–p). A collection of requests from one user's visit is a **session**. As p → 0 the partly-open system is open; as p → 1 (with fixed arrival rate compensated by session length) it approaches closed. **This is the right model for most web workloads**, the authors argue.

Load ρ in all three is (arrival rate) × (mean service demand E[S]); in the closed system load is set by adjusting think time Z, because higher Z means users are more often thinking instead of issuing work.

## Why most load tools are closed without telling you

Table 1 of the paper enumerates workload generators and their silent assumptions. The vast majority — WebStone, Surge, WebBench, TPC-W, TPC-C, RUBiS, RUBBoS, MS WAST, Hammerhead, Siege, and so on — are **closed by construction**, because a virtual user is a loop: send request, `read(sock)`, wait for full response, think, loop. The authors: *"for many of these workload generators, it was quite difficult to figure out which system model was being assumed — the builders often do not seem to view this as an important factor worth mentioning in the documentation."* A small minority (httperf, Sclient, NS, SPECWeb96, SPECmail2001) are open.

The critical observation for the modern reader: **this is still true in 2026**. JMeter, Gatling, Locust, k6, and Tsung all default to fixed-VU closed-loop generation. k6 has supported open-model "arrival-rate executors" since v0.27, Gatling has an "open injection profile", JMeter has "Constant Throughput Timer", but the default in every mainstream tool is still virtual-users-in-a-loop, i.e. closed.

## The 8 principles

The paper's contribution is eight principles that summarise how closed and open models differ. They are derived via a mix of real implementations (web server + static HTTP, database back-end of an e-commerce site), trace-driven simulation (auction site, supercomputer), and model-based simulation.

**(i) For a given load, mean response times in an open system are much higher than in a closed system with equal load.** The magnitude of the gap depends on MPL and on service-time variability. At moderate loads the open system can be an order of magnitude slower. At first glance this seems wrong — surely the closed system's back-pressure would make it slower — but the mechanism is subtle: in closed systems, a completion immediately unblocks the next arrival, so the *correlation* between completion times and arrival times creates a feedback loop that limits the queue length. Open systems have no such brake.

**(ii) As the MPL grows, the closed system approaches the open one — but very slowly.** At MPL = 100 a closed system "still behaves closed". Even at MPL = 1000 there is a significant gap. Converting rule-of-thumb: you need MPL on the order of several thousand for a closed generator to approximate open-system response times, and the rate of convergence slows down with higher service-time variability C².

**(iii) Service-time variability (C²) has a large effect on open systems but a much smaller effect on closed systems.** Heavy-tailed job-size distributions blow up open-system response times but are partially absorbed by the closed-loop back pressure. A closed-loop load test on a heavy-tailed workload *understates* the real user pain.

**(iv) Open systems benefit significantly from scheduling that favours short jobs (SRPT, PSJF).** The paper shows order-of-magnitude mean-response-time improvements in open systems from switching FCFS → SRPT.

**(v) Scheduling only significantly improves response time in a closed system under narrow conditions** — high MPL *and* moderate load *and* high C². Under most closed-system operating points, SRPT barely helps. **This means a closed-loop load test will tell you "scheduling doesn't matter" on a system where it matters enormously in production.**

**(vi) Scheduling can limit the effect of variability in an open system.** SRPT makes high-C² workloads behave more like low-C² ones — a form of variability insurance that a closed system cannot provide because it doesn't see the variability in the first place.

**(vii) A partly-open system behaves similarly to a closed one when the session length exceeds a threshold (≈ 10 requests per session for C² = 4, up to ≈ 20 for C² = 49), and similarly to an open one for shorter sessions.** This is the actionable rule for choosing between open and closed when your tool forces a binary pick.

**(viii) In a partly-open system, think time has almost no effect on mean response time.** This is the most counter-intuitive principle and directly contradicts folklore. The intuition: think time only affects *when* the user issues its next request during a session, not the total work submitted or the arrival rate of new sessions. Load ρ = λ · E[R] · E[S], independent of think time. Formally provable for PS and FCFS under product-form workloads.

## Concrete method for choosing a model from a trace

The paper demonstrates the method on 10 real web traces (corporate site, CMU, online department store, USGS, online gaming, financial service, supercomputer site, Kasparov–DeepBlue, a slashdotted site, 1998 soccer world cup).

1. Collect traces from the target system.
2. Build the partly-open model: estimate session length E[R] from trace using a timeout τ (the defacto 1800s = 30 min works for most cases; alternatively find the knee in the "number of sessions vs τ" curve).
3. Apply principle (vii): if E[R] < ~5 requests/session → open model is fine; if E[R] > ~15 → closed is fine; in between, you need partly-open.

Reported per-trace session lengths:

| Site | Requests/session |
|---|---|
| Large corporate | 2.4 |
| CMU web server | 1.8 |
| Online dept store | 5.4 |
| USGS | 3.6 |
| Online gaming | **12.9** |
| Financial service | 1.4 |
| Supercomputing site | 6.0 |
| Kasparov–DeepBlue | 2.4 |
| Slashdot'd site | 1.2 |
| 1998 World Cup | **11.6** |

→ Corporate, CMU, USGS, financial, Kasparov, slashdot sites should be tested with *open* models. Online gaming and World Cup should be tested with *closed* models. Department store and supercomputing site need *partly-open*. Using the wrong one invalidates the study.

## Why this matters for APEX G-46 and performance-test generation

1. **APEX's G-46 spec (the vault's root note) explicitly puts load/stress/concurrent-user testing "out of scope".** This paper is the reason that decision is defensible: doing it properly requires choosing the right workload model per target, which in turn requires trace analysis or user research. It cannot be a one-line flag in a test generator.
2. **When APEX does add load generation (phase 2 / post-G-46), the default must be *open* or *partly-open*, not closed.** Every modern closed-loop tool inherits the principle-(v) bug: scheduling-insensitive results that mislead design choices.
3. **Coordinated omission** (Gil Tene, separate note) is a closely related phenomenon that appears inside closed-loop generators. Schroeder et al. is the system-level framing; coordinated omission is the latency-metric-level framing. Both point at the same underlying flaw: generators that throttle themselves when the SUT slows down produce numbers that do not reflect user experience.

## Adversarial reading

- The paper uses mean response time throughout. The *tail* (p99, p99.9) differences between open and closed models are almost certainly even more dramatic than the means reported here, because closed systems clip the tail. A 2006 follow-up would have benefited from HdrHistogram, which was built around 2012.
- The "partly-open" model assumes a Geometric distribution of session length. Real session lengths are often heavy-tailed themselves (power-law number of pages per visit). The principles still hold qualitatively but the 10-requests-per-session threshold may shift under heavy-tailed sessions.
- The real-system experiments are ~2005-vintage hardware and software. The *qualitative* conclusions about open-vs-closed are architecture-independent (they come from queueing theory), but the specific response-time ratios would be different on a modern async server with thousands of in-flight requests.

## References

- Schroeder, Wierman, Harchol-Balter — "Open Versus Closed: A Cautionary Tale" — NSDI 2006 — [usenix.org PDF](https://www.usenix.org/legacy/event/nsdi06/tech/full_papers/schroeder/schroeder.pdf)
- Bondi, Whitt — "The influence of service-time variability in a closed network of queues" — *Performance Evaluation* 6:219–234, 1986 (cited as prior art; same qualitative finding for FCFS)
- Schatte — "The M/GI/1 queue as limit of closed queueing systems" — 1984 (formal proof of closed→open as MPL → ∞)
- Little's Law note — `01KNZ4VB6J4TER1QCE9CKABBED`
- Coordinated Omission note — `01KNZ4VB6JB4Q5H3NPS72MZZ2A`
