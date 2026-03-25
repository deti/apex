---
date: 2026-03-26
crew: runtime
at_commit: 73a9c9f
affected_partners: [foundation,exploration,intelligence]
severity: minor
acknowledged_by: []
---

ExecutionResult gained a new resource_metrics: Option<ResourceMetrics> field (serde default = None)

Added ResourceMetrics { wall_time_ms, cpu_time_ms, peak_memory_bytes } to apex_core::types.
Added resource_metrics: Option<ResourceMetrics> to ExecutionResult with #[serde(default)].
ProcessSandbox::run() now populates this field on every execution using getrusage(RUSAGE_CHILDREN).
All other ExecutionResult constructors in apex-sandbox set resource_metrics: None.
Serialized JSON from older versions will deserialize fine (field defaults to None).
Partners that pattern-match on ExecutionResult struct literal construction will need to add the field.
