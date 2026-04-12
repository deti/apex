---
id: 01KNZ68KG6N588XFB9A13H7RQZ
title: PerfLearner — Generating Performance Test Frames from Bug Reports
type: literature
tags: [perflearner, bug-reports, performance-testing, test-generation, nlp, mining-software-repositories]
links:
  - target: 01KNZ68KDQCP6HGTQEBGQW26VC
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:13:16.038151+00:00
modified: 2026-04-11T21:13:16.038156+00:00
source: "https://ieeexplore.ieee.org/document/9000010"
---

# PerfLearner — Learning to Generate Performance Test Frames from Bug Reports

*Published at a 2019 IEEE conference (document 9000010 on IEEEXplore; also surfaced in the ASE/ICSE ecosystem). Authors: Han, Yu, et al. The paper is the only published "learn-from-bug-reports" approach specifically targeting performance tests I have found.*

## Core claim and dataset

The authors manually analysed **300 bug reports** from Apache HTTP Server, MySQL, and Mozilla Firefox. They found that:

1. **Exposing performance bugs often requires specific combinations of input parameters.** It's not enough to exercise an endpoint — the bug only fires for particular parameter combinations.
2. **Certain input parameters recur across multiple bugs.** Some fields are "high-risk" — modifying them is likely to expose a performance bug.
3. **Bug reports often contain the execution commands and input parameters that reproduced the bug.** In natural language, sometimes partially, but extractable.

From these observations, PerfLearner builds a pipeline:

1. **NLP extraction.** Parse bug reports (text + optional stack traces) to pull out execution commands and input parameter references.
2. **Parameter aggregation.** Build per-parameter statistics across many bug reports — which parameters have been involved in bugs, with what values.
3. **Test frame generation.** Produce skeletal test cases ("test frames") that populate the risky parameters with bug-adjacent values.

## Why this matters

Most test generation ignores the huge corpus of historical bug reports. Every issue tracker contains, for free, a ground-truth dataset of "inputs that broke us." PerfLearner is the first published approach to mine that dataset specifically for *performance* bugs and use it to seed new tests.

Considerations that make this powerful:

- **Zero cost to find "risky parameters."** Bug reports are labelled: if a parameter shows up in three bug reports, it's clearly risky, no program analysis needed.
- **Cross-release signal.** Bugs from the last five releases tell you what kinds of inputs have historically mattered — a better prior than random.
- **Domain-neutral.** Works for any project with a public bug tracker.

## Adversarial reading

1. **Bug reports are noisy NLP.** Extracting parameter values from natural-language bug reports is imprecise. False positives ("parameter" as a word in a non-technical context) and false negatives (parameters mentioned in a code snippet that the NLP doesn't understand) are both common. The paper doesn't give cross-validation accuracy.
2. **Survivor bias.** Parameters that appear in bug reports are ones that had *reported* bugs. Parameters that cause silent slowness (without a user reporting) are invisible. This biases the dataset toward user-visible crashes, not performance issues proper.
3. **Training set is three projects.** Generalising to other codebases requires retraining or porting the NLP heuristics. The paper doesn't demonstrate transfer.
4. **Test frame ≠ test.** PerfLearner produces *frames* — skeletons with the risky parameters slotted in. Turning a frame into a runnable test still requires a harness, a workload profile, and assertions. The paper stops at the frame.
5. **Not an end-to-end tool.** No open-source implementation as far as I can tell; it is a published technique, not a shipping product.
6. **Time decay.** Bug reports from 2015 are less indicative of 2025 risk. There's no obvious way to weight recent bugs higher, and re-training costs engineer time.

## Why this is interesting to revisit with LLMs

The whole PerfLearner pipeline is an NLP + heuristics stack built pre-LLM. Every step gets easier with a 2023-class LLM:

- **Extraction.** An LLM can read a bug report and return structured JSON of reproduction steps with high accuracy — far better than the paper's heuristic extraction.
- **Classification.** An LLM can label bugs as "performance-related," "correctness," or "other" without a training set.
- **Parameter risk estimation.** An LLM can read many bug reports and synthesise a ranked list of risky parameters with explanations.
- **Test frame generation.** An LLM can write a runnable test given the extracted reproduction steps, not just a frame.

This is another instance of the pattern I noted elsewhere: a 2018-era software-analytics approach is now a five-LLM-call pipeline that probably outperforms the original with much less engineering. Rebuilding PerfLearner on an LLM stack is a natural research/prototype project.

## Related work from the same tradition

- **AutoPerf** (earlier) — load-generator tool; different approach, same target (multi-tier web apps).
- **Performance-bug classifiers** from the MSR (Mining Software Repositories) community — several papers classify GitHub issues as perf vs. non-perf.
- **BugDuplicate detection** tools — related infrastructure for deduping reports before mining.

## Citations

- IEEE paper: https://ieeexplore.ieee.org/document/9000010
- ResearchGate copy (behind login): https://www.researchgate.net/publication/327123463_PerfLearner_learning_from_bug_reports_to_understand_and_generate_performance_test_frames
- Related work: https://www.researchgate.net/publication/316355730_AutoPerf_Automated_Load_Testing_and_Resource_Usage_Profiling_of_Multi-Tier_Internet_Applications