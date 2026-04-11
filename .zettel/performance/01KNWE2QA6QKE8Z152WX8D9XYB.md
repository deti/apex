---
id: 01KNWE2QA6QKE8Z152WX8D9XYB
title: Resource Exhaustion Patterns (CWE-400 Family)
type: concept
tags: [cwe-400, dos, resource-exhaustion, memory-leak, xml-bomb, fd-exhaustion]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: related
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: related
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5FCBGJCSJJ3XPH1H1DG
    type: references
  - target: 01KNWGA5FEAC0QN3PK6CAYP7T8
    type: references
  - target: 01KNWGA5FRK4HKHP4ZX35ZZ9FB
    type: references
  - target: 01KNWGA5FYGP8SMYTVYNRY8W62
    type: references
  - target: 01KNWGA5G0GB0F6EZHMWYQW7MP
    type: related
  - target: 01KNWGA5GJ1VC19DXXAX3SS947
    type: related
created: 2026-04-10
modified: 2026-04-10
---

# Resource Exhaustion Patterns (CWE-400 Family)

**CWE-400 — Uncontrolled Resource Consumption** is the umbrella weakness for "code that allocates, processes, or holds a resource without bounding how much it can consume." It debuted in the **2024 CWE Top 25 at rank 24**, reflecting both the prevalence of the bug and the fact that defenders have become better at measuring impact.

This note collects the most common runtime patterns that manifest as CWE-400, with fix sketches and detection signals. APEX's G-46 performance test generation feature aims to produce inputs that surface these at test time.

## Pattern catalogue

### 1. Memory leak under load
Memory allocated on a hot path but only freed on the *success* branch of an error path. A single request appears to work; a stream of errors accumulates leaked allocations.

- **Signal**: RSS / heap grows monotonically in a stress loop while the functional workload is constant.
- **Example**: C code that `malloc`s a buffer, returns `-1` on parse failure without `free`.
- **Fix**: RAII / deferred cleanup (`goto cleanup:`, `defer`, `try/finally`, RAII destructors).

### 2. File-descriptor / socket exhaustion
Handles opened but never closed when the caller takes an error path, or held for the lifetime of a request pool that never tops up.

- **Signal**: `ulimit -n` hit under benign load; `EMFILE` or `ENFILE` errors in logs.
- **Example**: Python `open()` without context manager; Java `FileInputStream` without try-with-resources.
- **Fix**: RAII, linters (pylint `consider-using-with`, sonar `S2095`).

### 3. Connection-pool starvation
All pooled connections held open by slow or stuck requests; new requests queue or fail.

- **Signal**: request latency rises sharply once the pool saturates; pool metrics show `inUse == max`.
- **Example**: HikariCP pool exhausted by long-running transactions that never commit.
- **Fix**: per-connection timeouts, circuit breakers, bulkhead isolation (Hystrix/Resilience4j pattern), separate pools for fast vs. slow paths.

### 4. XML entity expansion ("billion laughs")
An XML document defines nested entity references that expand exponentially when resolved: `&lol;` → 10× `&lol2;` → 100× `&lol3;` → ... → ~3 billion `lol`s in a 1 KB input.

- **Signal**: XML parser takes seconds on a KB-scale input; process OOMs.
- **Example**: 2003 `xmlsoft` libxml2; 2010s Python `xml.etree` default config.
- **Fix**: disable DTD / entity expansion (`XMLConstants.FEATURE_SECURE_PROCESSING`, `defusedxml` in Python), or set entity limits.

### 5. Zip bomb / decompression bomb
A tiny compressed archive that expands to an astronomical size (canonical: 42.zip → 4.5 PB).

- **Signal**: decompression throughput > 10x input size with no bound.
- **Fix**: cap decompressed size before and during decompression; detect by streaming a quota and aborting when exceeded.

### 6. Recursive data-structure expansion
JSON arrays nested to depth 10⁴, YAML anchors (`&anchor [*anchor, *anchor]`), protobuf deeply nested messages. Parser recursion → stack overflow; or parser builds a linearised structure → OOM.

- **Signal**: stack overflow on parse; parse time super-linear in input size.
- **Fix**: non-recursive (iterative) parsers, depth limits, per-parse byte budgets. Go standard library `encoding/json` has `UnmarshalJSON` depth limits since 1.15.

### 7. Quadratic string accumulation
```python
result = ""
for chunk in chunks:
    result += chunk    # O(n) copy each iteration → O(n²) total
```
Classic quadratic in immutable-string languages (Python, Java `String`, JavaScript).

- **Signal**: parse time scales as n² where n is number of chunks.
- **Fix**: `"".join(chunks)`, `StringBuilder`, `bytes.Buffer`, array-join pattern.

### 8. Unbounded queue / channel growth
Producer outpaces consumer; intermediate queue grows without bound, eventually OOMs.

- **Signal**: heap histogram dominated by queue node class.
- **Fix**: bounded queues with back-pressure (Go buffered channels, Kafka backpressure, reactive streams).

### 9. Slowloris — long-lived requests
Client opens a connection and feeds data at one byte per minute, holding a worker thread. At 256 workers, 256 attackers block the whole server.

- **Signal**: worker threads all in `read(client_socket)` wait.
- **Fix**: read timeouts, header-receive deadlines, non-blocking I/O with per-request deadlines (nginx's default defence).

### 10. Regex / parser catastrophic backtracking
See ReDoS note. Not strictly "allocation" but a CPU exhaustion variant.

### 11. Thread-pool starvation
A single synchronous I/O call holds a worker thread; at N workers, N concurrent slow calls block all progress.

- **Signal**: CPU idle but throughput zero; all workers in blocking syscall.
- **Fix**: async I/O, separate thread pools by expected latency class.

### 12. Cache unbounded growth
An in-process cache without eviction policy; cache keys grow unbounded with unique URLs / IDs.

- **Signal**: RSS grows linearly with time / unique requests.
- **Fix**: LRU with fixed size, TTL eviction, size-based eviction (Caffeine, Guava CacheBuilder).

## Detection signals APEX can use

A practical resource-exhaustion detector needs to correlate *multiple* signals:

- **Allocation growth per request** — monotonic under load ⇒ leak.
- **Peak RSS vs. input size** — super-linear ⇒ XML-bomb / recursive-expansion class.
- **Execution time vs. input size** — super-linear ⇒ complexity attack class.
- **Handle count over time** — monotonic ⇒ FD leak.
- **Queue depth time series** — unbounded growth ⇒ pool starvation / backpressure miss.

The G-46 spec mandates at least the first three. APEX can surface findings automatically once resource profiling is integrated into the test runner.

## References

- MITRE CWE-400 — [cwe.mitre.org/data/definitions/400.html](https://cwe.mitre.org/data/definitions/400.html)
- MITRE/CISA 2024 CWE Top 25 — [cwe.mitre.org/top25/archive/2024/2024_cwe_top25.html](https://cwe.mitre.org/top25/archive/2024/2024_cwe_top25.html)
- OWASP "Denial of Service Cheat Sheet" — [cheatsheetseries.owasp.org/cheatsheets/Denial_of_Service_Cheat_Sheet.html](https://cheatsheetseries.owasp.org/cheatsheets/Denial_of_Service_Cheat_Sheet.html)
- defusedxml — [github.com/tiran/defusedxml](https://github.com/tiran/defusedxml)
- "Billion laughs attack" — [en.wikipedia.org/wiki/Billion_laughs_attack](https://en.wikipedia.org/wiki/Billion_laughs_attack)
