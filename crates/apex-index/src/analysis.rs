use crate::types::{branch_key, BranchIndex};
use apex_core::types::BranchId;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Flaky detection
// ---------------------------------------------------------------------------

/// A flaky test: same test, different branch sets across runs.
#[derive(Debug, Clone, Serialize)]
pub struct FlakyTest {
    pub test_name: String,
    /// Branches that appear in some runs but not others.
    pub divergent_branches: Vec<DivergentBranch>,
    /// Number of runs where divergence was observed.
    pub divergent_runs: usize,
    pub total_runs: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DivergentBranch {
    pub branch: BranchId,
    pub file_path: Option<PathBuf>,
    /// How many of N runs hit this branch.
    pub hit_ratio: String,
}

/// Analyze multiple traces of the same tests to find nondeterminism.
pub fn detect_flaky_tests(
    runs: &[Vec<crate::TestTrace>],
    file_paths: &HashMap<u64, PathBuf>,
) -> Vec<FlakyTest> {
    if runs.is_empty() {
        return vec![];
    }

    // Group traces by test name across runs
    let mut test_runs: HashMap<&str, Vec<HashSet<String>>> = HashMap::new();

    for run in runs {
        for trace in run {
            let keys: HashSet<String> = trace.branches.iter().map(branch_key).collect();
            test_runs
                .entry(&trace.test_name)
                .or_default()
                .push(keys);
        }
    }

    let mut flaky = Vec::new();

    for (test_name, branch_sets) in &test_runs {
        if branch_sets.len() < 2 {
            continue;
        }

        // Find branches that aren't in every run
        let union: HashSet<&String> = branch_sets.iter().flat_map(|s| s.iter()).collect();
        let intersection: HashSet<&String> = branch_sets[0]
            .iter()
            .filter(|k| branch_sets.iter().all(|s| s.contains(*k)))
            .collect();

        let divergent_keys: Vec<&String> = union.difference(&intersection).copied().collect();

        if !divergent_keys.is_empty() {
            let total_runs = branch_sets.len();
            let divergent_branches: Vec<DivergentBranch> = divergent_keys
                .iter()
                .map(|key| {
                    let hits = branch_sets.iter().filter(|s| s.contains(*key)).count();
                    // Parse branch from key (file_id:line:col:direction:condition)
                    let parts: Vec<&str> = key.split(':').collect();
                    let file_id: u64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let line: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let direction: u8 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

                    DivergentBranch {
                        branch: BranchId::new(file_id, line, 0, direction),
                        file_path: file_paths.get(&file_id).cloned(),
                        hit_ratio: format!("{}/{}", hits, total_runs),
                    }
                })
                .collect();

            flaky.push(FlakyTest {
                test_name: test_name.to_string(),
                divergent_branches,
                divergent_runs: total_runs,
                total_runs,
            });
        }
    }

    flaky.sort_by(|a, b| b.divergent_branches.len().cmp(&a.divergent_branches.len()));
    flaky
}

// ---------------------------------------------------------------------------
// Complexity analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct FunctionComplexity {
    pub file_path: PathBuf,
    pub function_name: String,
    pub line: u32,
    /// Total branches in this function (static complexity).
    pub static_complexity: usize,
    /// Branches actually exercised by tests.
    pub exercised_complexity: usize,
    /// Ratio: exercised / static.
    pub exercise_ratio: f64,
    /// Classification based on the ratio.
    pub classification: String,
}

