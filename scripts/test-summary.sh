#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/test-summary.sh [cargo test args...]
# Example: ./scripts/test-summary.sh -p apex-lang -- cpp
#
# Friendly wrapper around `cargo test` inspired by cargo-nextest and gotestsum.
# Hides empty test binaries, aggregates results, color-codes the summary.

export PATH="$HOME/.cargo/bin:$PATH"

# ── Colors (disabled if not a terminal) ─────────────────────────────
if [ -t 1 ]; then
  GREEN='\033[32m' RED='\033[31m' YELLOW='\033[33m'
  DIM='\033[90m' BOLD='\033[1m' RESET='\033[0m'
else
  GREEN='' RED='' YELLOW='' DIM='' BOLD='' RESET=''
fi

# ── Parse args to show what we're testing ───────────────────────────
CRATE="" FILTER="" PREV="" SEEN_SEP=false
for arg in "$@"; do
  if $SEEN_SEP; then
    [ -z "$FILTER" ] && FILTER="$arg"
  elif [ "$arg" = "--" ]; then
    SEEN_SEP=true
  elif [ "$PREV" = "-p" ]; then
    CRATE="$arg"
  elif [ "$arg" = "--workspace" ]; then
    CRATE="workspace"
  fi
  PREV="$arg"
done

LABEL="${CRATE:-workspace}"
[ -n "$FILTER" ] && LABEL="$LABEL (filter: $FILTER)"

# ── Run ─────────────────────────────────────────────────────────────
printf "\n  ${BOLD}Running:${RESET} cargo test"
[ -n "$CRATE" ] && [ "$CRATE" != "workspace" ] && printf " -p %s" "$CRATE"
[ "$CRATE" = "workspace" ] && printf " --workspace"
[ -n "$FILTER" ] && printf " -- %s" "$FILTER"
printf "\n\n"

OUTPUT=$(cargo test "$@" 2>&1) || true
EXIT_CODE=${PIPESTATUS[0]:-$?}

# ── Parse results ───────────────────────────────────────────────────
# Skip binaries with zero tests (no matches, doc-tests with nothing)
RESULTS=$(echo "$OUTPUT" | grep "^test result:" \
  | grep -v "0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out" || true)

# Aggregate counts
TOTAL_PASSED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="passed;") s+=$(i-1)} END{print s+0}')
TOTAL_FAILED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="failed;") s+=$(i-1)} END{print s+0}')
TOTAL_IGNORED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="ignored;") s+=$(i-1)} END{print s+0}')
TOTAL_FILTERED=$(echo "$RESULTS" | awk '{for(i=1;i<=NF;i++) if($i=="filtered") s+=$(i-1)} END{print s+0}')

# ── Handle edge cases ──────────────────────────────────────────────
if [ -z "$RESULTS" ] || { [ "$TOTAL_PASSED" -eq 0 ] && [ "$TOTAL_FAILED" -eq 0 ]; }; then
  if [ -n "$FILTER" ]; then
    printf "  ${YELLOW}0 tests matched filter \"%s\"${RESET}\n\n" "$FILTER"
  else
    printf "  ${DIM}No tests found.${RESET}\n\n"
  fi
  exit 0
fi

# ── Show failure details first (most important info) ────────────────
if [ "$TOTAL_FAILED" -gt 0 ]; then
  FAILURES=$(echo "$OUTPUT" | awk '/^failures:$/,/^test result:/' | grep '^ ' || true)
  if [ -n "$FAILURES" ]; then
    printf "  ${RED}${BOLD}Failures:${RESET}\n"
    echo "$FAILURES" | while read -r line; do
      printf "    ${RED}FAIL${RESET} %s\n" "$line"
    done
    printf "\n"
  fi
fi

# ── Summary (nextest-style) ─────────────────────────────────────────
# Format: PASS 14 passed, 2 skipped in 0.01s
#    or:  FAIL 12 passed, 2 failed in 0.03s

# Build parts list
PARTS=""
[ "$TOTAL_PASSED" -gt 0 ] && PARTS="${TOTAL_PASSED} passed"
if [ "$TOTAL_FAILED" -gt 0 ]; then
  [ -n "$PARTS" ] && PARTS="$PARTS, "
  PARTS="${PARTS}${TOTAL_FAILED} failed"
fi
if [ "$TOTAL_IGNORED" -gt 0 ]; then
  [ -n "$PARTS" ] && PARTS="$PARTS, "
  PARTS="${PARTS}${TOTAL_IGNORED} skipped"
fi

# Extract timing — sum all binary timings
TOTAL_TIME=$(echo "$RESULTS" | sed 's/.*finished in //' | awk '{s+=$1} END{printf "%.2fs", s}')

if [ "$TOTAL_FAILED" -gt 0 ]; then
  printf "  ${RED}${BOLD}FAIL${RESET}  %s in %s\n" "$PARTS" "$TOTAL_TIME"
else
  printf "  ${GREEN}${BOLD}PASS${RESET}  %s in %s\n" "$PARTS" "$TOTAL_TIME"
fi

printf "\n"
exit "$EXIT_CODE"
