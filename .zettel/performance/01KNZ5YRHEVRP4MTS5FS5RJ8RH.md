---
id: 01KNZ5YRHEVRP4MTS5FS5RJ8RH
title: "Java Flight Recorder (JFR) — JDK's built-in event-based continuous profiler"
type: literature
tags: [tool, jfr, java-flight-recorder, profiler, jvm, java, continuous-profiling, openjdk, jmc]
links:
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YRH40QCWBYW7FV6FRHJ2
    type: related
  - target: 01KNWGA5H1MNJK8GWPFCZSSW7E
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:07:53.518076+00:00
modified: 2026-04-11T21:07:53.518078+00:00
---

Source: https://docs.oracle.com/en/java/javase/21/jfapi/ and JEP 328 — Java Flight Recorder documentation, fetched 2026-04-12.

Java Flight Recorder (JFR) is the JDK's built-in continuous profiling and diagnostic event framework. It originated as a JRockit feature (BEA Systems, pre-Oracle), was carried into Oracle JDK as a commercial feature, and was open-sourced to OpenJDK in JDK 11 via JEP 328 (2018). Since JDK 11 it is a free, standard part of every OpenJDK distribution.

## What JFR is

JFR is not a sampling profiler in the traditional sense — it is an **event-based recording framework** where HotSpot and the JDK class library emit typed events for interesting occurrences. Events include:

- **Execution sampling** — periodic method sampling (the closest thing to traditional CPU profiling).
- **GC events** — GC pause start/end, phase transitions, heap size changes, allocation in new TLAB, object allocation outside TLAB.
- **Memory pool events** — committed/used heap, metaspace.
- **Thread events** — park, unpark, sleep, state changes.
- **Compilation events** — method compiled, deoptimized, inlined.
- **Class loading events** — class loaded, unloaded.
- **I/O events** — socket read/write, file read/write.
- **Synchronization events** — monitor wait, monitor enter.
- **Custom events** — applications can define their own event types via `jdk.jfr.Event`.

Each event has a schema (fields with types) and configurable sampling/threshold rules.

## Low-overhead design

JFR is designed to run continuously in production with under 1% overhead under the default configuration. The key design choices:
- **In-JVM ring buffers** — events are written to per-thread buffers and flushed asynchronously. No locks on the hot path.
- **Configurable thresholds** — noisy events (e.g. "allocation outside TLAB") can be filtered to only record events exceeding a size or duration threshold.
- **Lazy serialization** — events are serialized to the .jfr binary format in background threads.

## Configurations

JFR ships two built-in configurations in `$JAVA_HOME/lib/jfr/`:
- **default.jfc** — balanced configuration, < 1% overhead. Suitable for continuous always-on recording in production.
- **profile.jfc** — richer event set (more frequent sampling, more event types enabled), higher overhead (~2-3%), intended for explicit profiling sessions.

Custom configurations are authored in JMC (JDK Mission Control) — an Eclipse-based GUI that edits the XML `.jfc` schema and validates it.

## Usage

Start a recording via command-line flags or jcmd:

```
# At JVM start
java -XX:StartFlightRecording:filename=rec.jfr,dumponexit=true,settings=profile \
     -jar myapp.jar

# Attach to a running JVM
jcmd <pid> JFR.start name=myrec settings=profile
jcmd <pid> JFR.dump name=myrec filename=/tmp/rec.jfr
jcmd <pid> JFR.stop name=myrec
```

The dumped `.jfr` file opens in JMC or is consumed programmatically via the `jdk.jfr.consumer` API.

## JMC (JDK Mission Control)

JMC is the official viewer: an Eclipse-based GUI for analysing JFR recordings. It provides:
- Automated analysis rules ("this code path has a lot of allocation", "this lock is contended").
- Per-thread CPU timelines.
- GC pause visualisation.
- Allocation pressure drill-down by class.
- Lock contention hot spots.

JMC was open-sourced alongside JFR and is now maintained at `github.com/openjdk/jmc`.

## JFR Streaming (JDK 14+)

Later JDKs added `jdk.jfr.consumer.RecordingStream` which lets applications consume JFR events in-process as they are emitted. This enables building custom APM-style monitoring on top of JFR without writing the recording to disk first. It is the foundation for modern Java observability integrations.

## Strengths

- Zero-cost (effectively) always-on in production with default config.
- Richer event semantics than any external profiler — GC phases, TLAB allocations, class loading are all first-class events.
- Part of the JDK — no agent install, no third-party software.
- `.jfr` is a documented format; tooling ecosystem includes JMC, async-profiler (can emit JFR), Datadog, Dynatrace, New Relic, IntelliJ integration.

## Failure modes

- **JMC learning curve is steep** — the automated analysis rules are great but the raw event browser is overwhelming.
- **Some events are disabled by default** even in `profile.jfc` — allocation events in particular can be missing unless explicitly enabled.
- **`.jfr` files are binary** — tooling ecosystem is Java-centric. Non-JDK tools do not natively read JFR without a converter.
- **Vendor-specific events** (Oracle JDK historically had some; OpenJDK does not) can create compatibility issues across distributions.
- **Execution sampling in JFR has the safepoint-bias problem** async-profiler was built to avoid — JFR method sampling also uses safepoint-aligned polling. Use async-profiler's JFR output to combine the two toolchains.

## Relevance to APEX G-46

For JVM targets, JFR is the production observability substrate that APEX's resource-profiling phase would ideally hook into. A G-46 performance test on a Java target should emit a JFR recording as part of its evidence bundle — this makes findings directly actionable by users who already live in the JMC / Datadog / New Relic tooling. JFR's GC-pause and TLAB-allocation events are also the right source for memory-leak-under-load detection.