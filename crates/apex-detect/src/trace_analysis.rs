//! Cross-Service Trace Analysis — analyzes distributed trace data.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub service: String,
    pub operation: String,
    pub duration_ms: f64,
    pub status: SpanStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SpanStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceIssue {
    pub trace_id: String,
    pub issue_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceReport {
    pub total_traces: usize,
    pub total_spans: usize,
    pub slow_spans: Vec<Span>,
    pub broken_traces: Vec<TraceIssue>,
    pub error_chains: Vec<Vec<Span>>,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

pub fn analyze_traces(spans: &[Span], slow_threshold_ms: f64) -> TraceReport {
    let mut traces: HashMap<&str, Vec<&Span>> = HashMap::new();
    for span in spans {
        traces
            .entry(span.trace_id.as_str())
            .or_default()
            .push(span);
    }

    let mut slow_spans: Vec<Span> = spans
        .iter()
        .filter(|s| s.duration_ms > slow_threshold_ms)
        .cloned()
        .collect();
    slow_spans.sort_by(|a, b| {
        b.duration_ms
            .partial_cmp(&a.duration_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Find broken traces (spans with parent_span_id not found in same trace)
    let mut broken_traces = Vec::new();
    for (trace_id, trace_spans) in &traces {
        let span_ids: std::collections::HashSet<&str> =
            trace_spans.iter().map(|s| s.span_id.as_str()).collect();
        for span in trace_spans {
            if let Some(parent) = &span.parent_span_id {
                if !span_ids.contains(parent.as_str()) {
                    broken_traces.push(TraceIssue {
                        trace_id: trace_id.to_string(),
                        issue_type: "orphan-span".into(),
                        description: format!(
                            "Span {} references missing parent {}",
                            span.span_id, parent
                        ),
                    });
                }
            }
        }
    }

    // Find error chains
    let mut error_chains = Vec::new();
    for trace_spans in traces.values() {
        let errors: Vec<Span> = trace_spans
            .iter()
            .filter(|s| s.status == SpanStatus::Error)
            .map(|s| (*s).clone())
            .collect();
        if errors.len() > 1 {
            error_chains.push(errors);
        }
    }

    // Percentiles
    let mut durations: Vec<f64> = spans.iter().map(|s| s.duration_ms).collect();
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p50 = percentile(&durations, 50.0);
    let p95 = percentile(&durations, 95.0);
    let p99 = percentile(&durations, 99.0);

    TraceReport {
        total_traces: traces.len(),
        total_spans: spans.len(),
        slow_spans,
        broken_traces,
        error_chains,
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
    }
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_span(
        trace: &str,
        span: &str,
        parent: Option<&str>,
        ms: f64,
        status: SpanStatus,
    ) -> Span {
        Span {
            trace_id: trace.into(),
            span_id: span.into(),
            parent_span_id: parent.map(String::from),
            service: "svc".into(),
            operation: "op".into(),
            duration_ms: ms,
            status,
        }
    }

    #[test]
    fn detects_slow_spans() {
        let spans = vec![
            make_span("t1", "s1", None, 500.0, SpanStatus::Ok),
            make_span("t1", "s2", Some("s1"), 50.0, SpanStatus::Ok),
        ];
        let r = analyze_traces(&spans, 100.0);
        assert_eq!(r.slow_spans.len(), 1);
    }

    #[test]
    fn detects_broken_trace() {
        let spans = vec![make_span("t1", "s1", Some("missing"), 10.0, SpanStatus::Ok)];
        let r = analyze_traces(&spans, 1000.0);
        assert_eq!(r.broken_traces.len(), 1);
    }

    #[test]
    fn calculates_percentiles() {
        let spans: Vec<Span> = (1..=100)
            .map(|i| make_span("t1", &format!("s{i}"), None, i as f64, SpanStatus::Ok))
            .collect();
        let r = analyze_traces(&spans, 1000.0);
        assert!(r.p50_ms >= 49.0 && r.p50_ms <= 51.0);
        assert!(r.p95_ms >= 94.0 && r.p95_ms <= 96.0);
    }

    #[test]
    fn empty_spans() {
        let r = analyze_traces(&[], 100.0);
        assert_eq!(r.total_traces, 0);
    }
}
