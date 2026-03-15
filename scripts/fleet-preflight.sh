#!/usr/bin/env bash
set -euo pipefail

# Fleet Preflight — Runtime environment probe for adaptive parallelism.
#
# Usage: ./scripts/fleet-preflight.sh [task_count]
#   task_count  Number of tasks to dispatch (default: 6)
#
# Output: JSON with recommended dispatch parameters.
# Called by fleet dispatch before launching parallel agents.

TASK_COUNT="${1:-6}"

# ── Platform detection ────────────────────────────────────────────
OS=$(uname -s)

# ── CPU cores ─────────────────────────────────────────────────────
if [ "$OS" = "Darwin" ]; then
  CPU_CORES=$(sysctl -n hw.ncpu 2>/dev/null || echo 4)
else
  CPU_CORES=$(nproc 2>/dev/null || echo 4)
fi

# ── Current load (1-min avg) ─────────────────────────────────────
LOAD_AVG=$(uptime | awk '{print $(NF-2)}' | tr -d ',')
LOAD_RATIO=$(python3 -c "print(round(float('$LOAD_AVG') / $CPU_CORES, 2))")

# ── Battery state ────────────────────────────────────────────────
POWER_SOURCE="ac"
BATTERY_PCT=100

if [ "$OS" = "Darwin" ]; then
  BATT_INFO=$(pmset -g batt 2>/dev/null || echo "")
  if echo "$BATT_INFO" | grep -q "Battery Power"; then
    POWER_SOURCE="battery"
  fi
  BATTERY_PCT=$(echo "$BATT_INFO" | grep -oE '[0-9]+%' | head -1 | tr -d '%' || echo 100)
  [ -z "$BATTERY_PCT" ] && BATTERY_PCT=100
elif [ -f /sys/class/power_supply/BAT0/capacity ]; then
  BATTERY_PCT=$(cat /sys/class/power_supply/BAT0/capacity)
  if [ -f /sys/class/power_supply/BAT0/status ]; then
    STATUS=$(cat /sys/class/power_supply/BAT0/status)
    [ "$STATUS" = "Discharging" ] && POWER_SOURCE="battery"
  fi
fi

# ── Thermal pressure (macOS) ─────────────────────────────────────
THERMAL="nominal"
if [ "$OS" = "Darwin" ]; then
  THERM_INFO=$(pmset -g therm 2>/dev/null || echo "")
  if echo "$THERM_INFO" | grep -qi "speed_limit.*[0-9]"; then
    SPEED_LIMIT=$(echo "$THERM_INFO" | grep -oE 'CPU_Speed_Limit\s*=\s*[0-9]+' | grep -oE '[0-9]+' || echo 100)
    [ -n "$SPEED_LIMIT" ] && [ "$SPEED_LIMIT" -lt 80 ] && THERMAL="throttled"
    [ -n "$SPEED_LIMIT" ] && [ "$SPEED_LIMIT" -lt 50 ] && THERMAL="critical"
  fi
fi

# ── Existing heavy processes ─────────────────────────────────────
RUSTC_PROCS=$(pgrep -c rustc 2>/dev/null || echo 0)
CARGO_PROCS=$(pgrep -c cargo 2>/dev/null || echo 0)
HEAVY_PROCS=$((RUSTC_PROCS + CARGO_PROCS))

# ── Workspace size estimate (affects compile time) ────────────────
CRATE_COUNT=0
if [ -d "crates" ]; then
  CRATE_COUNT=$(find crates -maxdepth 1 -mindepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
elif [ -f "Cargo.toml" ]; then
  CRATE_COUNT=1
fi

# ── Compute recommended parallelism ──────────────────────────────
#
# Formula:
#   base = cpu_cores / 4  (each cargo test grabs ~4 cores via rayon/rustc)
#   clamp to [1, task_count]
#   penalties: battery mode (-50%), low battery (-75%), thermal (-50%), high load (-25%)
#   existing rustc procs reduce available slots

RECOMMENDED=$(python3 -c "
import math

cpu = $CPU_CORES
tasks = $TASK_COUNT
power = '$POWER_SOURCE'
batt = $BATTERY_PCT
thermal = '$THERMAL'
load_ratio = $LOAD_RATIO
heavy = $HEAVY_PROCS

# Base: each Rust compilation grabs ~4 cores
base = max(1, cpu // 4)

# Penalties
if power == 'battery':
    base = max(1, base // 2)
if batt < 30:
    base = max(1, base // 2)
if thermal == 'throttled':
    base = max(1, math.ceil(base * 0.6))
elif thermal == 'critical':
    base = 1
if load_ratio > 0.7:
    base = max(1, base - heavy)

# Clamp to task count
result = min(base, tasks)
print(result)
")

# ── Wave strategy ────────────────────────────────────────────────
if [ "$TASK_COUNT" -le "$RECOMMENDED" ]; then
  WAVES=1
  WAVE_SIZES="[$TASK_COUNT]"
else
  WAVES=$(python3 -c "import math; print(math.ceil($TASK_COUNT / $RECOMMENDED))")
  WAVE_SIZES=$(python3 -c "
tasks = $TASK_COUNT
per_wave = $RECOMMENDED
waves = []
remaining = tasks
while remaining > 0:
    w = min(per_wave, remaining)
    waves.append(w)
    remaining -= w
print(waves)
")
fi

# ── Shared target dir recommendation ─────────────────────────────
# If multiple agents compile the same workspace, sharing target dir
# saves disk and partial rebuild time — but causes lock contention
# if two agents compile simultaneously. Pre-warming avoids this.
USE_SHARED_TARGET="false"
PRE_WARM="false"
if [ "$RECOMMENDED" -gt 1 ]; then
  USE_SHARED_TARGET="true"
  PRE_WARM="true"
fi

# ── Output ────────────────────────────────────────────────────────
cat <<EOF
{
  "environment": {
    "os": "$OS",
    "cpu_cores": $CPU_CORES,
    "load_avg": $LOAD_AVG,
    "load_ratio": $LOAD_RATIO,
    "power_source": "$POWER_SOURCE",
    "battery_pct": $BATTERY_PCT,
    "thermal": "$THERMAL",
    "existing_rustc_procs": $RUSTC_PROCS,
    "existing_cargo_procs": $CARGO_PROCS,
    "workspace_crates": $CRATE_COUNT
  },
  "dispatch": {
    "max_parallel": $RECOMMENDED,
    "total_tasks": $TASK_COUNT,
    "waves": $WAVES,
    "wave_sizes": $WAVE_SIZES,
    "shared_target_dir": $USE_SHARED_TARGET,
    "pre_warm_build": $PRE_WARM
  },
  "agent_rules": {
    "batch_fixes_before_testing": true,
    "max_cargo_test_invocations": 3,
    "max_cargo_clippy_invocations": 1
  }
}
EOF
