# APEX Generate Tests

Generate tests for a specific crate or file, targeting uncovered branches.

## Usage
```
/apex-generate <crate-or-file>
```
Examples:
- `/apex-generate apex-coverage` — generate tests for the oracle
- `/apex-generate apex-fuzz/src/mutators.rs` — generate tests for mutators
- `/apex-generate apex-agent` — generate tests for the agent orchestrator

## Instructions

1. Identify the target from `$ARGUMENTS` (required — ask if missing).

2. Read the source file completely to understand:
   - All public functions
   - Error branches and edge cases
   - Existing `#[cfg(test)]` blocks

3. Run `cargo llvm-cov --json` to find uncovered regions in the target file.

4. Write tests that cover the uncovered regions. Place them in:
   - `#[cfg(test)] mod tests { ... }` inside the source file, OR
   - `crates/<name>/tests/<module>_tests.rs` for integration tests

5. Run `cargo test -p <crate>` to verify they pass.

6. Report: which regions are now covered, new coverage %, and any remaining gaps.

## Test Writing Standards

- Use `#[tokio::test]` for async functions
- Mock external I/O (HTTP, filesystem) with temp dirs and fake data
- For oracle tests: construct `BranchId` values directly
- Keep tests fast — no real network calls, no sleeping
