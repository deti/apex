---
date: 2026-03-14
crew: foundation
affected_partners: [runtime, exploration]
severity: major
acknowledged_by: []
---

## CoverageOracle::merge_bitmap uses non-deterministic DashMap iteration order

`merge_bitmap()` at `crates/apex-coverage/src/oracle.rs:126-139` collects DashMap keys into a Vec to map bitmap indices to BranchIds. DashMap iteration order is not stable and can change between calls. Any partner crate that feeds AFL++ or other fuzzer bitmaps through `merge_bitmap()` will get non-deterministic coverage tracking.

**Fix:** Maintain a separate `Vec<BranchId>` or `IndexMap` that preserves insertion order, or require callers to pass an explicit index-to-branch mapping.

### Additional findings from foundation review

| Severity | File:Line | Issue |
|----------|-----------|-------|
| Medium | `oracle.rs:34,39` | Mutex `.unwrap()` can panic on poison |
| Low | `semantic.rs:25` | Regex compiled per call (should be LazyLock) |
| Low | `mutation.rs:108` | Dead `test_command` field never read |
| Low | `semantic.rs:13` | `stack_depth_max` always zero (unimplemented) |
| Info | `types.rs:45-59` | BranchId missing Ord/PartialOrd derivation |
