---
id: 01KNZ5F8TK2AR78RGESX54MZKQ
title: "Goose — Rust/Tokio load framework (Locust-inspired, type-safe)"
type: literature
tags: [tool, goose, load-testing, rust, tokio, locust-inspired, closed-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.907379+00:00
modified: 2026-04-11T20:59:25.907381+00:00
---

Source: https://github.com/tag1consulting/goose — Goose README and https://book.goose.rs/ — The Goose Book, fetched 2026-04-12.

Goose is a load testing framework written in Rust by Jeremy Andrews (Tag1 Consulting), first released in 2020. Explicit "Locust in Rust" — the conceptual model of User classes and tasks is borrowed directly, re-cast as Rust structs and async functions on top of Tokio. Apache-2.0.

## Scripting model

Tests are ordinary Rust binaries that `use goose::prelude::*;`:

```rust
use goose::prelude::*;

async fn loadtest_index(user: &mut GooseUser) -> TransactionResult {
    let _goose = user.get("/").await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), GooseError> {
    GooseAttack::initialize()?
        .register_scenario(
            scenario!("WebsiteUser")
                .register_transaction(transaction!(loadtest_index))
                .set_wait_time(Duration::from_secs(1), Duration::from_secs(5))?
        )
        .execute()
        .await?;
    Ok(())
}
```

Scenarios are ordered collections of Transactions (Goose's term for Locust's `@task`). Transactions are async functions receiving `&mut GooseUser`, which provides an HTTP client wrapping `reqwest` and per-user state.

## Concurrency

Goose schedules "Goose Users" as async Tokio tasks. A single OS thread hosts thousands of async users. This is the same architectural bet as Locust + gevent, but with Rust's zero-cost async and no GIL. Practical per-core throughput is 5–10x Locust's FastHttpUser.

## Distributed mode

Goose has a manager/worker model — `--manager` and `--worker` flags — that coordinates multiple Goose processes. Unlike Locust's ZeroMQ, Goose uses TCP with a custom protocol. The docs note that distributed mode is less mature than the single-process mode.

## Metrics and output

Goose collects per-transaction counts, response times, and error rates. Output options: terminal summary, CSV, JSON, HTML report, and direct Graphite/StatsD push. Percentiles are computed via `Float` based on retained samples — adequate for most uses but not HDR-histogram precision.

## Goose Eggs

`goose-eggs` is a helper crate with common patterns: checking response bodies for expected content, Drupal-specific helpers (Goose's original motivating use case was Drupal load testing at Tag1).

## Strengths

- Rust-native: compile-time checked tests, no runtime errors in typos, fast.
- Tokio async scales well on small infrastructure.
- Locust-style ergonomics without Locust's Python overhead.
- Real scenarios with multi-step flows and session state.

## Failure modes

- **Closed-loop model** — Goose's "users" are persistent, iterating transactions. No native arrival-rate knob.
- **Smaller community** than every other tool in this list. Few StackOverflow answers.
- **Rust compile time** — the edit/run loop is slower than k6/Locust; test iteration is painful.
- **Distributed mode is second-class** — manager/worker works but the docs are explicit that single-process is preferred.
- **No YAML / no scripting-friendly mode** — you must write Rust and `cargo build` per change.

## When Goose is the right answer

- You are already a Rust shop and want type-safe tests.
- You need very high per-machine throughput on minimal infrastructure.
- Your target is a Rust service and you want the load generator and the target to share client crates.

## Relevance to APEX G-46

Goose is the direct Rust-ecosystem competitor in G-46's landscape, and therefore interesting as a comparison to APEX (which is also Rust-native). Integration opportunity: APEX could emit Goose transaction functions from discovered worst-case HTTP inputs, giving Rust users a native reproduction harness.