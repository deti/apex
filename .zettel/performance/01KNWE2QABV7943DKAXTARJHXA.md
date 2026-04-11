---
id: 01KNWE2QABV7943DKAXTARJHXA
title: Quadratic Accumulation and the Schlemiel-the-Painter Anti-pattern
type: concept
tags: [quadratic, accumulation, strings, python, java, performance-bug]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: related
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5FCBGJCSJJ3XPH1H1DG
    type: references
  - target: 01KNWGA5G5JNAQP0QEYZXN6T2H
    type: supports
  - target: 01KNZ2ZDM1XWWE840ADNCGKBMP
    type: references
created: 2026-04-10
modified: 2026-04-12
---

# Quadratic Accumulation and the Schlemiel-the-Painter Anti-pattern

**Quadratic accumulation** is a class of performance bug in which operations that look constant-time actually take time proportional to the result's current size, turning a linear loop into O(n²) work. Joel Spolsky christened the pattern "Schlemiel the Painter's Algorithm" after the folk-tale painter who walks back to his can of paint for every brushstroke, walking farther each time.

## The canonical example: immutable string concatenation

In a language where strings are immutable, each `s = s + chunk` allocates a new string whose length is `len(s) + len(chunk)` and copies the old characters in. Over `n` iterations with chunks of constant size `k`, the total work is:

```
k + 2k + 3k + ... + nk  =  O(n²·k)
```

This is the most common real-world quadratic bug in modern code. Cases APEX should catch:

### Python

```python
# Quadratic
result = ""
for chunk in chunks:
    result += chunk

# Linear
result = "".join(chunks)
```

CPython has a best-effort in-place optimisation for `s += t` *when `s` is not shared*, so naive benchmarks on small inputs sometimes look linear. But it silently degrades to quadratic the moment the string escapes the frame, gets captured, or hits reference-counting thresholds — hidden cliff.

### Java (pre-Java 5 or explicit `String.concat`)

```java
// Quadratic
String result = "";
for (String chunk : chunks) {
    result += chunk;  // desugars to new StringBuilder() per iteration since Java 5 — but inside a loop, each iteration creates a new builder
}

// Linear
StringBuilder sb = new StringBuilder();
for (String chunk : chunks) {
    sb.append(chunk);
}
String result = sb.toString();
```

Java 5+ desugars `a + b` for `String` to `new StringBuilder().append(a).append(b).toString()`, which looks linear at the use site but remains quadratic when the concat is inside a loop because the compiler cannot hoist the builder out of the loop.

### JavaScript

```javascript
// Engine-dependent. V8 used to detect the pattern and switch to a "cons string" rope
// representation, but large ropes get flattened on access, reintroducing quadratic work.
let s = "";
for (const c of chunks) s += c;

// Always linear
const s = chunks.join("");
```

### Go

```go
// Quadratic
var s string
for _, c := range chunks {
    s += c
}

// Linear
var b strings.Builder
for _, c := range chunks {
    b.WriteString(c)
}
s := b.String()
```

## Beyond strings

The anti-pattern is not string-specific. It appears whenever an operation's cost grows with the accumulated result and the programmer assumes it's constant:

### Repeated slicing / array concatenation

```python
result = []
for item in source:
    result = result + [item]  # O(n) per iteration → O(n²)
```

### List-to-string with growing separator

```python
s = ""
for item in items:
    s = s + ", " + str(item)
```

### Bytes buffer without `BytesIO`

```python
buf = b""
while chunk := stream.read(4096):
    buf += chunk  # O(n²) when total bytes large
```

### SQL in a loop

```python
for user_id in user_ids:
    rows = db.execute("SELECT * FROM orders WHERE user_id = ?", user_id)
    all_orders.extend(rows)
```

Each query is O(log n) on a B-tree index, so `n` queries is O(n log n) network + CPU — acceptable? No: the **round-trip latency** dominates. A batch query (`WHERE user_id IN (...)`) is one round-trip; the loop is `n` round-trips. Not quadratic, but same "repeat a cheap-looking operation a lot" shape.

### Python's `list.insert(0, x)`

Inserting at index 0 is O(n). A loop that prepends `n` items is O(n²). Use `collections.deque` instead.

### Nested loops with data-dependent inner bound

```python
for i, a in enumerate(items):
    for b in items[i+1:]:  # slice allocates!
        pairs.append((a, b))
```

The slice `items[i+1:]` is O(n) itself, so this is O(n³), not O(n²).

### XML/HTML attribute lookup in a loop

`element.get("attr")` on some DOM libraries is O(number of attributes) because attributes are stored as an unsorted list. Looking up every attribute in a loop is O(k²) for k attributes.

## Why compilers/interpreters rarely catch it

- The cost is **amortised and hidden** behind an operator (`+`, `+=`, `*`, `[i:]`, `.append`, `.insert`). The source code looks linear.
- Detection requires **whole-loop** analysis: recognising that a growing value is reused as an input to an operation whose cost scales with its size.
- Some runtimes (CPython, V8) have **best-effort special-cases** that hide the bug in microbenchmarks. Lint rules (`pylint R5501` for string concat in loop, ruff `PLW3301`, sonar `java:S1643`) catch a handful of known patterns but miss the general shape.

## Detection signals for APEX

1. **Empirical complexity estimation** — if an "obviously linear" function fits O(n²) in empirical benchmarks, flag it. This is the general-case detector.
2. **Static pattern matching** — `+=` inside a loop where the LHS is a string/list/bytes; `insert(0, ...)` in a loop; slice-based copying in a loop. Cheap, high-precision.
3. **Per-iteration profiling** — record wall-clock time per loop iteration; a linear trend in per-iteration time is the telltale signature.
4. **Allocation-count regression** — `n` iterations producing a linear-in-n accumulator should allocate O(n), not O(n²). Count allocations; flag super-linear.

## Representative real-world incidents

- **Moment.js early versions** — duration parsing via regex + concat in loop. Took milliseconds on normal input, seconds on 100-element durations.
- **Python `StringIO` vs `+=`** — countless web-scraping scripts made this mistake; trivially fixed with `"".join`.
- **Java 6 `String.concat` in Spring template rendering** — fixed in Spring 3.x by switching to StringBuilder.
- **Ruby ERB output buffers pre-2.0** — fixed with dedicated String output buffer class.
- **Go `fmt.Sprintf` in a loop** — re-allocates; `strings.Builder` is linear.

## Fix patterns

- **Build once, emit once** — accumulate into a builder/buffer, materialise once at the end.
- **Join with a separator utility** — `", ".join(items)` in Python, `strings.Join` in Go, `String.join` in Java 8.
- **Pre-size** — if you know the final size, allocate once (`bytearray(total_size)`, `make([]byte, 0, total_size)`).
- **Use linked-structure accumulators** — `collections.deque` for order-preserving FIFO growth, linked lists when order doesn't matter and iteration is the hot path.

## References

- Joel Spolsky — "Back to Basics" — [joelonsoftware.com/2001/12/11/back-to-basics](https://www.joelonsoftware.com/2001/12/11/back-to-basics/) (origin of the Schlemiel the Painter term)
- Python docs — "String concatenation" performance notes
- Java Language Specification — `String` + operator desugaring rules
- ruff PLW3301, pylint R5501 — static detection rules
