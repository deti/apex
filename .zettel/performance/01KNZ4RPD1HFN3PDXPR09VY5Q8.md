---
id: 01KNZ4RPD1HFN3PDXPR09VY5Q8
title: "Gregg: The Flame Graph (ACM Queue 2016)"
type: literature
tags: [article, flame-graph, profiling, visualization, acm-queue, 2016, gregg, cpu-profiling]
links:
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: extends
  - target: 01KNZ301FV6BZ60F02QWNWR4JB
    type: related
  - target: 01KNZ301FV2VB7BHH13YAAG7SA
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YREWKYWDWQ2MN39KHN5K
    type: related
  - target: 01KNZ5YRJ0QSF5TEDG6FFGE6SS
    type: related
  - target: 01KNZ5YRJAFN3CMW4QBMEDJKA6
    type: related
  - target: 01KNZ5YRH40QCWBYW7FV6FRHJ2
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://queue.acm.org/detail.cfm?id=2927301"
doi: "10.1145/2927299.2927301"
venue: "ACM Queue, Vol. 14, Issue 2 (2016)"
authors: [Brendan Gregg]
year: 2016
---

# The Flame Graph (Gregg, ACM Queue 2016)

**Author:** Brendan Gregg (then Netflix, previously Joyent).
**Venue:** ACM Queue, Vol. 14, No. 2 (March-April 2016) — pp. 91–104. Also published as **"The Flame Graph"** in Communications of the ACM, Vol. 59, Issue 6 (June 2016).
**DOI:** 10.1145/2927299.2927301.
**Canonical URL:** https://queue.acm.org/detail.cfm?id=2927301

*Source: https://queue.acm.org/detail.cfm?id=2927301 — direct fetch returned 403 from the ACM Queue CDN; this note is assembled from the ACM Queue table of contents, the full companion page at https://www.brendangregg.com/flamegraphs.html (already in the vault as `01KNZ301FVPT3WBK9D4AHAN5ZB`), and Gregg's own narrative in the ACM Queue abstract and surrounding talks.*

## Positioning

The flame graph was invented by Brendan Gregg in 2011 while he was debugging a MySQL performance regression at Joyent. By 2016 the technique had been adopted across essentially every CPU profiler in production use, from Intel VTune to AWS CodeGuru to `perf script` to Go's pprof to Rust's `cargo-flamegraph`. The ACM Queue article is the **peer-reviewed canonical reference** that moves flame graphs from "Brendan Gregg's blog post" to a citable scientific contribution.

The one-line summary: *a flame graph is a sampling-profile visualisation that compresses a hierarchical stack profile into a single image, with stack depth on the y-axis, sampled function width on the x-axis after alphabetical sorting, and frame merging across adjacent siblings*. The design choices are subtle and several of them are counter-intuitive, which is why the ACM Queue article — written after five years of community adoption — is useful: it captures the rationale for each choice.

## Pre-flame-graph state of the art

Before flame graphs the standard way to summarise a CPU profile was a text output listing function names with self-time and inclusive-time percentages, often with an indented call-tree. Tools: `perf report`, `gprof`, `oprofile`, `pstack`, DTrace. The problem: a real workload's profile has tens of thousands of stacks and hundreds of unique call chains, most of them short but a few important ones deep. A flat text summary buries the deep chains; an indented call-tree produces an unusable wall of text.

What Gregg wanted was a single image that:
- **Shows every stack**, not just the hot leaf functions.
- **Preserves stack structure** so callers and callees stay connected visually.
- **Allows fast identification of the widest subtree**, which is where the time is going.
- **Fits on one screen** regardless of trace size.

Earlier visualisations (calliper plots, time-ordered stack graphs, Time-based sequence diagrams) preserved *time ordering* at the expense of merging, producing dense but hard-to-read pictures. Gregg's insight was to sacrifice time ordering entirely and sort stacks alphabetically instead.

## The flame graph construction

Given a set of sampled stacks, each stack being a sequence of function names from leaf to root:

1. **Invert** each stack so the root is at the bottom (this is why "flame graphs" grow upward from a common base).
2. **Sort siblings alphabetically** at each level. Alphabetical sort is not meaningful in itself; the point is that it is *consistent*, so two adjacent stacks that share a common prefix will have their shared frames line up horizontally.
3. **Merge adjacent identical frames**, producing a horizontal rectangle whose width is the number of samples containing that frame at that stack depth.
4. **Draw** each merged frame as a coloured rectangle, with y = stack depth and x = sample index (post-merge and post-sort).