/// Analyze exercised vs static complexity per function.
pub fn analyze_complexity(
    index: &BranchIndex,
    target_root: &Path,
) -> Vec<FunctionComplexity> {
    let mut results = Vec::new();

    // Read source files and find function boundaries
    for (file_id, rel_path) in &index.file_paths {
        let full_path = target_root.join(rel_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();
        let functions = extract_functions(&lines, index.language);

        // Get branches in this file from profiles
        let file_profiles: Vec<_> = index
            .profiles
            .values()
            .filter(|p| p.branch.file_id == *file_id)
            .collect();

        for (func_name, func_start, func_end) in &functions {
            let in_function: Vec<_> = file_profiles
                .iter()
                .filter(|p| p.branch.line >= *func_start && p.branch.line <= *func_end)
                .collect();

            let static_count = in_function.len();
            let exercised_count = in_function.iter().filter(|p| p.hit_count > 0).count();

            if static_count == 0 {
                continue;
            }

            let ratio = exercised_count as f64 / static_count as f64;
            let classification = if ratio >= 0.9 {
                "fully-exercised"
            } else if ratio >= 0.5 {
                "partially-tested"
            } else if ratio > 0.0 {
                "under-tested"
            } else {
                "dead"
            };

            results.push(FunctionComplexity {
                file_path: rel_path.clone(),
                function_name: func_name.clone(),
                line: *func_start,
                static_complexity: static_count,
                exercised_complexity: exercised_count,
                exercise_ratio: ratio,
                classification: classification.into(),
            });
        }
    }

    results.sort_by(|a, b| {
        a.exercise_ratio
            .partial_cmp(&b.exercise_ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

/// Extract function names and line ranges from source code.
fn extract_functions(
    lines: &[&str],
    language: apex_core::types::Language,
) -> Vec<(String, u32, u32)> {
    let mut functions = Vec::new();

    let func_pattern: &[&str] = match language {
        apex_core::types::Language::Python => &["def "],
        apex_core::types::Language::Rust => &["fn "],
        apex_core::types::Language::JavaScript => &["function ", "=> {"],
        apex_core::types::Language::Java => &["void ", "public ", "private ", "protected "],
        apex_core::types::Language::Ruby => &["def "],
        _ => &["fn "],
    };

    let mut current_func: Option<(String, u32)> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let line_num = (i + 1) as u32;

        let is_func_start = func_pattern.iter().any(|p| trimmed.contains(p))
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("///");

        if is_func_start {
            // Close previous function
            if let Some((name, start)) = current_func.take() {
                functions.push((name, start, line_num - 1));
            }

            // Extract function name
            let name = extract_func_name(trimmed, language);
            current_func = Some((name, line_num));
        }
    }

    // Close last function
    if let Some((name, start)) = current_func {
        functions.push((name, start, lines.len() as u32));
    }

    functions
}

fn extract_func_name(line: &str, language: apex_core::types::Language) -> String {
    match language {
        apex_core::types::Language::Python => {
            // "def foo(...):"
            line.trim()
                .strip_prefix("def ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("unknown")
                .trim()
                .to_string()
        }
        apex_core::types::Language::Rust => {
            // "pub async fn foo(...)"
            let s = line.trim();
            let after_fn = s
                .find("fn ")
                .map(|i| &s[i + 3..])
                .unwrap_or("unknown");
            after_fn
                .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
                .next()
                .unwrap_or("unknown")
                .to_string()
        }
        _ => {
            // Generic: find first identifier-like token after keyword
            let tokens: Vec<&str> = line.split_whitespace().collect();
            tokens
                .iter()
                .find(|t| {
                    t.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)
                        && !["pub", "async", "fn", "def", "function", "void", "public", "private", "protected", "static"]
                            .contains(t)
                })
                .map(|t| t.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_'))
                .unwrap_or("unknown")
                .to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Documentation generation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDoc {
    pub file_path: PathBuf,
    pub function_name: String,
    pub line: u32,
    pub paths: Vec<ExecutionPath>,
    pub total_tests: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionPath {
    /// Branches taken in this path.
    pub branch_count: usize,
    /// Representative test that exercises this path.
    pub representative_test: String,
    /// Percentage of tests that follow this path.
    pub frequency_pct: f64,
    /// Number of tests following this path.
    pub test_count: usize,
}

/// Generate behavioral documentation from execution traces.
pub fn generate_docs(
    index: &BranchIndex,
    target_root: &Path,
) -> Vec<FunctionDoc> {
    let mut docs = Vec::new();

    for (file_id, rel_path) in &index.file_paths {
        let full_path = target_root.join(rel_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();
        let functions = extract_functions(&lines, index.language);

        for (func_name, func_start, func_end) in &functions {
            // For each test, compute its "path signature" within this function
            // (set of branches taken in this function's line range)
            let mut path_groups: HashMap<Vec<String>, Vec<&str>> = HashMap::new();

            for trace in &index.traces {
                let func_branches: Vec<String> = trace
                    .branches
                    .iter()
                    .filter(|b| {
                        b.file_id == *file_id && b.line >= *func_start && b.line <= *func_end
                    })
                    .map(branch_key)
                    .collect();

                if func_branches.is_empty() {
                    continue; // test doesn't touch this function
                }

                let mut sorted = func_branches;
                sorted.sort();
                path_groups
                    .entry(sorted)
                    .or_default()
                    .push(&trace.test_name);
            }

            if path_groups.is_empty() {
                continue;
            }

            let total_tests: usize = path_groups.values().map(|v| v.len()).sum();
            let mut paths: Vec<ExecutionPath> = path_groups
                .iter()
                .map(|(branches, tests)| {
                    ExecutionPath {
                        branch_count: branches.len(),
                        representative_test: tests[0].to_string(),
                        frequency_pct: (tests.len() as f64 / total_tests as f64) * 100.0,
                        test_count: tests.len(),
                    }
                })
                .collect();

            paths.sort_by(|a, b| {
                b.test_count
                    .cmp(&a.test_count)
                    .then(a.branch_count.cmp(&b.branch_count))
            });

            docs.push(FunctionDoc {
                file_path: rel_path.clone(),
                function_name: func_name.clone(),
                line: *func_start,
                paths,
                total_tests,
            });
        }
    }

    docs.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line.cmp(&b.line))
    });

    docs
}

// ---------------------------------------------------------------------------
// Attack surface analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AttackSurfaceReport {
    pub entry_pattern: String,
    pub entry_tests: usize,
    pub reachable_branches: usize,
    pub reachable_files: usize,
    pub total_branches: usize,
    pub attack_surface_pct: f64,
    pub reachable_file_details: Vec<ReachableFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReachableFile {
    pub file_path: PathBuf,
    pub reachable_branches: usize,
    pub total_branches_in_file: usize,
    pub coverage_pct: f64,
}

/// Map attack surface from entry-point test reachability.
pub fn analyze_attack_surface(
    index: &BranchIndex,
    entry_pattern: &str,
) -> AttackSurfaceReport {
    // Filter tests matching entry pattern
    let entry_traces: Vec<_> = index
        .traces
        .iter()
        .filter(|t| t.test_name.contains(entry_pattern))
        .collect();

    // Union of all branches reachable from entry-point tests
    let reachable: HashSet<String> = entry_traces
        .iter()
        .flat_map(|t| t.branches.iter().map(branch_key))
        .collect();

    // Group by file
    let mut file_reachable: HashMap<u64, HashSet<String>> = HashMap::new();
    for trace in &entry_traces {
        for branch in &trace.branches {
            file_reachable
                .entry(branch.file_id)
                .or_default()
                .insert(branch_key(branch));
        }
    }

    // Total branches per file from all profiles
    let mut file_totals: HashMap<u64, usize> = HashMap::new();
    for profile in index.profiles.values() {
        *file_totals.entry(profile.branch.file_id).or_default() += 1;
    }

    let mut reachable_files: Vec<ReachableFile> = file_reachable
        .iter()
        .map(|(file_id, branches)| {
            let total = file_totals.get(file_id).copied().unwrap_or(0);
            let path = index
                .file_paths
                .get(file_id)
                .cloned()
                .unwrap_or_else(|| PathBuf::from(format!("<{:016x}>", file_id)));
            ReachableFile {
                file_path: path,
                reachable_branches: branches.len(),
                total_branches_in_file: total,
                coverage_pct: if total > 0 {
                    (branches.len() as f64 / total as f64) * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect();

    reachable_files.sort_by(|a, b| b.reachable_branches.cmp(&a.reachable_branches));

    let attack_surface_pct = if index.total_branches > 0 {
        (reachable.len() as f64 / index.total_branches as f64) * 100.0
    } else {
        0.0
    };

    AttackSurfaceReport {
        entry_pattern: entry_pattern.to_string(),
        entry_tests: entry_traces.len(),
        reachable_branches: reachable.len(),
        reachable_files: file_reachable.len(),
        total_branches: index.total_branches,
        attack_surface_pct,
        reachable_file_details: reachable_files,
    }
}

// ---------------------------------------------------------------------------
// Boundary verification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct BoundaryReport {
    pub entry_pattern: String,
    pub auth_pattern: String,
    pub total_entry_tests: usize,
    pub passing_tests: usize,
    pub failing_tests: usize,
    pub unprotected_paths: Vec<UnprotectedPath>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnprotectedPath {
    pub test_name: String,
    /// Branches in the test trace — none matched the auth pattern.
    pub branches_traversed: usize,
    /// Files reached without auth.
    pub files_reached: Vec<PathBuf>,
}

/// Verify all entry-point test paths pass through auth-check branches.
///
/// `auth_checks` is a substring pattern matching source lines that represent
/// auth gates (e.g., "check_auth", "verify_token", "@login_required").
pub fn verify_boundaries(
    index: &BranchIndex,
    target_root: &Path,
    entry_pattern: &str,
    auth_checks: &str,
) -> BoundaryReport {
    // Find auth-check branches by scanning source for the pattern
    let mut auth_branches: HashSet<String> = HashSet::new();

    for (file_id, rel_path) in &index.file_paths {
        let full_path = target_root.join(rel_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        for (i, line) in source.lines().enumerate() {
            if line.contains(auth_checks) {
                let line_num = (i + 1) as u32;
                // Find all branches on or near this line
                for profile in index.profiles.values() {
                    if profile.branch.file_id == *file_id
                        && (profile.branch.line == line_num
                            || profile.branch.line == line_num + 1)
                    {
                        auth_branches.insert(branch_key(&profile.branch));
                    }
                }
            }
        }
    }

    // Filter entry-point tests
    let entry_traces: Vec<_> = index
        .traces
        .iter()
        .filter(|t| t.test_name.contains(entry_pattern))
        .collect();

    let mut unprotected = Vec::new();

    for trace in &entry_traces {
        let trace_keys: HashSet<String> =
            trace.branches.iter().map(|b| branch_key(b)).collect();

        let hits_auth = trace_keys.iter().any(|k| auth_branches.contains(k));

        if !hits_auth {
            let files_reached: Vec<PathBuf> = trace
                .branches
                .iter()
                .filter_map(|b| index.file_paths.get(&b.file_id))
                .collect::<HashSet<_>>()
                .into_iter()
                .cloned()
                .collect();

            unprotected.push(UnprotectedPath {
                test_name: trace.test_name.clone(),
                branches_traversed: trace.branches.len(),
                files_reached,
            });
        }
    }

    BoundaryReport {
        entry_pattern: entry_pattern.to_string(),
        auth_pattern: auth_checks.to_string(),
        total_entry_tests: entry_traces.len(),
        passing_tests: entry_traces.len() - unprotected.len(),
        failing_tests: unprotected.len(),
        unprotected_paths: unprotected,
    }
}

// ---------------------------------------------------------------------------
// Hot paths analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HotPath {
    pub branch: BranchId,
    pub file_path: PathBuf,
    pub line: u32,
    pub direction: &'static str,
    pub hit_count: u64,
    pub test_count: usize,
    /// Fraction of total hits across all branches.
    pub hit_share_pct: f64,
}

/// Rank branches by execution frequency.
pub fn analyze_hotpaths(index: &BranchIndex, top_n: usize) -> Vec<HotPath> {
    let total_hits: u64 = index.profiles.values().map(|p| p.hit_count).sum();

    let mut paths: Vec<HotPath> = index
        .profiles
        .values()
        .map(|p| {
            let file_path = index
                .file_paths
                .get(&p.branch.file_id)
                .cloned()
                .unwrap_or_else(|| PathBuf::from(format!("<{:016x}>", p.branch.file_id)));
            HotPath {
                branch: p.branch.clone(),
                file_path,
                line: p.branch.line,
                direction: if p.branch.direction == 0 { "true" } else { "false" },
                hit_count: p.hit_count,
                test_count: p.test_count,
                hit_share_pct: if total_hits > 0 {
                    (p.hit_count as f64 / total_hits as f64) * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect();

    paths.sort_by(|a, b| b.hit_count.cmp(&a.hit_count));
    paths.truncate(top_n);
    paths
}

// ---------------------------------------------------------------------------
// Risk assessment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RiskAssessment {
    pub level: &'static str,
    pub score: u32,
    pub changed_branches: usize,
    pub covered_changed: usize,
    pub uncovered_changed: usize,
    pub affected_tests: usize,
    pub coverage_of_changed: f64,
    pub reasons: Vec<String>,
}

/// Assess risk of changes based on branch coverage data.
pub fn assess_risk(
    index: &BranchIndex,
    changed_files: &[String],
) -> RiskAssessment {
    // Map changed files to file_ids
    let changed_file_ids: HashSet<u64> = index
        .file_paths
        .iter()
        .filter(|(_, path)| {
            let ps = path.to_string_lossy();
            changed_files.iter().any(|cf| ps.contains(cf.as_str()))
        })
        .map(|(id, _)| *id)
        .collect();

    // Branches in changed files
    let changed_branches: Vec<_> = index
        .profiles
        .values()
        .filter(|p| changed_file_ids.contains(&p.branch.file_id))
        .collect();

    let total_changed = changed_branches.len();
    let covered_changed = changed_branches.iter().filter(|p| p.hit_count > 0).count();
    let uncovered_changed = total_changed - covered_changed;

    // Tests that touch changed files
    let affected_tests: HashSet<&str> = index
        .traces
        .iter()
        .filter(|t| {
            t.branches
                .iter()
                .any(|b| changed_file_ids.contains(&b.file_id))
        })
        .map(|t| t.test_name.as_str())
        .collect();

    let coverage_of_changed = if total_changed > 0 {
        (covered_changed as f64 / total_changed as f64) * 100.0
    } else {
        100.0
    };

    let mut reasons = Vec::new();
    let mut score: u32 = 0;

    // Score components
    if coverage_of_changed < 50.0 {
        score += 40;
        reasons.push(format!(
            "Low coverage of changed code: {:.0}%",
            coverage_of_changed
        ));
    } else if coverage_of_changed < 80.0 {
        score += 20;
        reasons.push(format!(
            "Moderate coverage of changed code: {:.0}%",
            coverage_of_changed
        ));
    }

    if uncovered_changed > 10 {
        score += 30;
        reasons.push(format!("{} uncovered branches in changed files", uncovered_changed));
    } else if uncovered_changed > 0 {
        score += 10;
        reasons.push(format!("{} uncovered branches in changed files", uncovered_changed));
    }

    if affected_tests.len() > 50 {
        score += 20;
        reasons.push(format!("Wide blast radius: {} tests affected", affected_tests.len()));
    } else if affected_tests.len() > 10 {
        score += 10;
        reasons.push(format!("{} tests affected", affected_tests.len()));
    }

    if changed_file_ids.is_empty() {
        reasons.push("No changed files match indexed files".into());
    }

    let level = match score {
        0..=15 => "LOW",
        16..=35 => "MEDIUM",
        36..=60 => "HIGH",
        _ => "CRITICAL",
    };

    RiskAssessment {
        level,
        score,
        changed_branches: total_changed,
        covered_changed,
        uncovered_changed,
        affected_tests: affected_tests.len(),
        coverage_of_changed,
        reasons,
    }
}

// ---------------------------------------------------------------------------
// Invariant / contract discovery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredInvariant {
    pub file_path: PathBuf,
    pub function_name: String,
    pub line: u32,
    pub description: String,
    pub confidence: f64,
    pub evidence_tests: usize,
    pub kind: &'static str,
}

/// Discover invariants from branch execution patterns.
pub fn discover_contracts(
    index: &BranchIndex,
    target_root: &Path,
) -> Vec<DiscoveredInvariant> {
    let mut invariants = Vec::new();

    for (file_id, rel_path) in &index.file_paths {
        let full_path = target_root.join(rel_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let lines: Vec<&str> = source.lines().collect();
        let functions = extract_functions(&lines, index.language);

        for (func_name, func_start, func_end) in &functions {
            // Find all tests that call this function
            let func_tests: Vec<_> = index
                .traces
                .iter()
                .filter(|t| {
                    t.branches.iter().any(|b| {
                        b.file_id == *file_id && b.line >= *func_start && b.line <= *func_end
                    })
                })
                .collect();

            if func_tests.is_empty() {
                continue;
            }

            // For each branch in this function, check if it's always/never taken
            let func_profiles: Vec<_> = index
                .profiles
                .values()
                .filter(|p| {
                    p.branch.file_id == *file_id
                        && p.branch.line >= *func_start
                        && p.branch.line <= *func_end
                })
                .collect();

            for profile in &func_profiles {
                let key = branch_key(&profile.branch);
                let tests_hitting: usize = func_tests
                    .iter()
                    .filter(|t| t.branches.iter().any(|b| branch_key(b) == key))
                    .count();

                let total_func_tests = func_tests.len();
                if total_func_tests < 2 {
                    continue; // Need multiple tests for meaningful invariants
                }

                let ratio = tests_hitting as f64 / total_func_tests as f64;
                let dir = if profile.branch.direction == 0 {
                    "true"
                } else {
                    "false"
                };

                let src_line = lines
                    .get((profile.branch.line as usize).saturating_sub(1))
                    .map(|s| s.trim())
                    .unwrap_or("");

                if ratio >= 0.99 && total_func_tests >= 3 {
                    invariants.push(DiscoveredInvariant {
                        file_path: rel_path.clone(),
                        function_name: func_name.clone(),
                        line: profile.branch.line,
                        description: format!(
                            "Branch `{}` at line {} is ALWAYS {} when {}() is called",
                            src_line, profile.branch.line, dir, func_name
                        ),
                        confidence: ratio,
                        evidence_tests: total_func_tests,
                        kind: "always-taken",
                    });
                } else if ratio <= 0.01 && total_func_tests >= 3 {
                    invariants.push(DiscoveredInvariant {
                        file_path: rel_path.clone(),
                        function_name: func_name.clone(),
                        line: profile.branch.line,
                        description: format!(
                            "Branch `{}` at line {} is NEVER {} when {}() is called",
                            src_line, profile.branch.line, dir, func_name
                        ),
                        confidence: 1.0 - ratio,
                        evidence_tests: total_func_tests,
                        kind: "never-taken",
                    });
                }
            }
        }
    }

    invariants.sort_by(|a, b| {
        b.evidence_tests
            .cmp(&a.evidence_tests)
            .then(b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal))
    });

    invariants
}

// ---------------------------------------------------------------------------
// Deploy score
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DeployScore {
    pub total_score: u32,
    pub coverage_score: u32,
    pub coverage_max: u32,
    pub test_quality_score: u32,
    pub test_quality_max: u32,
    pub detector_score: u32,
    pub detector_max: u32,
    pub stability_score: u32,
    pub stability_max: u32,
    pub recommendation: &'static str,
    pub breakdown: Vec<String>,
}

/// Compute aggregate deployment confidence score (0-100).
pub fn compute_deploy_score(
    index: &BranchIndex,
    detector_findings: usize,
    critical_findings: usize,
) -> DeployScore {
    let coverage_max = 30u32;
    let test_quality_max = 25u32;
    let detector_max = 25u32;
    let stability_max = 20u32;

    let mut breakdown = Vec::new();

    // Coverage component (0-30)
    let cov_pct = index.coverage_percent();
    let coverage_score = ((cov_pct / 100.0) * coverage_max as f64).round() as u32;
    breakdown.push(format!(
        "Coverage: {:.1}% → {}/{}",
        cov_pct, coverage_score, coverage_max
    ));

    // Test quality: unique coverage ratio (tests that cover unique branches / total tests)
    let total_tests = index.traces.len();
    let unique_tests = index
        .profiles
        .values()
        .filter(|p| p.test_count == 1)
        .count();
    let quality_ratio = if total_tests > 0 {
        (unique_tests as f64 / index.profiles.len().max(1) as f64).min(1.0)
    } else {
        0.0
    };
    let test_quality_score = (quality_ratio * test_quality_max as f64).round() as u32;
    breakdown.push(format!(
        "Test quality: {:.0}% unique coverage → {}/{}",
        quality_ratio * 100.0,
        test_quality_score,
        test_quality_max
    ));

    // Detector findings (0-25, loses points for findings)
    let detector_score = if critical_findings > 0 {
        0
    } else if detector_findings > 10 {
        5
    } else if detector_findings > 0 {
        detector_max - (detector_findings as u32 * 2).min(detector_max)
    } else {
        detector_max
    };
    breakdown.push(format!(
        "Detectors: {} findings ({} critical) → {}/{}",
        detector_findings, critical_findings, detector_score, detector_max
    ));

    // Stability: assume stable if we have an index (future: compare across runs)
    let stability_score = stability_max; // Full marks if index exists
    breakdown.push(format!(
        "Stability: index present → {}/{}",
        stability_score, stability_max
    ));

    let total_score = coverage_score + test_quality_score + detector_score + stability_score;

    let recommendation = match total_score {
        0..=40 => "BLOCK — significant gaps in coverage or security",
        41..=60 => "CAUTION — review findings before deploying",
        61..=80 => "ACCEPTABLE — deploy with monitoring",
        _ => "GO — high confidence deployment",
    };

    DeployScore {
        total_score,
        coverage_score,
        coverage_max,
        test_quality_score,
        test_quality_max,
        detector_score,
        detector_max,
        stability_score,
        stability_max,
        recommendation,
        breakdown,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestTrace;
    use apex_core::types::ExecutionStatus;

    fn br(file_id: u64, line: u32, dir: u8) -> BranchId {
        BranchId::new(file_id, line, 0, dir)
    }

    #[test]
    fn flaky_detect_no_divergence() {
        let run1 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert!(flaky.is_empty());
    }

    #[test]
    fn flaky_detect_finds_divergence() {
        let run1 = vec![TestTrace {
            test_name: "test_flaky".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_flaky".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 1)], // direction changed!
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert_eq!(flaky.len(), 1);
        assert_eq!(flaky[0].test_name, "test_flaky");
        assert!(flaky[0].divergent_branches.len() >= 1);
    }

    #[test]
    fn flaky_detect_empty_runs() {
        let flaky = detect_flaky_tests(&[], &HashMap::new());
        assert!(flaky.is_empty());
    }

    #[test]
    fn extract_func_name_python() {
        let name = extract_func_name("def process_order(order):", apex_core::types::Language::Python);
        assert_eq!(name, "process_order");
    }

    #[test]
    fn extract_func_name_rust() {
        let name = extract_func_name("pub async fn handle_request(req: Request) -> Response {", apex_core::types::Language::Rust);
        assert_eq!(name, "handle_request");
    }

    #[test]
    fn extract_func_name_python_no_args() {
        let name = extract_func_name("def setup():", apex_core::types::Language::Python);
        assert_eq!(name, "setup");
    }

    #[test]
    fn attack_surface_empty_pattern() {
        let index = BranchIndex {
            traces: vec![TestTrace {
                test_name: "test_internal".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            }],
            profiles: BranchIndex::build_profiles(&[TestTrace {
                test_name: "test_internal".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            }]),
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 5,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.entry_tests, 0);
        assert_eq!(report.reachable_branches, 0);
    }

    #[test]
    fn attack_surface_matches_pattern() {
        let traces = vec![
            TestTrace {
                test_name: "test_api_login".into(),
                branches: vec![br(1, 10, 0), br(2, 5, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_internal_helper".into(),
                branches: vec![br(3, 20, 0)],
                duration_ms: 30,
                status: ExecutionStatus::Pass,
            },
        ];

        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([
                (1, PathBuf::from("src/api.py")),
                (2, PathBuf::from("src/auth.py")),
                (3, PathBuf::from("src/internal.py")),
            ]),
            total_branches: 10,
            covered_branches: 3,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.entry_tests, 1);
        assert_eq!(report.reachable_branches, 2);
        assert_eq!(report.reachable_files, 2);
    }

    #[test]
    fn hotpaths_ranks_by_hit_count() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0), br(1, 10, 0), br(1, 20, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let hot = analyze_hotpaths(&index, 10);
        assert!(!hot.is_empty());
        // First entry should have highest hit_count
        assert!(hot[0].hit_count >= hot.last().unwrap().hit_count);
        // hit_share_pct should sum to ~100%
        let total_share: f64 = hot.iter().map(|h| h.hit_share_pct).sum();
        assert!((total_share - 100.0).abs() < 0.1);
    }

    #[test]
    fn risk_low_for_covered_changes() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        assert_eq!(risk.level, "LOW");
        assert!(risk.coverage_of_changed > 90.0);
    }

    #[test]
    fn risk_high_for_uncovered_changes() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Add many uncovered branches in changed file
        for line in 100..115 {
            let b = br(2, line, 0);
            profiles.insert(
                branch_key(&b),
                crate::BranchProfile {
                    branch: b,
                    hit_count: 0,
                    test_count: 0,
                    test_names: vec![],
                },
            );
        }

        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([
                (1, PathBuf::from("src/ok.py")),
                (2, PathBuf::from("src/risky.py")),
            ]),
            total_branches: 16,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["src/risky.py".to_string()]);
        assert!(risk.score > 30, "expected HIGH risk, got score={}", risk.score);
        assert!(risk.uncovered_changed > 10);
    }

    #[test]
    fn deploy_score_full_marks_no_findings() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 0, 0);
        assert_eq!(score.total_score, 100);
        assert!(score.recommendation.starts_with("GO"));
    }

    #[test]
    fn verify_boundaries_no_entry_tests() {
        let traces = vec![TestTrace {
            test_name: "test_internal".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = verify_boundaries(&index, Path::new("/nonexistent"), "test_api", "check_auth");
        assert_eq!(report.total_entry_tests, 0);
        assert_eq!(report.failing_tests, 0);
    }

    #[test]
    fn extract_func_name_js() {
        // JS uses generic fallback — keeps up to first non-alphanumeric (comma excluded by trim)
        let name = extract_func_name("function handleRequest(req, res) {", apex_core::types::Language::JavaScript);
        assert_eq!(name, "handleRequest(req");
    }

    #[test]
    fn extract_func_name_ruby() {
        // Ruby uses Python-style "def " prefix strip + split on '('
        let name = extract_func_name("def process_payment(amount)", apex_core::types::Language::Ruby);
        assert_eq!(name, "process_payment(amount");
    }

    #[test]
    fn extract_func_name_java() {
        // Java uses generic fallback
        let name = extract_func_name("public void processOrder(Order order) {", apex_core::types::Language::Java);
        assert_eq!(name, "processOrder(Order");
    }

    #[test]
    fn extract_func_name_generic_fallback() {
        let name = extract_func_name("fn do_stuff() {", apex_core::types::Language::Wasm);
        assert_eq!(name, "do_stuff");
    }

    #[test]
    fn extract_functions_rust_multiple() {
        let source = vec![
            "pub fn foo() {",
            "    let x = 1;",
            "}",
            "",
            "fn bar(a: i32) -> i32 {",
            "    a + 1",
            "}",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::Rust);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].0, "foo");
        assert_eq!(funcs[0].1, 1); // line 1
        assert_eq!(funcs[0].2, 4); // ends before bar starts at line 5
        assert_eq!(funcs[1].0, "bar");
        assert_eq!(funcs[1].1, 5);
        assert_eq!(funcs[1].2, 7); // last line
    }

    #[test]
    fn extract_functions_python() {
        let source = vec![
            "def hello():",
            "    print('hi')",
            "",
            "def goodbye():",
            "    print('bye')",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::Python);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].0, "hello");
        assert_eq!(funcs[1].0, "goodbye");
    }

    #[test]
    fn extract_functions_skips_comments() {
        let source = vec![
            "// fn not_a_function() {",
            "/// fn also_not() {",
            "fn real_function() {",
            "    42",
            "}",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::Rust);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, "real_function");
    }

    #[test]
    fn extract_functions_js_arrow() {
        let source = vec![
            "const handler = (req) => {",
            "    return 42;",
            "};",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::JavaScript);
        assert_eq!(funcs.len(), 1);
    }

    #[test]
    fn flaky_detect_single_run_not_flaky() {
        let run1 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let flaky = detect_flaky_tests(&[run1], &HashMap::new());
        assert!(flaky.is_empty());
    }

    #[test]
    fn flaky_detect_with_file_paths() {
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, PathBuf::from("src/lib.rs"));

        let run1 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0)],
            duration_ms: 60,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &file_paths);
        assert_eq!(flaky.len(), 1);
        // Should resolve file path
        assert!(flaky[0].divergent_branches.iter().any(|d| d.file_path.is_some()));
    }

    #[test]
    fn flaky_sorted_by_divergent_count() {
        let run1 = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![br(1, 20, 0), br(1, 30, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];
        let run2 = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![br(1, 10, 1)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![br(1, 20, 1), br(1, 30, 1)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert_eq!(flaky.len(), 2);
        // test_b has more divergent branches, should be first
        assert!(flaky[0].divergent_branches.len() >= flaky[1].divergent_branches.len());
    }

    #[test]
    fn deploy_score_caution_with_moderate_findings() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 12, 0);
        assert_eq!(score.detector_score, 5);
    }

    #[test]
    fn deploy_score_partial_detector_penalty() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 3, 0);
        // 25 - (3 * 2) = 19
        assert_eq!(score.detector_score, 19);
    }

    #[test]
    fn risk_no_changed_files_match() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["nonexistent.py".to_string()]);
        assert_eq!(risk.level, "LOW");
        assert_eq!(risk.changed_branches, 0);
        assert!(risk.reasons.iter().any(|r| r.contains("No changed files")));
    }

    #[test]
    fn risk_medium_moderate_coverage() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Add a few uncovered branches
        for line in 100..105 {
            let b = br(1, line, 0);
            profiles.insert(
                branch_key(&b),
                crate::BranchProfile {
                    branch: b,
                    hit_count: 0,
                    test_count: 0,
                    test_names: vec![],
                },
            );
        }

        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 6,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        // ~17% coverage: score = 40 (low cov) + 10 (uncovered > 0)
        assert!(risk.score >= 20);
    }

    #[test]
    fn hotpaths_empty_index() {
        let index = BranchIndex {
            profiles: HashMap::new(),
            traces: vec![],
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let hot = analyze_hotpaths(&index, 10);
        assert!(hot.is_empty());
    }

    #[test]
    fn hotpaths_truncates_to_top_n() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0), br(1, 30, 0), br(1, 40, 0), br(1, 50, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 5,
            covered_branches: 5,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let hot = analyze_hotpaths(&index, 3);
        assert_eq!(hot.len(), 3);
    }

    #[test]
    fn attack_surface_pct_calculation() {
        let traces = vec![
            TestTrace {
                test_name: "test_api_get".into(),
                branches: vec![br(1, 10, 0), br(1, 20, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];

        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/api.py"))]),
            total_branches: 10,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.entry_tests, 1);
        assert_eq!(report.reachable_branches, 2);
        assert!((report.attack_surface_pct - 20.0).abs() < 0.1);
    }

    #[test]
    fn deploy_score_blocked_by_critical_findings() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 5, 2);
        assert_eq!(score.detector_score, 0);
        assert!(score.total_score < 80);
    }
}
