<!-- status: ACTIVE -->
# Static Detection of Concurrency, Lock-Free, and Fail-Safe Bugs

Research survey for APEX detector development. Covers state of the art, CWE mappings,
detection approaches, and implementation guidance per language.

---

## Table of Contents

1. [Lock-Free / Wait-Free Correctness](#1-lock-free--wait-free-correctness)
2. [Lock-Related Bugs](#2-lock-related-bugs)
3. [Fail-Safe Patterns](#3-fail-safe-patterns)
4. [Adjacent Areas](#4-adjacent-areas)
5. [Tool Landscape Summary](#5-tool-landscape-summary)
6. [Implementation Priority Matrix](#6-implementation-priority-matrix)

---

## 1. Lock-Free / Wait-Free Correctness

### 1.1 ABA Problem Detection

| Field | Value |
|-------|-------|
| **Bug class** | ABA problem in CAS-based algorithms |
| **CWE** | CWE-362 (Race Condition), CWE-825 (Expired Pointer Dereference) |
| **Languages** | C, C++, Rust, Java, Go |
| **Detection approach** | Pattern matching + data flow |
| **False positive rate** | Medium |
| **Existing tools** | None dedicated; CERT CON09-C rule (manual audit); research prototypes only |

**Description:** A compare-and-swap (CAS) operation succeeds because the expected value
matches, but the memory location has been changed from A to B and back to A by another
thread. The CAS "succeeds" despite the semantic state having changed. This corrupts
lock-free data structures (stacks, queues, free lists).

**Source code patterns to detect:**

```
// C/C++: Raw pointer CAS without tagged pointer or hazard pointer
atomic_compare_exchange_*(&head, &expected, new_node)
// where `expected` is a raw pointer and no epoch/hazard-pointer guard is visible

// Rust: AtomicPtr::compare_exchange without crossbeam::epoch or hazard pointers
ptr.compare_exchange(old, new, Ordering::*, Ordering::*)
// where `old` came from a previous load with no epoch::pin() guard

// Java: AtomicReference.compareAndSet without versioned wrapper
ref.compareAndSet(expected, newVal)
// where expected came from a plain get() and the type is reusable/pooled
```

**Detection algorithm for APEX:**
1. Find all CAS operations on pointer/reference types.
2. Check whether the loaded "expected" value is protected by:
   - Epoch-based reclamation (crossbeam::epoch, rcu_read_lock)
   - Hazard pointers
   - Tagged/stamped pointers (AtomicStampedReference in Java)
   - Double-width CAS (DWCAS)
3. If none of these guards are present, flag as potential ABA.

**Language-specific notes:**
- **Java:** `AtomicStampedReference` solves ABA; detect use of plain `AtomicReference`
  on pooled/recycled objects.
- **C/C++:** Look for `__sync_val_compare_and_swap` or `atomic_compare_exchange` on
  pointer types without epoch guards.
- **Rust:** `AtomicPtr` without crossbeam epoch or `arc-swap`.
- **Go:** `atomic.CompareAndSwapPointer` (rare; Go's GC mitigates but doesn't eliminate).

---

### 1.2 Memory Ordering Bugs

| Field | Value |
|-------|-------|
| **Bug class** | Incorrect atomic memory ordering |
| **CWE** | CWE-362, CWE-820 (Missing Synchronization) |
| **Languages** | C, C++, Rust, Go |
| **Detection approach** | Pattern matching + synchronizes-with analysis |
| **False positive rate** | Medium-High |
| **Existing tools** | CDSChecker, GenMC, RCMC (model checkers); Miri (Rust, dynamic); none static |

**Description:** Using `Relaxed` ordering where `Acquire`/`Release` is required, or
using `Acquire` on a store (which is meaningless). Results in reads seeing stale values,
publication protocols failing, or reordering across CPUs.

**Source code patterns to detect:**

```rust
// Anti-pattern 1: Relaxed store used as a "publish" signal
flag.store(true, Ordering::Relaxed);  // BUG: should be Release
// ... other thread ...
if flag.load(Ordering::Acquire) { /* sees data that isn't published */ }

// Anti-pattern 2: Acquire on store (no-op / compile error in Rust but valid in C++)
atomic_store_explicit(&x, val, memory_order_acquire); // C++: compiles, meaningless

// Anti-pattern 3: Relaxed load-then-branch controlling data access
let idx = counter.load(Ordering::Relaxed);
data[idx]  // BUG: no acquire barrier, data may not be visible

// Anti-pattern 4: SeqCst everywhere (correctness ok, but performance smell)
x.store(val, Ordering::SeqCst); // Smell: likely Relaxed or Release suffices
```

**Detection algorithm for APEX:**
1. Build a "synchronizes-with" graph: for each atomic variable, track
   store/load pairs across threads (or async tasks).
2. Flag stores with `Relaxed` that have a corresponding load with `Acquire`
   (mismatch indicates missing Release on store side).
3. Flag `Acquire` used on store operations.
4. Flag `Relaxed` loads whose result controls access to non-atomic data
   (missing acquire barrier).
5. **Performance smell:** Flag `SeqCst` used everywhere (suggest
   Acquire/Release if no total ordering is needed).

**Practical simplification for static analysis:** Since full
synchronizes-with analysis requires inter-procedural thread-aware flow,
a pragmatic approach is pattern-based:
- Flag `Relaxed` on any store that also has a string match to "flag",
  "ready", "published", "init", "done" (semantic publish signals).
- Flag `Relaxed` load immediately followed by array/pointer dereference.

---

### 1.3 Progress Guarantee Violations

| Field | Value |
|-------|-------|
| **Bug class** | Claiming lock-free but actually blocking |
| **CWE** | CWE-833 (Deadlock), CWE-662 (Improper Synchronization) |
| **Languages** | All |
| **Detection approach** | Call graph analysis + pattern matching |
| **False positive rate** | Low-Medium |
| **Existing tools** | No dedicated tool; manual audit |

**Description:** Code marketed or commented as "lock-free" that actually
acquires a mutex, performs a blocking allocation, or makes a system call
that can block.

**Source code patterns to detect:**

```rust
// Function or type documented as lock-free but calls blocking ops
/// Lock-free queue implementation
impl<T> LockFreeQueue<T> {
    fn push(&self, val: T) {
        let node = Box::new(Node::new(val)); // BUG: heap allocation can block
        self.lock.lock();                     // BUG: mutex in "lock-free" code
    }
}
```

**Detection algorithm for APEX:**
1. Identify functions/types annotated or documented as "lock-free" or "wait-free".
2. Build call graph from those functions.
3. Flag any reachable call to:
   - Mutex/RwLock lock operations
   - Heap allocation (`malloc`, `new`, `Box::new`, `Vec::push`)
   - Blocking I/O (`read`, `write`, `recv`, `send`)
   - System calls that can block (`futex`, `sleep`, `yield`)
   - `println!` / logging (may acquire internal locks)
4. Whitelist: CAS retry loops are acceptable (that's how lock-free works).

**Language-specific blocking operations:**

| Language | Blocking calls to flag |
|----------|----------------------|
| Rust | `Mutex::lock`, `RwLock::*`, `Box::new`, `Vec::push` (alloc), `std::io::*`, `thread::sleep`, `println!` |
| C/C++ | `pthread_mutex_lock`, `malloc`, `new`, `printf`, `read`, `write`, `sleep` |
| Go | `sync.Mutex.Lock`, `make([]T, n)` (large), `fmt.Print*`, channel send/recv |
| Java | `synchronized`, `ReentrantLock.lock`, `new` (GC pressure), `Thread.sleep` |

---

### 1.4 Linearizability Violations

| Field | Value |
|-------|-------|
| **Bug class** | Non-linearizable concurrent operations |
| **CWE** | CWE-362 |
| **Languages** | All |
| **Detection approach** | Model checking (not pattern matching) |
| **False positive rate** | Low (but high analysis cost) |
| **Existing tools** | Line-Up (Microsoft), Relinche (POPL 2025), TMRexp, Violat |

**Description:** A concurrent data structure's operations don't appear to
take effect instantaneously at some point between invocation and response.

**Static detection feasibility:** Very limited. This fundamentally requires
exploring interleavings. Current state of the art uses bounded model checking
(Relinche, Line-Up). Not practical for a fast static analyzer.

**Practical APEX approach:** Instead of checking linearizability directly,
detect common *indicators* of non-linearizability:
1. CAS retry loops that don't re-read shared state after failure.
2. Multi-step updates to shared state without atomic commit (e.g., updating
   `head` and `size` in separate atomic operations).
3. Methods on concurrent containers that compose multiple operations
   non-atomically (check-then-act).

```java
// Classic check-then-act (non-linearizable composition)
if (!concurrentMap.containsKey(key)) {
    concurrentMap.put(key, value);  // BUG: another thread may have inserted
}
// Fix: concurrentMap.putIfAbsent(key, value)
```

---

## 2. Lock-Related Bugs

### 2.1 Deadlock Detection (Lock Ordering)

| Field | Value |
|-------|-------|
| **Bug class** | Deadlock via inconsistent lock ordering |
| **CWE** | CWE-833 (Deadlock) |
| **Languages** | All |
| **Detection approach** | Lock graph cycle detection |
| **False positive rate** | Medium |
| **Existing tools** | Lockbud (Rust), Peahen, DLOS (kernel), RacerX, Infer/starvation, Helgrind (dynamic), tracing-mutex (Rust runtime) |

**Description:** Thread A holds lock X and waits for lock Y; Thread B holds
lock Y and waits for lock X. Classic dining philosophers.

**Detection algorithm:**
1. Build a lock acquisition graph: nodes = lock instances, edges = "acquired while holding".
2. For each function, track which locks are held (GenKill analysis on lock guards).
3. Find cycles in the graph. Each cycle is a potential deadlock.
4. **Rust-specific:** Track `MutexGuard` / `RwLockGuard` lifetime. A lock
   is held until the guard is dropped. Lockbud does exactly this.

**Source code patterns:**

```rust
// Deadlock: inconsistent lock ordering
fn thread_a(m1: &Mutex<()>, m2: &Mutex<()>) {
    let _g1 = m1.lock().unwrap();
    let _g2 = m2.lock().unwrap(); // acquires m2 while holding m1
}
fn thread_b(m1: &Mutex<()>, m2: &Mutex<()>) {
    let _g2 = m2.lock().unwrap();
    let _g1 = m1.lock().unwrap(); // acquires m1 while holding m2 -- DEADLOCK
}
```

```python
# Python: same pattern with threading.Lock
lock_a.acquire()
lock_b.acquire()  # Thread 1
# vs
lock_b.acquire()
lock_a.acquire()  # Thread 2
```

**Refinement strategies (reduce false positives):**
- Gate-lock analysis (Peahen): if a "gate lock" is always held before the
  cycle, the cycle is infeasible.
- Path feasibility: prune cycles where the two code paths cannot execute
  concurrently.
- Async deadlocks: for Rust tokio, track `.await` points as potential
  "lock release" points for async mutex.

---

### 2.2 Double-Lock (Re-entrant Lock on Non-Reentrant Mutex)

| Field | Value |
|-------|-------|
| **Bug class** | Double lock / self-deadlock |
| **CWE** | CWE-764 (Multiple Locks of a Critical Resource) |
| **Languages** | Rust, C/C++, Python, Go |
| **Detection approach** | Intra-procedural + call graph analysis |
| **False positive rate** | Low |
| **Existing tools** | Lockbud (Rust), Clippy (proposed), Helgrind (dynamic) |

**Description:** A thread attempts to lock a mutex it already holds. With
non-reentrant mutexes (Rust's `std::sync::Mutex`, Go's `sync.Mutex`,
`pthread_mutex_t` default), this is an immediate deadlock.

**Detection algorithm:**
1. Track lock guards in scope.
2. If the same lock is acquired again (directly or via function call) while
   the guard is still alive, flag it.
3. Inter-procedural: if function `f` holds lock `L` and calls `g` which also
   locks `L`, flag it.

```rust
let _guard = mutex.lock().unwrap();
// ... some code ...
let _guard2 = mutex.lock().unwrap(); // DEADLOCK: double lock

// Subtler: via method call
fn process(&self) {
    let _g = self.lock.lock().unwrap();
    self.helper(); // calls lock() internally
}
fn helper(&self) {
    let _g = self.lock.lock().unwrap(); // DEADLOCK
}
```

---

### 2.3 Lock Contention Hotspots

| Field | Value |
|-------|-------|
| **Bug class** | Excessive lock hold duration / contention |
| **CWE** | CWE-662 (Improper Synchronization) |
| **Languages** | All |
| **Detection approach** | Pattern matching + scope analysis |
| **False positive rate** | Medium-High |
| **Existing tools** | No static tools; perf/lockstat (dynamic profiling) |

**Description:** Holding a lock while performing expensive operations
(I/O, allocation, computation) causes all other threads to stall.

**Detection algorithm:**
1. Find lock guard acquisitions.
2. Scan code between acquisition and drop for:
   - I/O operations (file, network, database)
   - Sleep/yield calls
   - Heavy computation (nested loops)
   - Allocation (`Vec::with_capacity`, `HashMap::new`)
3. Flag if expensive operations are found within lock scope.

```rust
let _guard = self.state.lock().unwrap();
let data = std::fs::read_to_string("config.json")?; // BUG: I/O under lock
self.state.config = parse(data);
// Fix: read file outside lock, then acquire lock just for assignment
```

---

### 2.4 Missing Lock Guards (Data Race Potential)

| Field | Value |
|-------|-------|
| **Bug class** | Unsynchronized access to shared mutable state |
| **CWE** | CWE-362, CWE-820 (Missing Synchronization) |
| **Languages** | Java, Python, Go, C/C++ (Rust prevents at compile time for safe code) |
| **Detection approach** | Field access + lock analysis (abstract interpretation) |
| **False positive rate** | Medium |
| **Existing tools** | RacerD (Java), SpotBugs IS2_INCONSISTENT_SYNC (Java), ThreadSanitizer (dynamic), Go race detector (dynamic) |

**Description:** A field is accessed with a lock held in some paths but
without a lock in others. Inconsistent synchronization.

**Detection algorithm (RacerD approach):**
1. For each field, track accesses (read/write) and whether a lock is held.
2. If a field has at least one write access without lock protection AND
   at least one access (read or write) with lock protection, flag
   inconsistent synchronization.
3. Compositional: analyze each method independently, then compose.

```java
class Counter {
    private int count;
    private final Object lock = new Object();

    void increment() {
        synchronized(lock) { count++; }  // synchronized
    }

    int getCount() {
        return count;  // BUG: unsynchronized read
    }
}
```

**Rust note:** Safe Rust prevents this at compile time via the type system
(`Mutex<T>` forces lock acquisition to access `T`). However, `unsafe` code
and `UnsafeCell` bypass this. Detect: `UnsafeCell` accesses without
documented synchronization.

---

### 2.5 Mutex Poisoning Without Recovery

| Field | Value |
|-------|-------|
| **Bug class** | Ignoring or unwrapping poisoned mutex |
| **CWE** | CWE-755 (Improper Handling of Exceptional Conditions) |
| **Languages** | Rust |
| **Detection approach** | Pattern matching |
| **False positive rate** | Low |
| **Existing tools** | Clippy (partial) |

**Description:** In Rust, when a thread panics while holding a `MutexGuard`,
the mutex becomes "poisoned." Calling `.lock().unwrap()` on a poisoned mutex
panics, potentially cascading failures.

**Detection algorithm:**
1. Find all `mutex.lock().unwrap()` patterns.
2. Flag as potential problem; suggest `.lock().unwrap_or_else(|e| e.into_inner())`
   or explicit poison handling.

```rust
// Anti-pattern
let data = shared.lock().unwrap(); // Panics if mutex is poisoned

// Better
let data = shared.lock().unwrap_or_else(|poisoned| {
    log::warn!("Mutex was poisoned, recovering");
    poisoned.into_inner()
});
```

---

### 2.6 RwLock Writer Starvation

| Field | Value |
|-------|-------|
| **Bug class** | Writer starvation with reader-preference RwLock |
| **CWE** | CWE-662 |
| **Languages** | Rust, C/C++, Java, Go |
| **Detection approach** | Pattern matching + usage frequency heuristic |
| **False positive rate** | High |
| **Existing tools** | None |

**Description:** A `RwLock` with reader preference can starve writers if
readers continuously hold the lock. In Rust, `std::sync::RwLock` does not
guarantee fairness; `parking_lot::RwLock` provides writer priority.

**Detection algorithm:**
1. Find `RwLock` declarations.
2. Count read vs write lock acquisitions across codebase.
3. If ratio > 10:1 reads:writes, flag potential writer starvation.
4. Suggest `parking_lot::FairMutex` or writer-preferring `RwLock`.

This is more of a code smell / advisory than a definitive bug detection.

---

## 3. Fail-Safe Patterns

### 3.1 Missing Timeouts on I/O / Network Operations

| Field | Value |
|-------|-------|
| **Bug class** | Blocking I/O without timeout |
| **CWE** | CWE-400 (Uncontrolled Resource Consumption), CWE-835 (Infinite Loop) |
| **Languages** | All |
| **Detection approach** | Pattern matching |
| **False positive rate** | Low |
| **Existing tools** | APEX (existing MissingTimeoutDetector for Python); ESLint rules; custom lints |

**Note:** APEX already has a `MissingTimeoutDetector` in
`crates/apex-detect/src/detectors/timeout.rs` for Python HTTP libraries.

**Expansion needed for multi-language:**

| Language | Patterns to detect |
|----------|-------------------|
| **Python** | `requests.*()` without `timeout=`, `socket.recv()` without `settimeout`, `urllib.urlopen` without timeout, `aiohttp` without `timeout` |
| **JavaScript** | `fetch()` without `AbortController`/timeout, `axios` without `timeout`, `net.Socket` without `setTimeout`, `http.request` without timeout |
| **Go** | `http.Get()` without context deadline, `net.Dial` without timeout, `sql.DB` without `SetConnMaxLifetime`, channel recv without `select`+`time.After` |
| **Rust** | `TcpStream::connect` without `connect_timeout`, `reqwest` without `.timeout()`, `tokio::io` without `tokio::time::timeout` |
| **Java** | `HttpURLConnection` without `setConnectTimeout`/`setReadTimeout`, `Socket` without `setSoTimeout`, JDBC without `setQueryTimeout` |
| **C/C++** | `connect()` without `SO_SNDTIMEO`/`SO_RCVTIMEO`, `recv()` without `select`/`poll` timeout |

---

### 3.2 Unbounded Queues / Channels

| Field | Value |
|-------|-------|
| **Bug class** | Unbounded queue leading to memory exhaustion |
| **CWE** | CWE-400, CWE-770 (Allocation of Resources Without Limits) |
| **Languages** | All |
| **Detection approach** | Pattern matching |
| **False positive rate** | Medium |
| **Existing tools** | None dedicated |

**Source code patterns:**

```rust
// Rust: unbounded channel
let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
let (tx, rx) = std::sync::mpsc::channel(); // std channel is unbounded

// Go: unbounded channel (well, Go channels are bounded, but large buffers)
ch := make(chan Msg, 1_000_000) // suspiciously large buffer

// Python: queue.Queue() without maxsize
q = queue.Queue()  // unbounded by default

// Java: LinkedBlockingQueue without capacity
new LinkedBlockingQueue<>()  // unbounded by default
```

**Detection algorithm:**
1. Find channel/queue creation.
2. Check if a capacity/bound is specified.
3. Flag unbounded channels in production code (not tests).
4. Suggest bounded alternatives with backpressure.

---

### 3.3 Missing Circuit Breakers in Retry Loops

| Field | Value |
|-------|-------|
| **Bug class** | Infinite/excessive retry without backoff or limit |
| **CWE** | CWE-400, CWE-835 (Infinite Loop) |
| **Languages** | All |
| **Detection approach** | Pattern matching + loop analysis |
| **False positive rate** | Medium |
| **Existing tools** | None dedicated |

**Source code patterns:**

```python
# Anti-pattern: retry forever without backoff
while True:
    try:
        response = requests.get(url)
        break
    except:
        pass  # BUG: infinite retry, no backoff, no limit, bare except

# Also detect: loop { retry } without max_retries counter
```

**Detection algorithm:**
1. Find loop patterns containing network/I/O calls inside try/catch.
2. Check for:
   - Max retry counter (variable that decrements or `for i in range(N)`)
   - Exponential backoff (`sleep(delay * 2)`, `sleep(2**attempt)`)
   - Circuit breaker library usage
3. Flag loops missing all three safeguards.

---

### 3.4 Resource Leak Detection

| Field | Value |
|-------|-------|
| **Bug class** | Leaked file descriptors, sockets, temp files |
| **CWE** | CWE-772 (Missing Release of Resource), CWE-404 (Improper Resource Shutdown), CWE-775 (Missing FD Release) |
| **Languages** | All (Rust RAII helps but not immune) |
| **Detection approach** | Data flow / lifetime analysis |
| **False positive rate** | Medium |
| **Existing tools** | Infer/Pulse (Java, C), GCC -fanalyzer (C), SpotBugs (Java), Clippy (partial, Rust), RLFixer (repair) |

**Source code patterns:**

```python
# Python: file opened without context manager
f = open("data.txt")
data = f.read()
# f.close() missing -- leaked if exception occurs between open and close

# Fix:
with open("data.txt") as f:
    data = f.read()
```

```go
// Go: HTTP response body not closed
resp, err := http.Get(url)
if err != nil { return err }
// resp.Body.Close() missing -- BUG: leaked connection
```

```java
// Java: stream not closed in finally/try-with-resources
InputStream is = new FileInputStream("file.txt");
byte[] data = is.readAllBytes();
// is.close() missing
```

**Detection algorithm:**
1. Track resource-acquiring calls (`open`, `socket`, `connect`, `File::open`).
2. Follow the returned handle through the function.
3. Check that every exit path (including exceptions/errors) closes the resource.
4. For languages with RAII (Rust, C++ with smart pointers): check that the
   handle isn't leaked via `mem::forget`, `ManuallyDrop`, or `.into_raw_fd()`.

---

### 3.5 Missing Graceful Shutdown Handlers

| Field | Value |
|-------|-------|
| **Bug class** | No SIGTERM/SIGINT handler; abrupt termination |
| **CWE** | CWE-404 (Improper Resource Shutdown) |
| **Languages** | All server/daemon code |
| **Detection approach** | Pattern matching on binary/server entry points |
| **False positive rate** | Medium-High |
| **Existing tools** | None dedicated |

**Detection algorithm:**
1. Identify server/daemon binaries (presence of `listen()`, `bind()`,
   `serve()`, web framework setup).
2. Search for signal handler registration:
   - Rust: `ctrlc::set_handler`, `tokio::signal`, `signal_hook`
   - Go: `signal.Notify(ch, syscall.SIGTERM)`
   - Python: `signal.signal(signal.SIGTERM, handler)`
   - Node.js: `process.on('SIGTERM', handler)`
   - Java: `Runtime.addShutdownHook`
   - C: `signal(SIGTERM, handler)` or `sigaction`
3. If server code has no signal handler, flag as missing graceful shutdown.

---

### 3.6 Panic-in-Drop (Unwinding Through Destructors)

| Field | Value |
|-------|-------|
| **Bug class** | Panic during stack unwinding causes abort |
| **CWE** | CWE-755 (Improper Handling of Exceptional Conditions) |
| **Languages** | Rust, C++ |
| **Detection approach** | AST/pattern matching |
| **False positive rate** | Low |
| **Existing tools** | Clippy (no dedicated lint); Miri (dynamic); RFC 3288 proposed banning |

**Description:** If `drop()` panics while the stack is already unwinding from
another panic, the process aborts. In C++, throwing from a destructor during
unwinding calls `std::terminate`.

**Detection algorithm:**
1. Find all `impl Drop for T` blocks.
2. Inside the `drop(&mut self)` method, flag:
   - Direct `panic!()`, `unwrap()`, `expect()`, `todo!()`, `unimplemented!()`
   - Calls to functions that may panic (heuristic: functions with `unwrap()`
     in their body)
   - `assert!()` macros
3. Suggest using `std::thread::panicking()` to check before panicking,
   or converting operations to fallible alternatives.

```rust
impl Drop for MyResource {
    fn drop(&mut self) {
        self.file.flush().unwrap(); // BUG: can panic during unwind
        // Fix: self.file.flush().ok(); // or log the error
    }
}
```

---

## 4. Adjacent Areas

### 4.1 TOCTOU Race Conditions

| Field | Value |
|-------|-------|
| **Bug class** | Time-of-check time-of-use filesystem/network race |
| **CWE** | CWE-367 |
| **Languages** | All |
| **Detection approach** | Pattern matching (check-then-use pairs) |
| **False positive rate** | Medium |
| **Existing tools** | Polyspace (MATLAB/C), Coverity, PVS-Studio, Flawfinder (C) |

**Source code patterns:**

```python
# Classic TOCTOU
if os.path.exists(filepath):     # CHECK
    with open(filepath) as f:    # USE -- file may have changed/been replaced
        data = f.read()

# Fix: just open and handle FileNotFoundError
```

```c
// C: stat then open
if (stat(path, &sb) == 0) {     // CHECK
    fd = open(path, O_RDONLY);   // USE
}
// Fix: open first, then fstat on the fd
```

```rust
// Rust: same pattern
if Path::new(path).exists() {      // CHECK
    let data = std::fs::read(path)?; // USE
}
```

**Detection algorithm:**
1. Find filesystem check functions: `exists()`, `is_file()`, `is_dir()`,
   `stat()`, `access()`, `os.path.exists`, `Path.exists`.
2. Find filesystem use functions: `open()`, `read()`, `write()`, `remove()`,
   `rename()`, `mkdir()`.
3. If a check on the same path is followed by a use (within same function,
   no lock/atomic-rename between them), flag as TOCTOU.

---

### 4.2 Priority Inversion Detection

| Field | Value |
|-------|-------|
| **Bug class** | High-priority thread blocked by low-priority thread |
| **CWE** | CWE-662 |
| **Languages** | C, C++, Rust (real-time systems) |
| **Detection approach** | Pattern matching + priority annotation analysis |
| **False positive rate** | High |
| **Existing tools** | No dedicated static tools; RTOS analyzers (dynamic) |

**Description:** A high-priority thread waits on a lock held by a low-priority
thread, which is preempted by a medium-priority thread. The high-priority
thread is effectively blocked by the medium-priority one.

**Detection approach (limited):**
1. Identify thread priority assignments (`pthread_setschedparam`, OS-level
   thread priority APIs).
2. If threads with different priorities share a mutex and the mutex is NOT
   configured with priority inheritance (`PTHREAD_PRIO_INHERIT`), flag it.
3. This is mostly relevant for embedded/RTOS code.

---

### 4.3 Signal Safety Violations

| Field | Value |
|-------|-------|
| **Bug class** | Calling non-async-signal-safe functions in signal handlers |
| **CWE** | CWE-479 (Signal Handler Use of a Non-Reentrant Function) |
| **Languages** | C, C++, Rust, Go |
| **Detection approach** | Call graph analysis from signal handlers |
| **False positive rate** | Low |
| **Existing tools** | asyncsafe (C, LD_PRELOAD dynamic), CERT SIG30-C rule |

**Description:** Signal handlers interrupt normal execution at arbitrary
points. Calling functions that aren't async-signal-safe (e.g., `malloc`,
`printf`, `mutex_lock`) from a signal handler is undefined behavior.

**Detection algorithm:**
1. Identify signal handler registrations: `signal()`, `sigaction()`,
   `ctrlc::set_handler` (Rust), `signal.signal` (Python).
2. Extract the handler function.
3. Build call graph from handler function.
4. Flag any reachable call NOT in the POSIX async-signal-safe list:
   `write`, `_exit`, `signal`, `abort`, `read` (and ~100 others from POSIX).
5. Common violations: `printf`, `malloc`/`free`, `syslog`, `pthread_*`,
   `exit` (vs `_exit`).

```c
void handler(int sig) {
    printf("Caught signal %d\n", sig);  // BUG: printf is not signal-safe
    free(global_ptr);                    // BUG: free is not signal-safe
    _exit(1);                            // OK: _exit IS signal-safe
}
```

---

### 4.4 Starvation Patterns

| Field | Value |
|-------|-------|
| **Bug class** | Busy-wait, spinlock without yield, unfair scheduling |
| **CWE** | CWE-400 (Resource Consumption) |
| **Languages** | All |
| **Detection approach** | Pattern matching |
| **False positive rate** | Low |
| **Existing tools** | None dedicated |

**Source code patterns:**

```rust
// Anti-pattern: busy-wait without yield
while !flag.load(Ordering::Relaxed) {
    // BUG: spinning without yielding CPU
}
// Fix: add std::hint::spin_loop() or std::thread::yield_now()

// Anti-pattern: spinlock in userspace
loop {
    if lock.compare_exchange(false, true, ...).is_ok() {
        break;
    }
    // No backoff, no yield -- 100% CPU usage
}
```

**Detection algorithm:**
1. Find tight loops (`while`/`loop`) whose body is:
   - Only atomic loads/CAS operations
   - No `sleep`, `yield`, `spin_loop_hint`, `pause`, `sched_yield`
2. Flag as busy-wait without yield.

---

### 4.5 Livelock Detection

| Field | Value |
|-------|-------|
| **Bug class** | Threads making progress individually but collectively stuck |
| **CWE** | CWE-662 |
| **Languages** | All |
| **Detection approach** | Pattern matching (limited) |
| **False positive rate** | High |
| **Existing tools** | None for static detection |

**Description:** Unlike deadlock, threads in a livelock are not blocked --
they're actively executing but making no forward progress (e.g., two threads
repeatedly yielding to each other).

**Static detection is extremely limited.** The best we can do is flag
suspicious patterns:
1. Retry loops that back off identically (same backoff formula) in
   multiple threads accessing the same resource.
2. CAS retry loops where the "failure" path modifies the same shared state
   that caused the failure (feedback loop).

---

## 5. Tool Landscape Summary

### By Language

| Tool | Type | Languages | What it detects |
|------|------|-----------|-----------------|
| **ThreadSanitizer** | Dynamic | C/C++, Go, Rust (via LLVM) | Data races, deadlocks, thread leaks |
| **Helgrind** | Dynamic | C/C++ (via Valgrind) | Data races, lock order violations, misuse of POSIX APIs |
| **RacerD** | Static | Java | Data races (inconsistent sync) |
| **Infer/Pulse** | Static | Java, C, C++, ObjC | Resource leaks, null derefs, data races, starvation |
| **Lockbud** | Static | Rust | Double-lock, conflicting lock order, condvar deadlock |
| **Miri** | Dynamic (interpreter) | Rust | UB, data races, aliasing violations, memory leaks |
| **SpotBugs** | Static | Java | 400+ bug patterns including concurrency |
| **Clippy** | Static | Rust | mutex_atomic, misc lints; limited concurrency |
| **Go Race Detector** | Dynamic | Go | Data races |
| **Staticcheck** | Static | Go | General bugs; limited concurrency |
| **ESLint** | Static | JavaScript | require-atomic-updates; limited |
| **Peahen** | Static (research) | C/C++ | Deadlocks with context-sensitive gate-lock reduction |
| **DLOS** | Static (research) | C (kernel) | OS kernel deadlocks |
| **CDSChecker/GenMC** | Model checker | C/C++ | Memory model violations, weak memory bugs |
| **Relinche** | Model checker | C/C++ | Linearizability under weak memory |
| **NodeRacer** | Dynamic | Node.js | Event race conditions |
| **GCC -fanalyzer** | Static | C | Resource leaks (FD, FILE*), double-free |
| **PVS-Studio** | Static | C/C++, Java, C# | TOCTOU, concurrency issues, 900+ checks |
| **Coverity** | Static | C/C++, Java | TOCTOU, concurrency, resource leaks |
| **ConSynergy** | Hybrid (static+LLM) | C/C++ | Complex concurrency bugs via SMT solving |

### By Bug Class

| Bug Class | Static Detection Feasible? | Best Approach |
|-----------|---------------------------|---------------|
| ABA problem | Yes (pattern) | CAS-on-pointer without epoch guard |
| Memory ordering | Partial (pattern) | Flag Relaxed on publish patterns |
| Progress violations | Yes (call graph) | Blocking calls in lock-free code |
| Linearizability | No (need model checking) | Flag check-then-act patterns |
| Deadlock (lock order) | Yes (lock graph) | Cycle detection in acquisition graph |
| Double-lock | Yes (scope analysis) | Same lock acquired twice in scope/callgraph |
| Lock contention | Partial (heuristic) | I/O under lock scope |
| Missing lock guards | Yes (abstract interp) | Inconsistent sync on same field |
| Mutex poisoning | Yes (pattern) | `.lock().unwrap()` in Rust |
| RwLock starvation | Partial (heuristic) | Read:write ratio analysis |
| Missing timeouts | Yes (pattern) | Network calls without timeout param |
| Unbounded queues | Yes (pattern) | Channel/queue without capacity |
| Missing circuit breaker | Yes (pattern+loop) | Retry loop without limit/backoff |
| Resource leaks | Yes (data flow) | Open without close on all paths |
| Missing shutdown | Yes (pattern) | Server without signal handler |
| Panic-in-drop | Yes (pattern+callgraph) | Panicking calls in Drop impl |
| TOCTOU | Yes (pattern) | Check-then-use on same path |
| Priority inversion | Partial | Cross-priority mutex without inheritance |
| Signal safety | Yes (callgraph) | Non-safe calls reachable from handler |
| Starvation/busy-wait | Yes (pattern) | Tight loop without yield |
| Livelock | Barely | Symmetric retry patterns |

---

## 6. Implementation Priority Matrix

Ranked by: impact (severity x frequency) / implementation difficulty.

### Tier 1: High Impact, Low Difficulty (Pattern Matching)

These can be implemented as APEX detectors using regex + simple scope analysis,
matching the existing `SecurityPattern` / `MissingTimeoutDetector` architecture.

| # | Detector | Est. Lines | Languages |
|---|----------|-----------|-----------|
| 1 | **Missing Timeout (multi-lang)** | ~200 | Extend existing Python to Rust, Go, JS, Java, C |
| 2 | **Panic-in-Drop** | ~150 | Rust |
| 3 | **Mutex Poisoning Unwrap** | ~80 | Rust |
| 4 | **Double-Lock (intra-procedural)** | ~200 | Rust, Go, Python, Java, C |
| 5 | **TOCTOU Filesystem** | ~200 | All |
| 6 | **Busy-Wait Without Yield** | ~120 | Rust, C/C++, Go, Java |
| 7 | **Unbounded Queue/Channel** | ~150 | Rust, Go, Python, Java, JS |
| 8 | **Missing Graceful Shutdown** | ~180 | Rust, Go, Python, JS, Java |
| 9 | **Resource Leak (simple)** | ~250 | Python, Go, Java, JS |
| 10 | **Signal Safety Violations** | ~200 | C, Rust |

### Tier 2: High Impact, Medium Difficulty (Scope/Flow Analysis)

These need lock guard tracking, scope analysis, or simple data flow.

| # | Detector | Est. Lines | Languages |
|---|----------|-----------|-----------|
| 11 | **Deadlock (lock ordering)** | ~500 | Rust, Go, Java, C |
| 12 | **Lock Contention Hotspots** | ~300 | Rust, Go, Java |
| 13 | **Memory Ordering (Relaxed publish)** | ~300 | Rust, C/C++ |
| 14 | **ABA Problem** | ~350 | Rust, C/C++ |
| 15 | **Missing Retry Limit/Backoff** | ~250 | All |
| 16 | **Check-then-Act on Concurrent Collections** | ~200 | Java, Go, Python |

### Tier 3: Medium Impact, High Difficulty (Inter-procedural/Semantic)

These require call graph construction, abstract interpretation, or
cross-function analysis.

| # | Detector | Est. Lines | Languages |
|---|----------|-----------|-----------|
| 17 | **Progress Guarantee Violations** | ~600 | Rust, C/C++ |
| 18 | **Inconsistent Synchronization (RacerD-lite)** | ~800 | Java, Go |
| 19 | **Resource Leak (all paths)** | ~600 | All |
| 20 | **RwLock Starvation Analysis** | ~300 | Rust, Go, Java |
| 21 | **Priority Inversion** | ~400 | C/C++, Rust (embedded) |

### Not Recommended for Static Detection

| Bug Class | Reason | Alternative |
|-----------|--------|-------------|
| Linearizability | Requires state space exploration | Model checking (Line-Up, Relinche) |
| Livelock | No reliable static patterns | Dynamic testing / fuzzing |
| Full data race detection | Needs happens-before analysis | ThreadSanitizer, Miri |

---

## CWE Reference Table

| CWE ID | Name | Bug Classes |
|--------|------|-------------|
| CWE-362 | Race Condition | Data races, ABA, memory ordering |
| CWE-367 | TOCTOU | Filesystem/network check-then-use |
| CWE-400 | Uncontrolled Resource Consumption | Missing timeout, busy-wait, unbounded queue |
| CWE-404 | Improper Resource Shutdown | Resource leaks, missing shutdown |
| CWE-479 | Signal Handler Non-Reentrant Function | Signal safety violations |
| CWE-662 | Improper Synchronization | Lock contention, starvation, priority inversion |
| CWE-664 | Improper Control of Resource Lifetime | Resource leaks |
| CWE-667 | Improper Locking | Various lock bugs |
| CWE-764 | Multiple Locks of Critical Resource | Double-lock |
| CWE-770 | Resource Allocation Without Limits | Unbounded queues |
| CWE-772 | Missing Release of Resource | FD/socket/file leaks |
| CWE-775 | Missing Release of File Descriptor | FD leaks specifically |
| CWE-820 | Missing Synchronization | Missing lock guards |
| CWE-821 | Incorrect Synchronization | Wrong ordering, wrong lock |
| CWE-825 | Expired Pointer Dereference | ABA problem (use-after-free variant) |
| CWE-833 | Deadlock | Lock ordering, double-lock |
| CWE-835 | Infinite Loop | Retry without limit |
| CWE-755 | Improper Handling of Exceptional Conditions | Mutex poisoning, panic-in-drop |

---

## References

### Tools
- [Lockbud](https://github.com/BurtonQin/lockbud) -- Rust concurrency bug detector
- [Infer/RacerD](https://fbinfer.com/docs/checker-racerd/) -- Facebook's compositional race detector
- [SpotBugs](https://spotbugs.github.io/) -- Java static analysis (FindBugs successor)
- [Miri](https://github.com/rust-lang/miri) -- Rust interpreter for UB detection
- [ThreadSanitizer](https://clang.llvm.org/docs/ThreadSanitizer.html) -- LLVM dynamic race detector
- [Go Race Detector](https://go.dev/doc/articles/race_detector) -- Built-in Go race detection
- [Clippy](https://rust-lang.github.io/rust-clippy/master/index.html) -- Rust linter
- [Staticcheck](https://staticcheck.dev/) -- Go static analysis
- [NodeRacer](https://users-cs.au.dk/amoeller/papers/noderacer/paper.pdf) -- Node.js event race detection
- [asyncsafe](https://github.com/dwks/asyncsafe) -- Signal safety checker
- [tracing-mutex](https://docs.rs/tracing-mutex) -- Rust runtime deadlock detection
- [GCC -fanalyzer](https://gcc.gnu.org/onlinedocs/gcc-15.2.0/gcc/Static-Analyzer-Options.html) -- C resource leak detection

### Research Papers
- Peahen: Fast and Precise Static Deadlock Detection via Context Reduction (FSE 2022)
- DLOS: Effective Static Detection of Deadlocks in OS Kernels (ATC 2022)
- Static Deadlock Detection for Rust Programs (arXiv 2401.01114, 2024)
- Language-Agnostic Static Deadlock Detection for Futures (PPoPP 2024)
- Relinche: Automatically Checking Linearizability (POPL 2025)
- Miri: Practical Undefined Behavior Detection for Rust (POPL 2026)
- ConSynergy: Concurrency Bug Detection via Static Analysis and LLMs (2025)
- RacerD: Compositional Static Race Detection (OOPSLA 2018)
- Line-Up: A Complete and Automatic Linearizability Checker (Microsoft Research)
- Decoupling Lock-Free Data Structures from Memory Reclamation for Static Analysis (POPL 2019)

### Standards
- [CERT C CON09-C](https://wiki.sei.cmu.edu/confluence/display/c/CON09-C.+Avoid+the+ABA+problem+when+using+lock+free+algorithms) -- Avoid ABA problem
- [CERT C SIG30-C](https://wiki.sei.cmu.edu/confluence/display/c/SIG30-C.+Call+only+asynchronous-safe+functions+within+signal+handlers) -- Async-signal-safe functions only
- [CWE-362](https://cwe.mitre.org/data/definitions/362.html) -- Race Condition
- [CWE-833](https://cwe.mitre.org/data/definitions/833.html) -- Deadlock
- [CWE-367](https://cwe.mitre.org/data/definitions/367.html) -- TOCTOU
