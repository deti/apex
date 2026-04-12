---
id: 01KNZ4RPD3AV67Q8W1D6G6EW12
title: "MOpt-AFL repository (puppet-meteor/MOpt-AFL)"
type: tool
tags: [tool, fuzzing, mutation, pso, afl, mopt, repo, github]
links:
  - target: 01KNZ4RPCS4PA5RHBWKW08X4G8
    type: extends
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: related
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/puppet-meteor/MOpt-AFL"
---

# MOpt-AFL (puppet-meteor/MOpt-AFL)

**Repository:** https://github.com/puppet-meteor/MOpt-AFL
**Mirror:** https://github.com/Microsvuln/MOpt-AFL
**Canonical paper:** Lyu et al., USENIX Security 2019 — see `01KNZ4RPCS4PA5RHBWKW08X4G8`
**Base fuzzer:** AFL 2.52b.

*Source: https://github.com/puppet-meteor/MOpt-AFL (README) — fetched 2026-04-12.*

## What it is

MOpt-AFL is the **reference implementation** of the MOPT paper, released by the authors as a fork of the original AFL 2.52b with the Particle Swarm Optimization (PSO) scheduler wired into the havoc mutation stage. The repository is the canonical source for reproducing the paper's benchmark numbers and for running MOPT against bespoke targets that have not yet been integrated into downstream forks.

The artifact exists in parallel with the **AFL++ integration** of MOPT (via `afl-fuzz -L 1`), which is the version most practitioners actually run today. `puppet-meteor/MOpt-AFL` remains the reference for researchers comparing MOPT to baselines in a controlled environment.

## Key implementation notes from the README

- **Command-line option `-L t`** controls the PSO pacemaker mode. `-L 0` activates MOPT immediately; `-L 1` waits for one minute of no new path/crash before switching the mutation scheduler to PSO-optimised weights; `-L t` for larger `t` waits `t` minutes.
- **Two operator sets.** MOPT maintains separate PSO swarms for the deterministic mutation stage and for the havoc mutation stage, because the two stages have very different operator vocabularies. Each swarm evolves independently.
- **Period parameters.** Two knobs control the PSO update frequency: `period_pilot` (duration of the pilot evaluation phase per swarm) and `period_core` (duration of the core exploitation phase). Defaults are chosen to match the paper's benchmark settings.
- **Compatibility.** The MOPT patch is AFL-compatible; instrumentation, corpus format, and output directory structure are unchanged from AFL 2.52b.
- **Installation.** `make`, same as vanilla AFL. The compiled binary is `afl-fuzz` with a modified main loop.

## Reported numbers (from README / reproduced from paper)

The README reports 24-hour fuzzing runs on three canonical AFL targets, comparing MOpt-AFL `-L 1` against baseline AFL:

| Target | AFL paths | MOpt-AFL `-L 1` paths | Speedup |
|---|---|---|---|
| infotocap | 1,821 | 3,983 | 2.2× |
| objdump | 1,099 | 5,499 | 5.0× |
| sqlite3 | 4,949 | 9,975 | 2.0× |

Crash/bug numbers from the paper itself: **170% more crashes** than AFL and **350% more unique bugs**, aggregated across 13 open-source programs.

## Status and maintenance

As of 2026 the `puppet-meteor/MOpt-AFL` repository is mostly dormant — it has not seen major updates since the 2019 paper — but remains in a buildable state for reproduction purposes. Active development of MOPT-style mutation scheduling happens inside the **AFL++** codebase, where the PSO scheduler is continuously maintained and tested against the FuzzBench benchmark. The mirror at `Microsvuln/MOpt-AFL` contains a handful of minor build fixes for newer LLVM versions.

## Relationship to AFL++

When running AFL++ with `-L <t>`, the user is effectively running the MOPT algorithm as described in the paper, but on AFL++'s more modern feedback map (NeverZero counters, context-sensitive edges, CMPLOG) and with AFL++'s newer mutation operators. In practice this is strictly better than `puppet-meteor/MOpt-AFL` for real-world targets, and the AFL++ integration is what APEX should benchmark against.

## Relevance to APEX G-46

1. **Reproduction target.** If APEX's evaluation compares its own mutation scheduler to MOPT, `puppet-meteor/MOpt-AFL` is the reference point for the *paper's* results; AFL++ with `-L 1` is the reference point for the *modern* state of the art.
2. **Code reading.** The `puppet-meteor` fork is useful as a compact, focused reference for the PSO scheduling logic itself, without the surrounding complexity of AFL++.
3. **Do not deploy.** For production fuzzing campaigns, use AFL++'s built-in MOPT integration rather than the original fork, which lacks the downstream bug fixes and modern instrumentation modes.

## References

- Repository — [github.com/puppet-meteor/MOpt-AFL](https://github.com/puppet-meteor/MOpt-AFL)
- Mirror — [github.com/Microsvuln/MOpt-AFL](https://github.com/Microsvuln/MOpt-AFL)
- Canonical paper — see `01KNZ4RPCS4PA5RHBWKW08X4G8`
- AFL++ (downstream integration) — see `01KNZ2ZDMEPBXSH02HFWYAKFE4`
