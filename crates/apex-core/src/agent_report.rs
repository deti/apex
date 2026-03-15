use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::types::BranchId;

// ─── Report structs ─────────────────────────────────────────────────────────

/// Rich gap report designed for external agent consumption (Claude Code).
/// Produced by `--strategy agent --output-format json`.
///
/// **Note:** "gaps" are *uncovered branches* in the source code — not bugs or
/// security findings.  The `difficulty` field estimates how hard it would be to
/// write a test that exercises each branch (easy / medium / hard / blocked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGapReport {
    pub summary: GapSummary,
    pub gaps: Vec<GapEntry>,
    pub blocked: Vec<BlockedEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_summary: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compound_analysis: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapSummary {
    pub total_branches: usize,
    pub covered_branches: usize,
    pub coverage_pct: f64,
    pub files_total: usize,
    pub files_fully_covered: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapEntry {
    pub file: PathBuf,
    pub function: Option<String>,
    pub branch_line: u32,
    pub branch_condition: Option<String>,
    pub source_context: Vec<String>,
    pub uncovered_branches: usize,
    pub coverage_pct: f64,
    /// Fraction of file's branches that are uncovered. Higher = more gain from testing this file.
    pub bang_for_buck: f64,
    pub difficulty: GapDifficulty,
    pub difficulty_reason: String,
    pub suggested_approach: String,
    pub closest_existing_test: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedEntry {
    pub file: PathBuf,
    pub uncovered_branches: usize,
    pub reason: String,
}

/// How hard it is to write a test covering this uncovered branch.
///
/// - **Easy**: pure function, simple inputs, no external deps — unit test suffices.
/// - **Medium**: needs mocking, config setup, or filesystem access.
/// - **Hard**: async I/O, network, FFI, or process spawning — integration test needed.
/// - **Blocked**: cannot be tested without an external service or harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GapDifficulty {
    Easy,
    Medium,
    Hard,
    Blocked,
}

// ─── Difficulty classifier ──────────────────────────────────────────────────

/// Heuristic difficulty classification based on source context keywords.
pub fn classify_difficulty(source_lines: &[String]) -> (GapDifficulty, String) {
    let joined = source_lines.join(" ").to_lowercase();

    let hard_patterns: &[(&str, &str)] = &[
        ("unsafe", "unsafe code block"),
        ("extern", "FFI / extern function"),
        (".await", "async operation — needs runtime + possibly mock"),
        ("tokio::spawn", "async task spawning"),
        ("command::new", "process spawning"),
        ("std::process", "process spawning"),
        ("tcplistener", "network listener"),
        ("tcpstream", "network connection"),
        ("udpsocket", "network socket"),
        ("hyper::", "HTTP framework"),
        ("reqwest::", "HTTP client"),
        ("tonic::", "gRPC framework"),
        ("grpc", "gRPC integration"),
    ];

    for (pattern, reason) in hard_patterns {
        if joined.contains(pattern) {
            return (
                GapDifficulty::Hard,
                format!("needs external deps: {reason}"),
            );
        }
    }

    let medium_patterns: &[(&str, &str)] = &[
        ("match ", "match expression — needs test per arm"),
        ("unwrap_or", "error fallback path"),
        ("map_err", "error mapping"),
        ("ok_or", "option-to-result conversion"),
        ("config.", "configuration-dependent branch"),
        ("env::", "environment-dependent branch"),
        ("std::fs::", "filesystem operation"),
        ("file::open", "file I/O"),
    ];

    for (pattern, reason) in medium_patterns {
        if joined.contains(pattern) {
            return (GapDifficulty::Medium, format!("needs setup: {reason}"));
        }
    }

    (GapDifficulty::Easy, "simple conditional".to_string())
}

// ─── Bang-for-buck scorer ───────────────────────────────────────────────────

/// Fraction of file's branches that are uncovered. Higher = more improvement potential.
pub fn compute_bang_for_buck(uncovered_in_file: usize, total_in_file: usize) -> f64 {
    if total_in_file == 0 {
        return 0.0;
    }
    (uncovered_in_file as f64 / total_in_file as f64).min(1.0)
}

/// Generate a one-line test approach suggestion based on difficulty and source.
pub fn suggest_approach(difficulty: GapDifficulty, source_lines: &[String]) -> String {
    let joined = source_lines.join(" ").to_lowercase();

    match difficulty {
        GapDifficulty::Easy => {
            "Write a unit test with direct function call and assertion".to_string()
        }
        GapDifficulty::Medium => {
            if joined.contains("match ") {
                "Test each match arm with controlled input variants".to_string()
            } else if joined.contains("err") {
                "Test error path by providing invalid input".to_string()
            } else if joined.contains("config") || joined.contains("env") {
                "Test with different configuration/environment values".to_string()
            } else {
                "Write test with appropriate setup and mocks".to_string()
            }
        }
        GapDifficulty::Hard => {
            if joined.contains("unsafe") || joined.contains("extern") {
                "Use --strategy fuzz for binary-level exploration".to_string()
            } else if joined.contains(".await") || joined.contains("grpc") || joined.contains("tcp")
            {
                "Use --strategy driller or mock the async boundary".to_string()
            } else {
                "Consider --strategy fuzz or integration test with full harness".to_string()
            }
        }
        GapDifficulty::Blocked => {
            "Cannot unit-test — needs integration harness or external service".to_string()
        }
    }
}

// ─── Function name extraction ───────────────────────────────────────────────

/// Extract the enclosing function name from source context lines.
/// Scans for `fn <name>(` pattern, handling pub/async/unsafe prefixes.
pub fn extract_enclosing_function(source_lines: &[String]) -> Option<String> {
    for line in source_lines {
        let trimmed = line.trim();
        if let Some(fn_pos) = trimmed.find("fn ") {
            // Skip if inside a comment
            if fn_pos > 0 && trimmed[..fn_pos].contains("//") {
                continue;
            }
            let after_fn = &trimmed[fn_pos + 3..];
            if let Some(paren_pos) = after_fn.find('(') {
                let name = after_fn[..paren_pos].trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

// ─── Report builder ─────────────────────────────────────────────────────────

/// Build a complete AgentGapReport from oracle data.
///
/// `source_cache` maps (file_id, line) → source line text.
/// Gaps are sorted by `bang_for_buck` descending.
pub fn build_agent_gap_report(
    total_branches: usize,
    covered_branches: usize,
    uncovered: &[BranchId],
    file_paths: &HashMap<u64, PathBuf>,
    source_cache: &HashMap<(u64, u32), String>,
) -> AgentGapReport {
    let coverage_pct = if total_branches == 0 {
        1.0
    } else {
        covered_branches as f64 / total_branches as f64
    };

    // Count uncovered per file
    let mut uncovered_per_file: HashMap<u64, usize> = HashMap::new();
    for b in uncovered {
        *uncovered_per_file.entry(b.file_id).or_default() += 1;
    }

    let files_total = file_paths.len();
    let files_with_uncovered: std::collections::HashSet<u64> =
        uncovered.iter().map(|b| b.file_id).collect();
    let files_fully_covered = files_total.saturating_sub(files_with_uncovered.len());

    // Deduplicate by (file_id, line)
    let mut seen: std::collections::HashSet<(u64, u32)> = std::collections::HashSet::new();
    let mut gaps = Vec::new();

    for branch in uncovered {
        let key = (branch.file_id, branch.line);
        if !seen.insert(key) {
            continue;
        }

        let file = file_paths
            .get(&branch.file_id)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(format!("file_{}", branch.file_id)));

        // Gather source context: ±3 lines around the branch
        let mut context_lines = Vec::new();
        for offset in -3i32..=3 {
            let l = (branch.line as i32 + offset) as u32;
            if let Some(src) = source_cache.get(&(branch.file_id, l)) {
                context_lines.push(src.clone());
            }
        }

        let (difficulty, difficulty_reason) = classify_difficulty(&context_lines);
        let approach = suggest_approach(difficulty, &context_lines);
        let function = extract_enclosing_function(&context_lines);

        let file_uncovered = uncovered_per_file
            .get(&branch.file_id)
            .copied()
            .unwrap_or(0);
        let bang = compute_bang_for_buck(file_uncovered, total_branches);

        let branch_condition = source_cache.get(&(branch.file_id, branch.line)).cloned();

        let file_cov = 1.0 - (file_uncovered as f64 / total_branches.max(1) as f64);

        gaps.push(GapEntry {
            file,
            function,
            branch_line: branch.line,
            branch_condition,
            source_context: context_lines,
            uncovered_branches: file_uncovered,
            coverage_pct: file_cov,
            bang_for_buck: bang,
            difficulty,
            difficulty_reason,
            suggested_approach: approach,
            closest_existing_test: None,
        });
    }

    gaps.sort_by(|a, b| {
        b.bang_for_buck
            .partial_cmp(&a.bang_for_buck)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    AgentGapReport {
        summary: GapSummary {
            total_branches,
            covered_branches,
            coverage_pct,
            files_total,
            files_fully_covered,
        },
        gaps,
        blocked: Vec::new(),
        findings: None,
        security_summary: None,
        compound_analysis: None,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_gap_report_serializes_to_json() {
        let report = AgentGapReport {
            summary: GapSummary {
                total_branches: 100,
                covered_branches: 80,
                coverage_pct: 0.80,
                files_total: 5,
                files_fully_covered: 2,
            },
            gaps: vec![GapEntry {
                file: "src/main.rs".into(),
                function: Some("handle_request".into()),
                branch_line: 42,
                branch_condition: Some("match status {".into()),
                source_context: vec!["    match status {".into(), "        200 => ok(),".into()],
                uncovered_branches: 3,
                coverage_pct: 0.75,
                closest_existing_test: None,
                bang_for_buck: 0.85,
                difficulty: GapDifficulty::Medium,
                difficulty_reason: "needs mock HTTP client".into(),
                suggested_approach: "Test each match arm with mock responses".into(),
            }],
            blocked: vec![BlockedEntry {
                file: "src/rpc.rs".into(),
                uncovered_branches: 50,
                reason: "gRPC server required".into(),
            }],
            findings: None,
            security_summary: None,
            compound_analysis: None,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"coverage_pct\": 0.8"));
        assert!(json.contains("\"bang_for_buck\": 0.85"));
        assert!(json.contains("\"difficulty\": \"medium\""));
        assert!(json.contains("\"gRPC server required\""));
    }

    #[test]
    fn agent_gap_report_deserializes_without_findings() {
        let json = r#"{
            "summary": {"total_branches": 10, "covered_branches": 5, "coverage_pct": 0.5, "files_total": 1, "files_fully_covered": 0},
            "gaps": [],
            "blocked": []
        }"#;
        let report: AgentGapReport = serde_json::from_str(json).unwrap();
        assert!(report.findings.is_none());
        assert!(report.security_summary.is_none());
    }

    #[test]
    fn gap_difficulty_ordering() {
        assert!(GapDifficulty::Easy < GapDifficulty::Medium);
        assert!(GapDifficulty::Medium < GapDifficulty::Hard);
        assert!(GapDifficulty::Hard < GapDifficulty::Blocked);
    }

    #[test]
    fn classify_easy_branch() {
        let lines = vec![
            "    if x > 0 {".to_string(),
            "        return true;".to_string(),
        ];
        let (diff, reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Easy);
        assert!(reason.contains("simple conditional"));
    }

    #[test]
    fn classify_medium_needs_setup() {
        let lines = vec![
            "    match config.mode {".to_string(),
            "        Mode::Debug => {".to_string(),
        ];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn classify_hard_external_deps() {
        let lines = vec!["    let resp = client.get(url).await?;".to_string()];
        let (diff, reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
        assert!(
            reason.contains("async") || reason.contains("external") || reason.contains("needs")
        );
    }

    #[test]
    fn classify_hard_unsafe() {
        let lines = vec!["    unsafe { ptr::write(dest, val) }".to_string()];
        let (diff, _) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_hard_ffi() {
        let lines = vec!["    extern \"C\" fn callback(data: *mut c_void) {".to_string()];
        let (diff, _) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn bang_for_buck_high_when_many_uncovered() {
        let score = compute_bang_for_buck(20, 100);
        assert!((score - 0.2).abs() < 0.01);
    }

    #[test]
    fn bang_for_buck_capped_at_one() {
        let score = compute_bang_for_buck(50, 50);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn bang_for_buck_zero_when_no_uncovered() {
        let score = compute_bang_for_buck(0, 100);
        assert!((score - 0.0).abs() < 0.01);
    }

    #[test]
    fn suggested_approach_for_easy() {
        let approach = suggest_approach(GapDifficulty::Easy, &["if x > 0 {".into()]);
        assert!(approach.contains("unit test"));
    }

    #[test]
    fn suggested_approach_for_hard_fuzz() {
        let approach =
            suggest_approach(GapDifficulty::Hard, &["unsafe { parse_bytes(buf) }".into()]);
        assert!(approach.contains("fuzz"));
    }

    #[test]
    fn extract_function_name_from_context() {
        let lines = vec![
            "fn handle_request(req: Request) -> Response {".to_string(),
            "    if req.method == Method::GET {".to_string(),
        ];
        assert_eq!(
            extract_enclosing_function(&lines),
            Some("handle_request".to_string())
        );
    }

    #[test]
    fn extract_function_name_async() {
        let lines = vec![
            "    async fn process(data: &[u8]) -> Result<()> {".to_string(),
            "        let parsed = parse(data)?;".to_string(),
        ];
        assert_eq!(
            extract_enclosing_function(&lines),
            Some("process".to_string())
        );
    }

    #[test]
    fn extract_function_name_pub() {
        let lines = vec![
            "pub(crate) fn validate(input: &str) -> bool {".to_string(),
            "    !input.is_empty()".to_string(),
        ];
        assert_eq!(
            extract_enclosing_function(&lines),
            Some("validate".to_string())
        );
    }

    #[test]
    fn extract_function_name_none_when_absent() {
        let lines = vec![
            "    let x = 42;".to_string(),
            "    println!(\"{x}\");".to_string(),
        ];
        assert_eq!(extract_enclosing_function(&lines), None);
    }

    #[test]
    fn build_report_from_oracle_data() {
        let file_paths: HashMap<u64, PathBuf> = [
            (1, PathBuf::from("src/main.rs")),
            (2, PathBuf::from("src/lib.rs")),
        ]
        .into();

        let uncovered = vec![
            BranchId {
                file_id: 1,
                line: 10,
                col: 5,
                direction: 1,
                discriminator: 0,
                condition_index: None,
            },
            BranchId {
                file_id: 1,
                line: 20,
                col: 5,
                direction: 0,
                discriminator: 0,
                condition_index: None,
            },
        ];

        let source_cache: HashMap<(u64, u32), String> = [
            ((1, 10), "    if x > 0 {".to_string()),
            ((1, 20), "    match mode {".to_string()),
        ]
        .into();

        let report = build_agent_gap_report(4, 2, &uncovered, &file_paths, &source_cache);

        assert_eq!(report.summary.total_branches, 4);
        assert_eq!(report.summary.covered_branches, 2);
        assert!((report.summary.coverage_pct - 0.5).abs() < 0.01);
        assert_eq!(report.summary.files_total, 2);
        assert_eq!(report.summary.files_fully_covered, 1);
        assert_eq!(report.gaps.len(), 2);
        // Sorted by bang_for_buck descending (both same file, so equal)
        assert!(report.gaps[0].bang_for_buck >= report.gaps[1].bang_for_buck);
    }

    // ─── Additional branch coverage tests ──────────────────────────────────

    #[test]
    fn build_report_zero_total_branches_coverage_is_one() {
        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let uncovered: Vec<BranchId> = Vec::new();
        let source_cache: HashMap<(u64, u32), String> = HashMap::new();

        let report = build_agent_gap_report(0, 0, &uncovered, &file_paths, &source_cache);
        assert!((report.summary.coverage_pct - 1.0).abs() < 0.001);
        assert_eq!(report.gaps.len(), 0);
        assert_eq!(report.summary.files_fully_covered, 0);
    }

    #[test]
    fn build_report_unknown_file_id_uses_fallback_path() {
        // branch with file_id not in file_paths → path is "file_<id>"
        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let uncovered = vec![BranchId {
            file_id: 9999,
            line: 5,
            col: 0,
            direction: 0,
            discriminator: 0,
            condition_index: None,
        }];
        let source_cache: HashMap<(u64, u32), String> = HashMap::new();

        let report = build_agent_gap_report(5, 4, &uncovered, &file_paths, &source_cache);
        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].file.to_string_lossy().contains("9999"));
    }

    #[test]
    fn build_report_deduplicates_same_line_different_direction() {
        // Two branches with same (file_id, line) but different direction → only 1 gap entry.
        let file_paths: HashMap<u64, PathBuf> = [(1u64, PathBuf::from("src/a.rs"))].into();
        let uncovered = vec![
            BranchId {
                file_id: 1,
                line: 10,
                col: 0,
                direction: 0,
                discriminator: 0,
                condition_index: None,
            },
            BranchId {
                file_id: 1,
                line: 10,
                col: 0,
                direction: 1,
                discriminator: 0,
                condition_index: None,
            },
        ];
        let source_cache: HashMap<(u64, u32), String> = HashMap::new();
        let report = build_agent_gap_report(4, 2, &uncovered, &file_paths, &source_cache);
        assert_eq!(report.gaps.len(), 1);
    }

    #[test]
    fn build_report_all_files_covered_when_no_uncovered() {
        let file_paths: HashMap<u64, PathBuf> = [
            (1u64, PathBuf::from("src/a.rs")),
            (2u64, PathBuf::from("src/b.rs")),
        ]
        .into();
        let uncovered: Vec<BranchId> = Vec::new();
        let source_cache: HashMap<(u64, u32), String> = HashMap::new();
        let report = build_agent_gap_report(10, 10, &uncovered, &file_paths, &source_cache);
        assert_eq!(report.summary.files_fully_covered, 2);
    }

    #[test]
    fn build_report_source_context_sets_branch_condition() {
        let file_paths: HashMap<u64, PathBuf> = [(42u64, PathBuf::from("x.rs"))].into();
        let uncovered = vec![BranchId {
            file_id: 42,
            line: 7,
            col: 0,
            direction: 0,
            discriminator: 0,
            condition_index: None,
        }];
        let source_cache: HashMap<(u64, u32), String> =
            [((42u64, 7u32), "    if flag {".to_string())].into();
        let report = build_agent_gap_report(2, 1, &uncovered, &file_paths, &source_cache);
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(
            report.gaps[0].branch_condition.as_deref(),
            Some("    if flag {")
        );
    }

    #[test]
    fn classify_hard_tokio_spawn() {
        let lines = vec!["    tokio::spawn(async move { work() });".to_string()];
        let (diff, reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
        assert!(reason.contains("async") || reason.contains("external"));
    }

    #[test]
    fn classify_hard_tcp_listener() {
        let lines = vec!["    let listener = TcpListener::bind(addr).await?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_hard_reqwest() {
        let lines = vec!["    let resp = reqwest::get(url).await?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_medium_match_expression() {
        let lines = vec![
            "    match value {".to_string(),
            "        Ok(v) => v,".to_string(),
        ];
        let (diff, reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
        assert!(reason.contains("match") || reason.contains("needs"));
    }

    #[test]
    fn classify_medium_map_err() {
        let lines = vec!["    .map_err(|e| MyError::from(e))?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn classify_medium_env() {
        let lines = vec!["    let val = env::var(\"FOO\").unwrap_or_default();".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn classify_medium_filesystem() {
        let lines = vec!["    std::fs::read_to_string(path)?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn bang_for_buck_zero_when_zero_total() {
        let score = compute_bang_for_buck(5, 0);
        assert!((score - 0.0).abs() < 0.001);
    }

    #[test]
    fn suggest_approach_medium_match() {
        let lines = vec!["    match value {".to_string()];
        let approach = suggest_approach(GapDifficulty::Medium, &lines);
        assert!(approach.contains("match arm") || approach.contains("match"));
    }

    #[test]
    fn suggest_approach_medium_err() {
        let lines = vec!["    .map_err(|e| Err(e))?".to_string()];
        let approach = suggest_approach(GapDifficulty::Medium, &lines);
        assert!(approach.contains("error") || approach.contains("invalid"));
    }

    #[test]
    fn suggest_approach_medium_config() {
        let lines = vec!["    if config.debug { do_thing() }".to_string()];
        let approach = suggest_approach(GapDifficulty::Medium, &lines);
        assert!(approach.contains("config") || approach.contains("configuration"));
    }

    #[test]
    fn suggest_approach_medium_fallback() {
        // Lines that are Medium difficulty but don't match match/err/config
        let lines = vec!["    let x = ok_or(val, default)?;".to_string()];
        let approach = suggest_approach(GapDifficulty::Medium, &lines);
        assert!(!approach.is_empty());
    }

    #[test]
    fn suggest_approach_hard_async() {
        let lines = vec!["    let r = client.get(url).await?;".to_string()];
        let approach = suggest_approach(GapDifficulty::Hard, &lines);
        assert!(
            approach.contains("driller") || approach.contains("mock") || approach.contains("async")
        );
    }

    #[test]
    fn suggest_approach_hard_other() {
        // Hard but doesn't match unsafe/extern/await/grpc/tcp
        let lines = vec!["    hyper::server::listen(addr);".to_string()];
        let approach = suggest_approach(GapDifficulty::Hard, &lines);
        assert!(!approach.is_empty());
    }

    #[test]
    fn suggest_approach_blocked() {
        let approach = suggest_approach(GapDifficulty::Blocked, &[]);
        assert!(
            approach.contains("integration")
                || approach.contains("Cannot")
                || approach.contains("external")
        );
    }

    #[test]
    fn extract_function_name_skips_comment() {
        let lines = vec![
            "    // fn commented_out() {".to_string(),
            "    let x = 1;".to_string(),
        ];
        assert_eq!(extract_enclosing_function(&lines), None);
    }

    #[test]
    fn extract_function_name_empty_input() {
        assert_eq!(extract_enclosing_function(&[]), None);
    }

    #[test]
    fn extract_function_name_invalid_chars_in_name() {
        // fn followed by something with non-ident chars → should not match
        let lines = vec!["fn my-func(x: u32) -> u32 {".to_string()];
        assert_eq!(extract_enclosing_function(&lines), None);
    }

    #[test]
    fn suggest_approach_hard_grpc() {
        let lines = vec!["    grpc_client.call(request).await?;".to_string()];
        let approach = suggest_approach(GapDifficulty::Hard, &lines);
        assert!(approach.contains("driller") || approach.contains("mock"));
    }

    #[test]
    fn suggest_approach_hard_tcp() {
        let lines = vec!["    let stream = TcpStream::connect(addr)?;".to_string()];
        let approach = suggest_approach(GapDifficulty::Hard, &lines);
        assert!(approach.contains("driller") || approach.contains("mock"));
    }

    #[test]
    fn suggest_approach_hard_fallback() {
        // Hard difficulty but no unsafe/extern/await/grpc/tcp keywords
        let lines = vec!["    Command::new(\"ls\").output()?;".to_string()];
        let approach = suggest_approach(GapDifficulty::Hard, &lines);
        assert!(approach.contains("fuzz") || approach.contains("integration"));
    }

    #[test]
    fn classify_hard_process_command() {
        let lines = vec!["    Command::new(\"ls\").arg(\"-la\").output()?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_hard_udp_socket() {
        let lines = vec!["    let socket = UdpSocket::bind(\"0.0.0.0:0\")?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_hard_tonic() {
        let lines = vec!["    tonic::transport::Server::builder()".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_hard_hyper() {
        let lines = vec!["    hyper::Client::new().get(uri).await".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Hard);
    }

    #[test]
    fn classify_medium_unwrap_or() {
        let lines = vec!["    let v = opt.unwrap_or(default);".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn classify_medium_ok_or() {
        let lines = vec!["    let r = opt.ok_or(Error::Missing)?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn classify_medium_file_open() {
        let lines = vec!["    let f = File::open(path)?;".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn classify_medium_config_access() {
        let lines = vec!["    if config.verbose {".to_string()];
        let (diff, _reason) = classify_difficulty(&lines);
        assert_eq!(diff, GapDifficulty::Medium);
    }

    #[test]
    fn suggest_approach_medium_env() {
        let lines = vec!["    let home = env::var(\"HOME\")?;".to_string()];
        let approach = suggest_approach(GapDifficulty::Medium, &lines);
        assert!(approach.contains("config") || approach.contains("environment"));
    }

    #[test]
    fn build_report_source_context_collects_nearby_lines() {
        let file_paths: HashMap<u64, PathBuf> = [(1u64, PathBuf::from("a.rs"))].into();
        let uncovered = vec![BranchId {
            file_id: 1,
            line: 5,
            col: 0,
            direction: 0,
            discriminator: 0,
            condition_index: None,
        }];
        let source_cache: HashMap<(u64, u32), String> = [
            ((1, 2), "line 2".into()),
            ((1, 3), "line 3".into()),
            ((1, 4), "line 4".into()),
            ((1, 5), "if x > 0 {".into()),
            ((1, 6), "line 6".into()),
            ((1, 7), "line 7".into()),
            ((1, 8), "line 8".into()),
        ]
        .into();
        let report = build_agent_gap_report(10, 9, &uncovered, &file_paths, &source_cache);
        assert_eq!(report.gaps.len(), 1);
        // ±3 lines around line 5 = lines 2-8 = 7 context lines
        assert_eq!(report.gaps[0].source_context.len(), 7);
    }

    #[test]
    fn gap_difficulty_serde_roundtrip() {
        for diff in [
            GapDifficulty::Easy,
            GapDifficulty::Medium,
            GapDifficulty::Hard,
            GapDifficulty::Blocked,
        ] {
            let json = serde_json::to_string(&diff).unwrap();
            let back: GapDifficulty = serde_json::from_str(&json).unwrap();
            assert_eq!(diff, back);
        }
    }

    #[test]
    fn build_report_gaps_sorted_by_bang_for_buck_descending() {
        let file_paths: HashMap<u64, PathBuf> = [
            (1u64, PathBuf::from("small.rs")),
            (2u64, PathBuf::from("big.rs")),
        ]
        .into();
        // 10 uncovered in file 1, 1 uncovered in file 2
        let mut uncovered = Vec::new();
        for line in 1..=10 {
            uncovered.push(BranchId {
                file_id: 1,
                line,
                col: 0,
                direction: 0,
                discriminator: 0,
                condition_index: None,
            });
        }
        uncovered.push(BranchId {
            file_id: 2,
            line: 1,
            col: 0,
            direction: 0,
            discriminator: 0,
            condition_index: None,
        });
        let source_cache: HashMap<(u64, u32), String> = HashMap::new();
        let report = build_agent_gap_report(100, 89, &uncovered, &file_paths, &source_cache);
        // file 1 has higher bang_for_buck (10/100=0.1 > 1/100=0.01)
        // so file 1 gaps should come first
        assert_eq!(report.gaps.len(), 11);
        assert!(report.gaps[0].bang_for_buck >= report.gaps[10].bang_for_buck);
    }
}