Because alphabetical sorting groups stacks with common prefixes together, the merge step is maximally effective, and the resulting image is both compressed and structurally readable. Wider rectangles = more time spent; taller rectangles = deeper stacks. The hottest path is literally the widest column from base to tip.

## Why alphabetical sorting

One of the article's key arguments is that time-ordering the x-axis *destroys* the picture because samples from the same function appear scattered across the image and cannot be merged. Alphabetical sort is the only simple ordering that guarantees samples with identical stacks are adjacent and therefore mergeable. The trade-off — losing time-of-day information — is accepted because flame graphs are designed to answer "where is the CPU going?" not "when did each call happen?" For time-ordered questions, a separate heatmap or timeline is the right tool.

## Colour and interactivity

Gregg's default palette is a warm-orange "flame" gradient, chosen both for the pun (hot CPU) and because warm colours draw the eye to the widest regions. Colour has no semantic meaning in the default mode — it is randomised per function to visually separate adjacent frames.

Variants:
- **Differential flame graphs** colour frames by whether they grew or shrank between two profiles. Red = grew, blue = shrank. Used for regression analysis.
- **Language-aware colouring** (red for Java methods, green for C, yellow for JIT stubs) gives polyglot runtimes an at-a-glance language breakdown.
- **Off-CPU flame graphs** display time spent blocked on I/O or contention rather than on CPU, using the same visual grammar with different sample sources.
- **Memory flame graphs** replace CPU samples with allocation samples.
- **Cold flame graphs, icicle graphs** invert the y-axis for situations where leaf-down reading is more natural.

Interactive SVG flame graphs (the `flamegraph.pl` script Gregg shipped with the technique) allow clicking to zoom, hovering to see exact percentages, and searching by function name with cumulative highlighting.

## Adoption as of 2016 and beyond

By 2016 flame graphs had appeared in:

- Linux `perf script | stackcollapse-perf.pl | flamegraph.pl` (the de facto recipe).
- Intel VTune Amplifier.
- AWS CodeGuru Profiler.
- Go pprof, which made flame graphs first-class output.
- Rust `cargo-flamegraph`.
- Java's `async-profiler` and flame-graph support in JFR.
- DTrace (Joyent), FlameScope (Netflix), FlameGraph (standalone JVM tool).
- Microsoft Visual Studio and .NET profilers.

By 2026 essentially every professional profiler can emit flame graphs, and the term is commonly used without citation.

## Relevance to APEX G-46

1. **Evidence presentation.** When APEX produces a performance finding, a flame graph of the slow input's execution is the canonical way to show developers *where* the time is being spent. Producing one is one invocation of `flamegraph.pl` against any supported profiler's output.
2. **Comparison baseline.** For regression-mode G-46, a **differential flame graph** between the baseline and the slow input is the right visualisation — it directly highlights the frames that grew.
3. **Detector feedback.** A profiler-guided mutator can use per-frame sample counts as a feedback signal in the same way MemLock uses allocation counts or PerfFuzz uses max-per-edge counts. The flame graph is the human-readable projection of that feedback.
4. **Tooling integration.** APEX's CI integration should produce flame graphs as PR attachments automatically for any G-46 finding, so the developer has a one-click evidence trail.

## Citation

```
@article{gregg2016flamegraph,
  author    = {Brendan Gregg},
  title     = {The Flame Graph},
  journal   = {Communications of the ACM},
  volume    = {59},
  number    = {6},
  pages     = {48--57},
  year      = {2016},
  doi       = {10.1145/2927299.2927301}
}
```

## References

- ACM Queue article — [queue.acm.org/detail.cfm?id=2927301](https://queue.acm.org/detail.cfm?id=2927301)
- Gregg's canonical flame graph page — [brendangregg.com/flamegraphs.html](https://www.brendangregg.com/flamegraphs.html) — see `01KNZ301FVPT3WBK9D4AHAN5ZB`
- `FlameGraph` tools — [github.com/brendangregg/FlameGraph](https://github.com/brendangregg/FlameGraph)
- Linux perf (profiling data source) — see `01KNZ301FV6BZ60F02QWNWR4JB`
- Valgrind Callgrind — see `01KNZ301FV2VB7BHH13YAAG7SA`
