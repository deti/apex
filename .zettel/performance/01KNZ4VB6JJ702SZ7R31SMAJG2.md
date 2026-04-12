---
id: 01KNZ4VB6JJ702SZ7R31SMAJG2
title: "Volume Testing — Behaviour at Large Data Volumes"
type: concept
tags: [volume-testing, taxonomy, performance-testing, data-volume, database, meier-2007]
links:
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6J29PS11RZNH5K0E47
    type: related
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "ISO/IEC/IEEE 29119-4 — Software and systems engineering — Software testing — Test techniques; Meier 2007 Ch. 2 Additional Concepts"
---

# Volume Testing — Behaviour at Large Data Volumes

## Question answered

*"Does the system still meet its SLOs when the data it operates on is large? Not 'more requests', but 'same requests against a bigger database'?"*

Volume testing fixes *request rate* and varies *stored data volume*. Load testing is the orthogonal: fix data volume and vary request rate. Both are needed because they exercise different bottlenecks and most systems have non-linear performance as a function of data size.

## Why data volume is orthogonal to request rate

Many production incidents are traced back to performance degradation as data volume grew past an invisible threshold:

1. **Index no longer fits in RAM.** A query that ran in 1 ms when the B-tree index was 500 MB takes 50 ms when the index is 50 GB. Request rate is unchanged. Symptom: latency regressions without load regressions.

2. **Full-table scan that "was fine in dev".** A missing index doesn't bite when the table has 10 000 rows. At 10 000 000 rows it makes the query 1000x slower.

3. **Hash join that spills to disk.** Below a row-count threshold, Postgres / MySQL / Spark does an in-memory hash join; above it, it spills to disk. 10x latency jump at the threshold.

4. **Bloom filter false-positive rate.** A Cassandra bloom filter designed for 1 M keys but fed 100 M degrades to 100 % false positive, making bloom filter useless.

5. **Compaction backlog.** LSM stores (Cassandra, RocksDB, LevelDB) have per-SSTable compaction work proportional to data size. Beyond a capacity threshold, compaction can never catch up; read amplification increases indefinitely.

6. **Linear scan in an application log.** A "find the last N errors" feature that reads the entire log is fine at 10 MB, catastrophic at 10 GB.

7. **GC survivor-space overflow.** JVM heap sized for current dataset; dataset grew 3x; survivor space overflows; full GCs start dominating.

8. **Index rebuild or VACUUM taking longer than the maintenance window.** At 100 GB, the nightly VACUUM takes 2 hours. At 10 TB, 20 hours. The window is only 6 hours. Subsequent nights can't catch up. Storage bloat forever.

## Volume testing is not "ingesting a lot of data"

A volume test is *not* about pushing data in. It is about *querying against* a large existing dataset. Ingest performance is a separate concern (throughput of writes); volume testing fixes the data at some size and then runs the normal workload.

Typical setup:

1. Seed the database with N rows (or M GB) of realistic data. Realism matters: cardinality, distribution of foreign keys, hot vs cold rows.
2. Run the normal production workload at normal rate.
3. Measure latency and resource usage.
4. Repeat at N × 10, N × 100, and a projected future N.
5. Plot latency vs data size to identify thresholds.

## Canonical definition

Meier et al. 2007 place volume-related testing under capacity testing in their "additional concepts" section. ISO/IEC/IEEE 29119-4 defines volume testing as:

> *Testing in which the behaviour of a test item is evaluated for its ability to handle specific volumes of data, either in terms of data throughput or data storage capacity.*

ISTQB glossary similarly: *"Testing where the system is subjected to large volumes of data."*

Meier et al. distinguish it from capacity testing by the axis of variation: capacity tests vary load at fixed data; volume tests vary data at fixed load. In practice, real tests often vary both, but keeping the axes distinct helps diagnose *which* variable drives an observed slowdown.

## Fit in the SDLC

- **Architecture review.** Estimate future data volume; check feasibility against the intended stack's known limits.
- **Pre-release.** Run volume test at the projected 6–12-month data size to catch "works today, dies in six months" bugs.
- **Scheduled.** Re-run volume tests quarterly as real data grows, before production hits the threshold.
- **Migration.** Before migrating to a larger instance or switching storage engine, run volume tests on the target stack at production data volume.

## Common findings

- Query latency that was p99 = 20 ms at 10 GB becomes p99 = 500 ms at 100 GB.
- Compaction or vacuum duration exceeds the maintenance window.
- Full-text search index rebuild takes hours instead of minutes.
- Backup/restore time exceeds the RTO.
- Disk free space at 70 % during volume test → will hit 90 % within months at real growth rate.

## Anti-patterns

1. **"Dev database is 100 MB, prod is 100 GB, let's test on dev"**. The most common form of volume test: implicit, accidental, and missing the whole point. Fix: a dedicated environment with prod-scale data.

2. **Synthetic data with uniform distribution.** Real data is skewed. A test dataset with 1 M random user IDs tests nothing that 1 k random user IDs doesn't, because the query planner's choices don't change. Fix: seed with real (anonymised) production data when possible, or explicitly skewed synthetic data.

3. **Volume testing without aging the data.** A freshly-loaded table has zero tombstones, no dead tuples, no fragmentation. It performs differently from a 6-month-old table with the same row count. Fix: age the dataset by running the workload against it for long enough to approximate production steady state.

4. **Ignoring cold cache.** Volume tests with a warm buffer pool miss the pain of cold reads. Fix: test a mix of warm-path and cold-path queries, or drop caches before measurement.

5. **Only testing the "current" volume.** If current data is 1 TB and growing 2 TB/year, testing at 1 TB tells you nothing about next year's reality. Fix: test at realistic *future* volumes, not just present.

## Relationship to other test types

- **vs capacity.** Capacity = max load at fixed data. Volume = slowdown at fixed load, varying data.
- **vs soak.** Soak extends over time at fixed data. Some soak failure modes (compaction backlog) are also volume failure modes; if a soak runs long enough it becomes a volume test by accident.
- **vs stress.** Orthogonal. A volume-stressed test adds request-rate stress on top of large data.

## Acceptance criteria

- At data volume D, workload at rate R meets p99 < L.
- Backup time at D < T_backup (fits maintenance window).
- Compaction / VACUUM throughput ≥ ingest rate (catches up within a day).
- Query plan does not degrade across the D → 10D interval (no full-scan emergence).

## References

- Meier et al. — Microsoft p&p Performance Testing Guidance, Ch. 2 "Additional Concepts / Terms".
- ISO/IEC/IEEE 29119-4:2015 — Software testing — Test techniques (volume testing definition).
- ISTQB Glossary — "volume testing" entry.
- Jain, R. — *The Art of Computer Systems Performance Analysis*, section on performance at scale — Wiley 1991.
