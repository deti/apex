---
id: 01KNZ68KB81AGB3MWRNS87EKXK
title: pg_stat_statements and EXPLAIN ANALYZE — Slow Queries as Test Generation Seeds
type: literature
tags: [postgres, pg-stat-statements, explain-analyze, slow-query, database-performance, test-generation, hot-path]
links:
  - target: 01KNZ68K8H5MWD58PP5KVHD6QF
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:13:15.880802+00:00
modified: 2026-04-11T21:13:15.880808+00:00
source: "https://www.postgresql.org/docs/current/pgstatstatements.html"
---

# pg_stat_statements and EXPLAIN ANALYZE — Turning Slow Queries Into Performance Tests

## pg_stat_statements — the canonical Postgres slow-query aggregator

pg_stat_statements is a contrib extension to PostgreSQL (enabled via `shared_preload_libraries`) that tracks per-query execution statistics, aggregated by a **canonicalised form** of the SQL. Two queries that differ only in constants (e.g., `SELECT * FROM orders WHERE id = 42` vs. `WHERE id = 43`) are grouped into one entry.

Statistics per canonicalised query include:

- `calls`: total execution count
- `total_exec_time`: cumulative execution time in ms (was `total_time` before PG 13)
- `mean_exec_time`, `min_exec_time`, `max_exec_time`, `stddev_exec_time`
- `rows`: total rows returned/affected
- `shared_blks_hit`, `shared_blks_read`: cache hit vs. disk read block counts
- `shared_blks_dirtied`, `shared_blks_written`
- WAL statistics (PG 13+)

Canonical query to find the top slow queries:

```sql
SELECT
  query,
  calls,
  total_exec_time,
  mean_exec_time,
  rows,
  100.0 * shared_blks_hit / nullif(shared_blks_hit + shared_blks_read, 0) AS cache_hit_ratio
FROM pg_stat_statements
ORDER BY total_exec_time DESC
LIMIT 20;
```

This view is the starting point of essentially every Postgres performance-tuning session. Alternative views rank by `mean_exec_time` ("which query is individually slow") vs. `total_exec_time` ("which query consumes the most total DB time"). For perf-test generation, `total_exec_time` is usually the better axis — it tells you which queries matter at the production volume.

## EXPLAIN ANALYZE — per-query plan and real runtime

`EXPLAIN ANALYZE` runs the query and reports the executed plan with actual row counts and timings. Complements pg_stat_statements: the extension tells you *which* queries are slow aggregated, EXPLAIN ANALYZE tells you *why* a specific query is slow.

Key plan features to look for:

- **Seq Scan on a large table.** Missing index.
- **Nested Loop with inner Seq Scan.** Missing index on the join column, O(n×m) join cost.
- **Rows: 100000 planned, Rows: 1000000 actual.** Statistics drift; planner is working with bad estimates.
- **Buffers: read=... > 0.** The query missed the buffer cache. Heavy disk I/O.
- **Heap Fetches > 0.** Index scan is fetching from heap, index-only didn't work.

## From slow query to test case

The pipeline:

1. **Periodically sample pg_stat_statements** (e.g., snapshot every hour via pg_cron or an external agent).
2. **Rank queries** by `total_exec_time` delta (new load since last snapshot).
3. **Attribute each query to an HTTP endpoint.** This is the hard step. Options:
   - ORM query-source logs (Rails `ActiveRecord::LogSubscriber`, Django `django-debug-toolbar`).
   - APM trace context tags — Datadog/Elastic attach the trace ID to queries, which gives you per-endpoint query breakdown.
   - Comment-injection pattern: the application annotates every query with a `/*app:service, endpoint:/orders*/` comment. Sqlcommenter is the standard library.
4. **Generate a test** that drives that endpoint with parameter ranges that maximise the query's result-set size (e.g., a wildcard search instead of an exact-match, a date range that spans more rows).
5. **Run the test and compare** the endpoint's latency to a recent baseline. A regression in the endpoint's p95 where the underlying query's `total_exec_time` has also grown is a high-confidence regression.

## Failure modes of the pipeline

1. **Query-to-endpoint attribution is brittle.** Without sqlcommenter or APM trace context, you're guessing. ORMs emit queries from many code paths and the "which handler called this" question is not always answerable.
2. **Parameterisation blindness.** pg_stat_statements aggregates over parameter values. The "hot" query may only be hot for *specific* parameter values (a missing-cache-entry pattern where most calls hit the cache but one misses and is catastrophically slow). The aggregate doesn't show this.
3. **Data distribution drift.** Fitting a test from today's pg_stat_statements against a staging database with last week's data gives a test that hits a different plan. Staging data volume is almost always a fraction of production.
4. **Auto-vacuum and bloat effects.** A query that was slow an hour ago is fast now because auto-vacuum ran. The statistics are chronologically messy.
5. **Read vs. write skew.** pg_stat_statements treats reads and writes symmetrically. A slow `INSERT` with a trigger that scans another table has a different performance profile than a slow `SELECT`.

## Tooling gap

No open-source tool closes the "slow query → load test" loop. Engineers do it manually: read pg_stat_statements, guess which endpoint is involved, write a targeted test, run it, rinse, repeat. The manual process is one of the highest-leverage perf debugging workflows there is, and automating even part of it (attribution + test scaffolding) would be enormously valuable.

A sane architecture:

- Sqlcommenter on every application to annotate queries with endpoint tags.
- Periodic pg_stat_statements snapshots to a time-series store.
- A cron job that detects top-N queries by delta and, using the sqlcommenter-injected tag, creates a k6/Gatling test scaffold per endpoint.
- An LLM step to propose parameter values that would maximise query cost, given the query plan and schema.

This is a three-week project for a team that already has sqlcommenter and pg_stat_statements in place, and the payoff is a permanent pipeline for DB perf regression.

## Relation to the rest of the vault

- Perffuzz / SlowFuzz (already present) operate at the function/input level. This note is the DB-query analog.
- CBMG session mining (new) gives realistic user workloads. pg_stat_statements gives the *consequence* of workloads — which queries suffer. Together they let you target-generate tests for the query most damaging under the realistic workload.

## Citations

- pg_stat_statements docs: https://www.postgresql.org/docs/current/pgstatstatements.html
- Aiven guide: https://aiven.io/docs/products/postgresql/howto/identify-pg-slow-queries
- Tigerdata (Timescale) guide: https://www.tigerdata.com/blog/using-pg-stat-statements-to-optimize-queries
- Supabase guide: https://supabase.com/docs/guides/database/extensions/pg_stat_statements
- Cybertec query-performance post: https://www.cybertec-postgresql.com/en/postgresql-detecting-slow-queries-quickly/
- Sqlcommenter (Google/OpenCensus): https://google.github.io/sqlcommenter/
- EXPLAIN documentation: https://www.postgresql.org/docs/current/sql-explain.html