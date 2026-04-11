---
id: 01KNZ301FVPT3WBK9D4AHAN5ZB
title: "Brendan Gregg: Flame Graphs (Stack-Trace Visualization)"
type: reference
tags: [tool, profiler, flamegraph, visualization, methodology, brendan-gregg, off-cpu]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNZ301FV2VB7BHH13YAAG7SA
    type: related
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.brendangregg.com/flamegraphs.html"
author: "Brendan Gregg"
---

# Brendan Gregg: Flame Graphs (Stack-Trace Visualization)

**Primary reference:** https://www.brendangregg.com/flamegraphs.html
**Open-source implementation:** https://github.com/brendangregg/FlameGraph
**Author:** Brendan Gregg

## What flame graphs are

A flame graph is a visualization of a collection of stack traces sampled from a running program. Each rectangle in the image represents a stack frame. The y-axis is stack depth (zero at the bottom, deepest frames at the top). The x-axis is *not* time — it is the set of observed stacks sorted alphabetically, with identical adjacent frames merged into a single wider rectangle. The width of each rectangle is proportional to how often that frame appeared in the sampled stacks.

Because the x-axis is alphabetical rather than temporal, the merging is maximally effective: two different calls to the same function from different points in time collapse into a single visual block of proportional width. This produces a dense, readable summary of where the program's time is going, even for traces containing millions of samples.

## Why the shape matters

In a flame graph, **width means cost**. Wide frames near the top of the graph are the true hot spots — those are functions where the program is spending many samples and not delegating much of that cost to children. A narrow tall spike says "this call path is deep but rarely sampled." A wide tall mountain says "this call path is expensive and contains many interesting sub-paths."

For APEX-style worst-case analysis the central question is almost always "where is the time of this pathological input actually going?" A flame graph answers this in a single glance: the base of the hottest tower is the function where the blow-up happens.

## Types

Gregg's site documents six main flame-graph variants:

1. **CPU flame graphs** — the original. Stacks sampled by a CPU profiler (perf, DTrace, eBPF). Width = on-CPU samples = on-CPU time.
2. **Off-CPU flame graphs** — stacks sampled at scheduler-block events. Width = blocked/waiting time. Essential for I/O-bound analysis where CPU profiling shows an idle-looking system but the workload is actually waiting on disk, lock, or network.
3. **Memory flame graphs** — stacks sampled at allocation events. Width = bytes allocated. Directly useful for finding the call path responsible for an allocation blow-up — i.e., the same targets MemLock cares about.
4. **Differential flame graphs** — compare two profiles by coloring frames red where the new profile increased and blue where it decreased. This is the regression-detection version: run a flame graph on the baseline input and another on the pathological input, diff them, and the red hot path *is* the worst-case localization.
5. **Hot/cold flame graphs** — combine a profile and its inverse (what was running vs what was scheduled out) into a single graph.
6. **AI/GPU flame graphs** — newer variants adapted to accelerator pipelines.

Inverted ("icicle") charts show the same data top-down. Gregg notes he prefers the bottom-up flame shape for readability.

## Interactive features

The SVG output of the `flamegraph.pl` script is clickable and searchable in a browser. Clicking a frame zooms in on its subtree; searching highlights all frames containing a string and reports what fraction of the profile they account for. Modern JavaScript re-implementations (d3-flame-graph, speedscope, inferno) add pan/zoom, diffing, and large-trace streaming.

## Generating a CPU flame graph with Linux perf

The canonical one-liner:

```
git clone https://github.com/brendangregg/FlameGraph
perf record -F 99 -a -g -- sleep 30
perf script | ./FlameGraph/stackcollapse-perf.pl | ./FlameGraph/flamegraph.pl > out.svg
```

Step-by-step:

1. `perf record -F 99 -a -g` samples all CPUs at 99 Hz with stack traces (`-g`).
2. `perf script` emits one trace per line.
3. `stackcollapse-perf.pl` folds each trace into a single `;`-separated line with a sample count.
4. `flamegraph.pl` renders the folded stacks as SVG.

Later versions of `perf` can produce flame graphs natively: `perf script report flamegraph`.

## Supported profilers

**Linux:** `perf`, eBPF / BCC (`profile-bpfcc`), SystemTap, `ktap`.
**FreeBSD, illumos, Solaris:** DTrace.
**macOS:** Instruments (Time Profiler).
**Windows:** Windows Performance Analyzer (WPA), PerfView, Xperf / Event Tracing for Windows.

Commercial profilers (Google Cloud Profiler, AWS CodeGuru, Intel VTune, Java Mission Control, Datadog Continuous Profiler) now render flame graphs as a first-class output mode.

## Origin

Gregg invented the technique in 2011 while debugging a MySQL performance regression. He was frustrated by the "wall of text" output of traditional stack-sample summarizers and built a visualization that showed all stacks at once with frame merging. The name is a pun: the warm orange color palette (an allusion to "hot CPU") and the visual resemblance of deep stacks to rising flames. The technique was first presented at the USENIX LISA 2013 conference and is now documented in the ACM Queue article "The Flame Graph" (2016).

## Related visualizations

- **Flame charts** (note: different from flame graphs) place *time* on the x-axis. They are useful for seeing when an event happened but cannot aggregate identical frames. Chrome DevTools and Firefox Profiler use this shape.
- **Sunburst layouts** render the same hierarchical data in polar coordinates. They look impressive but are less information-dense than the cartesian flame graph for typical profile sizes.

## Relevance to APEX G-46

Flame graphs are the canonical *diagnostic* artifact to attach to a G-46 finding. When APEX emits a pathological input, the report should include:

1. A flame graph of the baseline input (brief, clean).
2. A flame graph of the pathological input (tall, wide tower where the worst case lives).
3. A differential flame graph between the two, with the red hot path highlighted.

This triple gives the downstream developer everything they need to localize the fix: which function, which source line, and how much of the cost lives there versus in callees. Because flame graphs compose with any stack sampler (perf, eBPF, DTrace, Instruments, WPA), APEX can support essentially every target platform with a single downstream visualization step.

Memory and off-CPU flame graphs extend the same pattern to MemLock-style memory-exhaustion bugs and to I/O-bound Slowloris-style bugs respectively. The G-46 report template should support all three axes (CPU, memory, off-CPU) with the same visualization grammar.
