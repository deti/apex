//! LLM-based triage for security findings (LLMxCPG, USENIX Security 2025).
//!
//! For each finding, extract a minimal code slice around the finding location,
//! then optionally use an LLM to assess exploitability. The actual LLM call
//! is opt-in (requires `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`). When no key
//! is configured, `triage_findings` returns `NeedsReview` for every finding.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::finding::Finding;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The LLM's verdict on a single finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriageVerdict {
    /// Exploitability classification.
    pub classification: TriageClass,
    /// Human-readable explanation produced by the LLM (or a stub message when
    /// no LLM is configured).
    pub reasoning: String,
    /// Confidence in the classification, 0.0–1.0.
    pub confidence: f64,
}

/// High-level exploitability classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageClass {
    /// Real vulnerability — the taint path is exercisable by an attacker.
    Confirmed,
    /// Not exploitable — sanitization, type constraints, or other mitigations
    /// break the taint chain in this slice.
    FalsePositive,
    /// Uncertain — the slice does not provide enough context for a definitive
    /// verdict.  Keep the finding as-is.
    NeedsReview,
}

// ---------------------------------------------------------------------------
// Slice extraction
// ---------------------------------------------------------------------------

/// Extract a minimal source slice around a finding for LLM triage.
///
/// Returns a `String` containing:
/// - Up to `context_lines` lines before the finding's line number.
/// - The finding line itself.
/// - Up to `context_lines` lines after the finding's line number.
///
/// When the source file is not present in `source_cache`, or the finding has
/// no line number, an empty string is returned.
///
/// The slice intentionally includes surrounding lines (function signature,
/// variable declarations, sanitizer calls) so the LLM can reason about the
/// full local context rather than just the isolated sink call.
pub fn extract_finding_slice(
    source_cache: &HashMap<PathBuf, String>,
    finding: &Finding,
    context_lines: usize,
) -> String {
    let line_1based = match finding.line {
        Some(l) => l as usize,
        None => return String::new(),
    };

    let source = match source_cache.get(&finding.file) {
        Some(s) => s,
        None => return String::new(),
    };

    let lines: Vec<&str> = source.lines().collect();
    let n = lines.len();

    if n == 0 || line_1based == 0 || line_1based > n {
        return String::new();
    }

    // Convert to 0-based index.
    let target = line_1based - 1;
    let start = target.saturating_sub(context_lines);
    let end = (target + context_lines + 1).min(n);

    let mut buf = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let abs_line = start + i + 1; // 1-based for display
        if abs_line == line_1based {
            buf.push_str(&format!(">>> {abs_line:4}: {line}\n"));
        } else {
            buf.push_str(&format!("    {abs_line:4}: {line}\n"));
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// Triage engine
// ---------------------------------------------------------------------------

/// Triage a batch of findings.
///
/// When `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` is set in the environment,
/// each finding is sent to the LLM with its code slice for exploitability
/// assessment. Without an API key the function falls back to a `NeedsReview`
/// stub so callers always receive a result vector of the same length as
/// `findings`.
///
/// The CPG is used to attach a taint-flow summary to the prompt when
/// available, giving the LLM additional signal beyond the raw source lines.
///
/// # Note on stub behaviour
///
/// The LLM call itself is intentionally stubbed here — the infrastructure
/// (slice extraction, prompt construction, result wiring) is the primary
/// deliverable. A production implementation would replace `stub_verdict` with
/// an async HTTP call to the chosen LLM provider.
pub async fn triage_findings(
    findings: &[Finding],
    source_cache: &HashMap<PathBuf, String>,
    cpg: Option<&apex_cpg::Cpg>,
) -> Vec<(Finding, TriageVerdict)> {
    let has_api_key =
        std::env::var("ANTHROPIC_API_KEY").is_ok() || std::env::var("OPENAI_API_KEY").is_ok();

    findings
        .iter()
        .map(|f| {
            let slice = extract_finding_slice(source_cache, f, 15);
            let taint_summary = build_taint_summary(cpg, f);
            let verdict = if has_api_key {
                // Stub: in a full implementation this would make an async
                // HTTP request to the LLM API with the prompt built from
                // `slice` and `taint_summary`.
                let _prompt = build_triage_prompt(f, &slice, &taint_summary);
                stub_verdict()
            } else {
                TriageVerdict {
                    classification: TriageClass::NeedsReview,
                    reasoning: "No LLM API key configured — skipping automated triage. \
                                Set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable."
                        .into(),
                    confidence: 0.0,
                }
            };
            (f.clone(), verdict)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a taint-flow summary string from the CPG for the given finding.
///
/// Returns an empty string when no CPG is available or no flows are found.
fn build_taint_summary(cpg: Option<&apex_cpg::Cpg>, finding: &Finding) -> String {
    let cpg = match cpg {
        Some(c) => c,
        None => return String::new(),
    };

    let sink_line = match finding.line {
        Some(l) => l,
        None => return String::new(),
    };

    // Count identifier / call nodes on the sink line that have reaching-def
    // edges from a Parameter — a simple proxy for "taint arrives here".
    use apex_cpg::{EdgeKind, NodeKind};

    let tainted_names: Vec<String> = cpg
        .nodes()
        .filter_map(|(id, kind)| {
            let line = match kind {
                NodeKind::Identifier { line, .. }
                | NodeKind::Call { line, .. }
                | NodeKind::Assignment { line, .. } => *line,
                _ => return None,
            };
            if line != sink_line {
                return None;
            }
            // Check for incoming ReachingDef from a Parameter.
            let has_taint = cpg.edges_to(id).iter().any(|(from, _, ek)| {
                matches!(ek, EdgeKind::ReachingDef { .. })
                    && matches!(cpg.node(*from), Some(NodeKind::Parameter { .. }))
            });
            if has_taint {
                let name = match kind {
                    NodeKind::Identifier { name, .. }
                    | NodeKind::Call { name, .. }
                    | NodeKind::Assignment { lhs: name, .. } => name.clone(),
                    _ => return None,
                };
                Some(name)
            } else {
                None
            }
        })
        .collect();

    if tainted_names.is_empty() {
        "CPG: no taint flow detected at sink line.".into()
    } else {
        format!(
            "CPG taint flow: parameter data reaches '{}' at line {}.",
            tainted_names.join(", "),
            sink_line
        )
    }
}

/// Construct the LLM prompt for a single finding.
///
/// The prompt follows the LLMxCPG methodology: provide the code slice,
/// the finding metadata, and any CPG taint-flow information, then ask the
/// model for an exploitability verdict.
fn build_triage_prompt(finding: &Finding, slice: &str, taint_summary: &str) -> String {
    let mut prompt = format!(
        "You are a security analyst reviewing a potential vulnerability.\n\n\
         Finding: {title}\n\
         Detector: {detector}\n\
         File: {file}\n\
         CWE: {cwes}\n\n",
        title = finding.title,
        detector = finding.detector,
        file = finding.file.display(),
        cwes = finding
            .cwe_ids
            .iter()
            .map(|c| format!("CWE-{c}"))
            .collect::<Vec<_>>()
            .join(", "),
    );

    if !taint_summary.is_empty() {
        prompt.push_str(&format!("Taint analysis: {taint_summary}\n\n"));
    }

    if !slice.is_empty() {
        prompt.push_str(&format!(
            "Code slice (>>> marks the flagged line):\n```\n{slice}```\n\n"
        ));
    }

    prompt.push_str(
        "Is this a real exploitable vulnerability?\n\
         Answer with one of: Confirmed, FalsePositive, NeedsReview.\n\
         Then explain your reasoning in 2-3 sentences.",
    );

    prompt
}

/// Stub verdict used when an LLM API key is present but the HTTP call is not
/// yet implemented. Returns `NeedsReview` with a note.
fn stub_verdict() -> TriageVerdict {
    TriageVerdict {
        classification: TriageClass::NeedsReview,
        reasoning: "LLM API call not yet implemented — infrastructure stub. \
                    Finding retained for human review."
            .into(),
        confidence: 0.5,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{FindingCategory, Severity};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_finding(file: &str, line: Option<u32>) -> Finding {
        Finding {
            id: Uuid::new_v4(),
            detector: "test-detector".into(),
            severity: Severity::High,
            category: FindingCategory::Injection,
            file: PathBuf::from(file),
            line,
            title: "Test injection finding".into(),
            description: "Test description".into(),
            evidence: vec![],
            covered: false,
            suggestion: "Fix it.".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![78],
            noisy: false,
            base_severity: None,
            coverage_confidence: None,
        }
    }

    // ---- extract_finding_slice ----

    #[test]
    fn slice_returns_correct_context_around_finding() {
        let source = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n";
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("src/app.js"), source.into());

        let finding = make_finding("src/app.js", Some(4));
        let slice = extract_finding_slice(&cache, &finding, 2);

        // Should contain lines 2-6 (4 ± 2).
        assert!(slice.contains("line 2"), "should include 2 lines before");
        assert!(slice.contains("line 6"), "should include 2 lines after");
        // Finding line should be marked with >>>.
        assert!(
            slice.contains(">>>"),
            "finding line should be marked with >>>"
        );
        // Lines around it should not have >>>.
        let lines: Vec<&str> = slice.lines().collect();
        let marked: Vec<_> = lines.iter().filter(|l| l.contains(">>>")).collect();
        assert_eq!(marked.len(), 1, "exactly one line should be marked");
    }

    #[test]
    fn slice_clamps_to_file_boundaries() {
        let source = "only line\n";
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("src/tiny.js"), source.into());

        let finding = make_finding("src/tiny.js", Some(1));
        let slice = extract_finding_slice(&cache, &finding, 10);

        // Should still work without panic, returning just the one line.
        assert!(slice.contains("only line"));
        assert!(slice.contains(">>>"));
    }

    #[test]
    fn slice_returns_empty_when_no_line_number() {
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("src/app.js"), "some code\n".into());

        let finding = make_finding("src/app.js", None);
        let slice = extract_finding_slice(&cache, &finding, 5);
        assert!(
            slice.is_empty(),
            "no line number should return empty string"
        );
    }

    #[test]
    fn slice_returns_empty_when_file_not_in_cache() {
        let cache = HashMap::new();
        let finding = make_finding("src/missing.js", Some(1));
        let slice = extract_finding_slice(&cache, &finding, 5);
        assert!(slice.is_empty(), "missing file should return empty string");
    }

    #[test]
    fn slice_returns_empty_for_zero_line() {
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("src/app.js"), "code\n".into());
        let finding = make_finding("src/app.js", Some(0));
        let slice = extract_finding_slice(&cache, &finding, 5);
        assert!(slice.is_empty(), "line 0 is invalid, should return empty");
    }

    // ---- TriageVerdict serialization ----

    #[test]
    fn triage_verdict_serializes_correctly() {
        let verdict = TriageVerdict {
            classification: TriageClass::Confirmed,
            reasoning: "Taint reaches exec().".into(),
            confidence: 0.9,
        };
        let json = serde_json::to_string(&verdict).unwrap();
        assert!(
            json.contains("\"confirmed\""),
            "classification should serialize as snake_case"
        );
        assert!(json.contains("0.9"), "confidence should be in JSON");
    }

    #[test]
    fn triage_verdict_deserializes_correctly() {
        let json =
            r#"{"classification":"false_positive","reasoning":"No user input.","confidence":0.8}"#;
        let verdict: TriageVerdict = serde_json::from_str(json).unwrap();
        assert_eq!(verdict.classification, TriageClass::FalsePositive);
        assert!((verdict.confidence - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn triage_class_needs_review_roundtrip() {
        let v = TriageClass::NeedsReview;
        let json = serde_json::to_string(&v).unwrap();
        let back: TriageClass = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }

    // ---- triage_findings (no API key path) ----

    #[tokio::test]
    async fn triage_findings_without_api_key_returns_needs_review() {
        // Ensure no API keys in test environment.
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");

        let findings = vec![make_finding("src/app.js", Some(1))];
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("src/app.js"), "exec(userInput)\n".into());

        let results = triage_findings(&findings, &cache, None).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.classification, TriageClass::NeedsReview);
        assert!(
            results[0].1.reasoning.contains("No LLM API key"),
            "reasoning should mention missing API key"
        );
        assert_eq!(results[0].1.confidence, 0.0);
    }

    #[tokio::test]
    async fn triage_findings_returns_one_result_per_finding() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");

        let findings = vec![
            make_finding("src/a.js", Some(1)),
            make_finding("src/b.js", Some(2)),
            make_finding("src/c.js", Some(3)),
        ];
        let cache = HashMap::new();
        let results = triage_findings(&findings, &cache, None).await;
        assert_eq!(results.len(), 3, "one result per finding");
    }

    #[tokio::test]
    async fn triage_findings_empty_input_returns_empty() {
        let results = triage_findings(&[], &HashMap::new(), None).await;
        assert!(results.is_empty());
    }

    // ---- build_taint_summary ----

    #[test]
    fn taint_summary_empty_when_no_cpg() {
        let finding = make_finding("src/app.py", Some(1));
        let summary = build_taint_summary(None, &finding);
        assert!(summary.is_empty());
    }

    #[test]
    fn taint_summary_detects_parameter_reaching_sink() {
        use apex_cpg::{EdgeKind, NodeKind};

        let mut cpg = apex_cpg::Cpg::new();
        let param = cpg.add_node(NodeKind::Parameter {
            name: "user_input".into(),
            index: 0,
        });
        let sink = cpg.add_node(NodeKind::Identifier {
            name: "cmd".into(),
            line: 5,
        });
        cpg.add_edge(
            param,
            sink,
            EdgeKind::ReachingDef {
                variable: "cmd".into(),
            },
        );

        let finding = make_finding("src/app.py", Some(5));
        let summary = build_taint_summary(Some(&cpg), &finding);
        assert!(
            summary.contains("CPG taint flow"),
            "should report taint flow: {summary}"
        );
        assert!(
            summary.contains("cmd"),
            "should name the tainted identifier"
        );
    }

    #[test]
    fn taint_summary_reports_no_flow_when_no_edges() {
        use apex_cpg::NodeKind;

        let mut cpg = apex_cpg::Cpg::new();
        cpg.add_node(NodeKind::Identifier {
            name: "cmd".into(),
            line: 3,
        });

        let finding = make_finding("src/app.py", Some(3));
        let summary = build_taint_summary(Some(&cpg), &finding);
        assert!(
            summary.contains("no taint flow"),
            "should report no taint flow: {summary}"
        );
    }

    // ---- build_triage_prompt ----

    #[test]
    fn prompt_contains_finding_metadata() {
        let finding = make_finding("src/run.js", Some(10));
        let slice = "    10: exec(cmd)\n";
        let prompt = build_triage_prompt(&finding, slice, "");
        assert!(
            prompt.contains("test-detector"),
            "should include detector name"
        );
        assert!(prompt.contains("CWE-78"), "should include CWE");
        assert!(prompt.contains("run.js"), "should include file name");
    }

    #[test]
    fn prompt_includes_taint_summary_when_present() {
        let finding = make_finding("src/run.js", Some(10));
        let summary = "CPG taint flow: parameter data reaches 'cmd' at line 10.";
        let prompt = build_triage_prompt(&finding, "", summary);
        assert!(
            prompt.contains("Taint analysis"),
            "should include taint section"
        );
        assert!(
            prompt.contains("CPG taint flow"),
            "should include the summary"
        );
    }

    #[test]
    fn prompt_omits_taint_section_when_empty() {
        let finding = make_finding("src/run.js", Some(10));
        let prompt = build_triage_prompt(&finding, "", "");
        assert!(
            !prompt.contains("Taint analysis"),
            "should not include empty taint section"
        );
    }
}
