---
id: 01KNWGA5GMWKV6AKP04D964G5H
title: "Tool: vuln-regex-detector (Ensemble ReDoS Scanner)"
type: literature
tags: [tool, vuln-regex-detector, davis, redos, static-analysis, ensemble]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://github.com/davisjam/vuln-regex-detector"
---

# vuln-regex-detector — Ensemble ReDoS Scanner

*Source: https://github.com/davisjam/vuln-regex-detector — fetched 2026-04-10.*

Maintained by James Davis, the first author of the canonical ESEC/FSE 2018 empirical study of ReDoS in practice. This tool is the practical embodiment of the methodology from that paper.

## Purpose

The vuln-regex-detector project provides tools to identify regexes susceptible to catastrophic backtracking (ReDoS attacks) through static code analysis.

## How It Works

The tool employs a three-stage pipeline:

1. **Regex Extraction** — statically extracts regex patterns from source code.
2. **Vulnerability Detection** — tests regexes against multiple vulnerability detectors.
3. **Vulnerability Validation** — confirms findings in the target programming language.

## Detection Approach

The tool uses an **ensemble methodology** where multiple detectors analyse each regex. Detectors are designed to catch different kinds of ReDoS (exponential vs polynomial, different NFA shapes), and the ensemble reports the union of flagged regexes. This trades precision for recall — the point is to catch everything; false positives are filtered by the dynamic validator stage.

The project includes language-specific validators for Perl, Python, JavaScript, Java, PHP, and Rust. These run the suspect regex in the actual target language's engine against a synthesised witness input, which is the only reliable way to confirm exploitability (since engines differ in their backtracking behaviour).

## Key Features

- **Local deployment** via configuration script and binaries.
- **Remote queries** through an npm module connecting to Virginia Tech servers.
- **Docker support** for non-Ubuntu systems.
- **Vulnerability validators** confirming results in actual languages rather than relying solely on static analysis.

## Important Limitations

The analysis is:
- **Static** — misses dynamically-defined patterns built from user input at runtime.
- **Input agnostic** — flags regexes regardless of whether the surrounding code actually exposes them to untrusted input.
- **Detector dependent** — may miss vulnerabilities no individual detector recognises.

## Usage

Users set the `VULN_REGEX_DETECTOR_ROOT` environment variable, run the configure script, then employ binaries from the `bin/` directory for scanning projects.

## Relevance to APEX G-46

vuln-regex-detector is the closest thing to a reference implementation for the ReDoS analysis capability the G-46 spec describes. APEX should:

1. **Match or exceed its detection recall** (≥80% on a shared benchmark corpus — G-46 acceptance criterion).
2. **Improve on its false-positive rate** by adding taint analysis: only flag a regex if the code path that runs it is actually reachable from an untrusted source. This is the "input agnostic" gap in vuln-regex-detector.
3. **Integrate with APEX findings pipeline** so ReDoS detections are reported uniformly alongside memory-safety and logic findings — vuln-regex-detector is a separate tool with its own output format.
4. **Reuse the ensemble pattern** — APEX's detector should run multiple pattern analysers (rxxr2, Rathnayake-Thielecke-style, regex engine static analysis) and take the union, just like Davis's tool.
