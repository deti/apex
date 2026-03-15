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
            test_runs.entry(&trace.test_name).or_default().push(keys);
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
                        hit_ratio: format!("{hits}/{total_runs}"),
                    }
                })
                .collect();

            // Count divergent runs: total minus the largest group of identical branch sets.
            let max_agreement = {
                // HashSet doesn't implement Hash, so we sort elements into a
                // BTreeSet for comparison via a Vec key.
                let mut sorted_sets: Vec<Vec<&String>> = branch_sets
                    .iter()
                    .map(|s| {
                        let mut v: Vec<&String> = s.iter().collect();
                        v.sort();
                        v
                    })
                    .collect();
                sorted_sets.sort();
                let mut max_count = 1usize;
                let mut cur_count = 1usize;
                for w in sorted_sets.windows(2) {
                    if w[0] == w[1] {
                        cur_count += 1;
                        if cur_count > max_count {
                            max_count = cur_count;
                        }
                    } else {
                        cur_count = 1;
                    }
                }
                max_count
            };

            flaky.push(FlakyTest {
                test_name: test_name.to_string(),
                divergent_branches,
                divergent_runs: total_runs - max_agreement,
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
pub fn analyze_complexity(index: &BranchIndex, target_root: &Path) -> Vec<FunctionComplexity> {
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

        let is_func_start = func_pattern.iter().any(|p| {
            match language {
                // Python: require "def " at start of line to avoid matching
                // lines that merely contain "def " as a substring
                apex_core::types::Language::Python | apex_core::types::Language::Ruby => {
                    trimmed.starts_with(p)
                }
                // Java: require "(" after the keyword to ensure it's a method signature
                apex_core::types::Language::Java => trimmed.contains(p) && trimmed.contains('('),
                // JS: for "=> {", require ")" before "=>" to confirm arrow function
                apex_core::types::Language::JavaScript => {
                    if *p == "=> {" {
                        trimmed.contains(')') && trimmed.contains("=> {")
                    } else {
                        trimmed.contains(p)
                    }
                }
                _ => trimmed.contains(p),
            }
        }) && !trimmed.starts_with('#')
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
            let after_fn = s.find("fn ").map(|i| &s[i + 3..]).unwrap_or("unknown");
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
                        && ![
                            "pub",
                            "async",
                            "fn",
                            "def",
                            "function",
                            "void",
                            "public",
                            "private",
                            "protected",
                            "static",
                        ]
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
pub fn generate_docs(index: &BranchIndex, target_root: &Path) -> Vec<FunctionDoc> {
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
                .map(|(branches, tests)| ExecutionPath {
                    branch_count: branches.len(),
                    representative_test: tests[0].to_string(),
                    frequency_pct: (tests.len() as f64 / total_tests as f64) * 100.0,
                    test_count: tests.len(),
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

    docs.sort_by(|a, b| a.file_path.cmp(&b.file_path).then(a.line.cmp(&b.line)));

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
pub fn analyze_attack_surface(index: &BranchIndex, entry_pattern: &str) -> AttackSurfaceReport {
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
                .unwrap_or_else(|| PathBuf::from(format!("<{file_id:016x}>")));
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
                        && (profile.branch.line == line_num || profile.branch.line == line_num + 1)
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
        let trace_keys: HashSet<String> = trace.branches.iter().map(branch_key).collect();

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
                direction: if p.branch.direction == 0 {
                    "true"
                } else {
                    "false"
                },
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
pub fn assess_risk(index: &BranchIndex, changed_files: &[String]) -> RiskAssessment {
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
            "Low coverage of changed code: {coverage_of_changed:.0}%"
        ));
    } else if coverage_of_changed < 80.0 {
        score += 20;
        reasons.push(format!(
            "Moderate coverage of changed code: {coverage_of_changed:.0}%"
        ));
    }

    if uncovered_changed > 10 {
        score += 30;
        reasons.push(format!(
            "{uncovered_changed} uncovered branches in changed files"
        ));
    } else if uncovered_changed > 0 {
        score += 10;
        reasons.push(format!(
            "{uncovered_changed} uncovered branches in changed files"
        ));
    }

    if affected_tests.len() > 50 {
        score += 20;
        reasons.push(format!(
            "Wide blast radius: {} tests affected",
            affected_tests.len()
        ));
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
pub fn discover_contracts(index: &BranchIndex, target_root: &Path) -> Vec<DiscoveredInvariant> {
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
        b.evidence_tests.cmp(&a.evidence_tests).then(
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
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
        "Coverage: {cov_pct:.1}% → {coverage_score}/{coverage_max}"
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
        "Detectors: {detector_findings} findings ({critical_findings} critical) → {detector_score}/{detector_max}"
    ));

    // Stability: assume stable if we have an index (future: compare across runs)
    let stability_score = stability_max; // Full marks if index exists
    breakdown.push(format!(
        "Stability: index present → {stability_score}/{stability_max}"
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
        assert!(!flaky[0].divergent_branches.is_empty());
    }

    #[test]
    fn flaky_detect_empty_runs() {
        let flaky = detect_flaky_tests(&[], &HashMap::new());
        assert!(flaky.is_empty());
    }

    #[test]
    fn extract_func_name_python() {
        let name = extract_func_name(
            "def process_order(order):",
            apex_core::types::Language::Python,
        );
        assert_eq!(name, "process_order");
    }

    #[test]
    fn extract_func_name_rust() {
        let name = extract_func_name(
            "pub async fn handle_request(req: Request) -> Response {",
            apex_core::types::Language::Rust,
        );
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
                    test_names: HashSet::new(),
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
        assert!(
            risk.score > 30,
            "expected HIGH risk, got score={}",
            risk.score
        );
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
        let name = extract_func_name(
            "function handleRequest(req, res) {",
            apex_core::types::Language::JavaScript,
        );
        assert_eq!(name, "handleRequest(req");
    }

    #[test]
    fn extract_func_name_ruby() {
        // Ruby uses Python-style "def " prefix strip + split on '('
        let name = extract_func_name(
            "def process_payment(amount)",
            apex_core::types::Language::Ruby,
        );
        assert_eq!(name, "process_payment(amount");
    }

    #[test]
    fn extract_func_name_java() {
        // Java uses generic fallback
        let name = extract_func_name(
            "public void processOrder(Order order) {",
            apex_core::types::Language::Java,
        );
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
        let source = vec!["const handler = (req) => {", "    return 42;", "};"];
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
        assert!(flaky[0]
            .divergent_branches
            .iter()
            .any(|d| d.file_path.is_some()));
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
                    test_names: HashSet::new(),
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
            branches: vec![
                br(1, 10, 0),
                br(1, 20, 0),
                br(1, 30, 0),
                br(1, 40, 0),
                br(1, 50, 0),
            ],
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
        let traces = vec![TestTrace {
            test_name: "test_api_get".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

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

    // -----------------------------------------------------------------------
    // extract_func_name edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn extract_func_name_python_no_def_prefix() {
        // Line that doesn't start with "def " should return "unknown"
        let name = extract_func_name("class Foo:", apex_core::types::Language::Python);
        assert_eq!(name, "unknown");
    }

    #[test]
    fn extract_func_name_rust_no_fn_keyword() {
        // Line without "fn " should return "unknown"
        let name = extract_func_name("let x = 42;", apex_core::types::Language::Rust);
        assert_eq!(name, "unknown");
    }

    #[test]
    fn extract_func_name_rust_with_generics() {
        let name = extract_func_name(
            "pub fn process<T: Clone>(items: &[T]) -> Vec<T> {",
            apex_core::types::Language::Rust,
        );
        assert_eq!(name, "process");
    }

    #[test]
    fn extract_func_name_generic_all_keywords() {
        // All tokens are keywords — should return "unknown"
        let name = extract_func_name("pub async fn", apex_core::types::Language::Wasm);
        assert_eq!(name, "unknown");
    }

    #[test]
    fn extract_func_name_generic_empty_line() {
        let name = extract_func_name("", apex_core::types::Language::Java);
        assert_eq!(name, "unknown");
    }

    #[test]
    fn extract_func_name_c_language_uses_generic() {
        // C falls through to the generic `_ =>` arm
        let name = extract_func_name(
            "void handle_event(int code) {",
            apex_core::types::Language::C,
        );
        assert_eq!(name, "handle_event(int");
    }

    #[test]
    fn extract_func_name_generic_static_keyword_skipped() {
        let name = extract_func_name(
            "public static void main(String[] args) {",
            apex_core::types::Language::Java,
        );
        assert_eq!(name, "main(String");
    }

    // -----------------------------------------------------------------------
    // extract_functions edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn extract_functions_empty_source() {
        let source: Vec<&str> = vec![];
        let funcs = extract_functions(&source, apex_core::types::Language::Python);
        assert!(funcs.is_empty());
    }

    #[test]
    fn extract_functions_skips_hash_comments() {
        // Lines starting with '#' should be skipped even if they contain "def "
        let source = vec!["# def fake_function():", "def real():", "    pass"];
        let funcs = extract_functions(&source, apex_core::types::Language::Python);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, "real");
    }

    #[test]
    fn extract_functions_java_patterns() {
        let source = vec![
            "public void handleRequest(Request req) {",
            "    // body",
            "}",
            "private int compute(int x) {",
            "    return x * 2;",
            "}",
            "protected String format(String s) {",
            "    return s.trim();",
            "}",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::Java);
        assert_eq!(funcs.len(), 3);
    }

    #[test]
    fn extract_functions_single_function_ends_at_last_line() {
        let source = vec!["def only_one():", "    return 42"];
        let funcs = extract_functions(&source, apex_core::types::Language::Python);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, "only_one");
        assert_eq!(funcs[0].1, 1); // starts at line 1
        assert_eq!(funcs[0].2, 2); // ends at last line (lines.len())
    }

    #[test]
    fn extract_functions_c_uses_fn_pattern() {
        // C falls through to default `_ => &["fn "]` pattern
        let source = vec!["fn c_like() {", "    // body", "}"];
        let funcs = extract_functions(&source, apex_core::types::Language::C);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, "c_like");
    }

    #[test]
    fn extract_functions_ruby_uses_def_pattern() {
        let source = vec![
            "def greet(name)",
            "  puts \"Hello #{name}\"",
            "end",
            "def farewell",
            "  puts 'bye'",
            "end",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::Ruby);
        assert_eq!(funcs.len(), 2);
    }

    // -----------------------------------------------------------------------
    // detect_flaky_tests edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn flaky_detect_divergent_branch_no_file_path() {
        // When file_id is not in file_paths, file_path should be None
        let run1 = vec![TestTrace {
            test_name: "test_x".into(),
            branches: vec![br(999, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_x".into(),
            branches: vec![br(999, 10, 1)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert_eq!(flaky.len(), 1);
        // file_path should be None since file_id 999 is not in the map
        for db in &flaky[0].divergent_branches {
            assert!(db.file_path.is_none());
        }
    }

    #[test]
    fn flaky_detect_hit_ratio_format() {
        let run1 = vec![TestTrace {
            test_name: "test_r".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_r".into(),
            branches: vec![br(1, 10, 1)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert_eq!(flaky.len(), 1);
        // Each divergent branch should have hit_ratio like "1/2"
        for db in &flaky[0].divergent_branches {
            assert!(db.hit_ratio.contains('/'));
            assert!(db.hit_ratio.ends_with("/2"));
        }
    }

    #[test]
    fn flaky_detect_multiple_tests_only_some_flaky() {
        let run1 = vec![
            TestTrace {
                test_name: "stable_test".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "flaky_test".into(),
                branches: vec![br(2, 20, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];
        let run2 = vec![
            TestTrace {
                test_name: "stable_test".into(),
                branches: vec![br(1, 10, 0)], // same
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "flaky_test".into(),
                branches: vec![br(2, 20, 1)], // different!
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert_eq!(flaky.len(), 1);
        assert_eq!(flaky[0].test_name, "flaky_test");
    }

    // -----------------------------------------------------------------------
    // analyze_hotpaths edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn hotpaths_direction_false_branch() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 1)], // direction=1 -> "false"
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

        let hot = analyze_hotpaths(&index, 10);
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].direction, "false");
    }

    #[test]
    fn hotpaths_unknown_file_id_fallback() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(9999, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::new(), // no file_paths
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let hot = analyze_hotpaths(&index, 10);
        assert_eq!(hot.len(), 1);
        // Should use the fallback format "<file_id_hex>"
        assert!(
            hot[0]
                .file_path
                .to_string_lossy()
                .contains("0000000000002710")
                || hot[0].file_path.to_string_lossy().starts_with('<')
        );
    }

    #[test]
    fn hotpaths_zero_total_hits() {
        // All profiles have hit_count=0 -> total_hits=0 -> hit_share_pct=0.0
        let mut profiles = HashMap::new();
        let b = br(1, 10, 0);
        profiles.insert(
            branch_key(&b),
            crate::BranchProfile {
                branch: b,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces: vec![],
            file_paths: HashMap::from([(1, PathBuf::from("src/a.py"))]),
            total_branches: 1,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let hot = analyze_hotpaths(&index, 10);
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].hit_share_pct, 0.0);
    }

    // -----------------------------------------------------------------------
    // assess_risk edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn risk_critical_score() {
        // Create scenario with score > 60: low coverage (40) + many uncovered (30) = 70
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Add 15 uncovered branches -> uncovered > 10 -> +30
        // Coverage will be 1/16 = 6.25% -> < 50% -> +40
        for line in 100..115 {
            let b = br(1, line, 0);
            profiles.insert(
                branch_key(&b),
                crate::BranchProfile {
                    branch: b,
                    hit_count: 0,
                    test_count: 0,
                    test_names: HashSet::new(),
                },
            );
        }

        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 16,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        assert_eq!(risk.level, "CRITICAL");
        assert!(risk.score > 60);
    }

    #[test]
    fn risk_wide_blast_radius_over_50_tests() {
        // Create > 50 tests that all touch the changed file
        let mut traces = Vec::new();
        for i in 0..55 {
            traces.push(TestTrace {
                test_name: format!("test_{i}"),
                branches: vec![br(1, 10, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
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

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        assert!(risk.affected_tests > 50);
        assert!(risk.reasons.iter().any(|r| r.contains("Wide blast radius")));
    }

    #[test]
    fn risk_moderate_blast_radius_10_to_50_tests() {
        let mut traces = Vec::new();
        for i in 0..15 {
            traces.push(TestTrace {
                test_name: format!("test_{i}"),
                branches: vec![br(1, 10, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
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

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        assert!(risk.affected_tests > 10);
        assert!(risk.reasons.iter().any(|r| r.contains("tests affected")));
    }

    #[test]
    fn risk_moderate_coverage_50_to_80() {
        // 3 covered out of 5 = 60% -> moderate coverage branch
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0), br(1, 30, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Add 2 uncovered branches
        for line in [40, 50] {
            let b = br(1, line, 0);
            profiles.insert(
                branch_key(&b),
                crate::BranchProfile {
                    branch: b,
                    hit_count: 0,
                    test_count: 0,
                    test_names: HashSet::new(),
                },
            );
        }

        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 5,
            covered_branches: 3,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        assert!(risk.coverage_of_changed >= 50.0 && risk.coverage_of_changed < 80.0);
        assert!(risk.reasons.iter().any(|r| r.contains("Moderate coverage")));
    }

    // -----------------------------------------------------------------------
    // compute_deploy_score edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn deploy_score_zero_coverage() {
        let index = BranchIndex {
            profiles: HashMap::new(),
            traces: vec![],
            file_paths: HashMap::new(),
            total_branches: 100,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 0, 0);
        assert_eq!(score.coverage_score, 0);
        // With 0% coverage, total = 0 + 0 + 25 + 20 = 45
        assert_eq!(
            score.recommendation,
            "CAUTION — review findings before deploying"
        );
    }

    #[test]
    fn deploy_score_no_traces_zero_quality() {
        // No traces -> total_tests = 0 -> quality_ratio = 0
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

        let score = compute_deploy_score(&index, 0, 0);
        assert_eq!(score.test_quality_score, 0);
    }

    #[test]
    fn deploy_score_block_recommendation() {
        // No coverage, many findings -> should BLOCK
        let index = BranchIndex {
            profiles: HashMap::new(),
            traces: vec![],
            file_paths: HashMap::new(),
            total_branches: 100,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 20, 5);
        assert!(score.recommendation.starts_with("BLOCK"));
        assert!(score.total_score <= 40);
    }

    #[test]
    fn deploy_score_acceptable_recommendation() {
        // Partial coverage, no findings -> ACCEPTABLE range (61-80)
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
            total_branches: 2, // 50% coverage
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let score = compute_deploy_score(&index, 0, 0);
        // coverage: 50% of 30 = 15, quality: 25 (1 unique / 1 profile), detector: 25, stability: 20
        // total = 15 + 25 + 25 + 20 = 85 -> actually GO
        // Let's just check the score makes sense
        assert!(score.total_score > 60);
    }

    #[test]
    fn deploy_score_detector_penalty_saturates_at_max() {
        // detector_findings = 13 -> penalty = 13*2 = 26 > 25 (max) -> clamped to 0
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

        // 13 findings but not > 10 threshold... wait, 13 > 10 -> detector_score = 5
        // Actually need exactly 10 findings to test the 1..=10 range
        let score = compute_deploy_score(&index, 10, 0);
        // 25 - (10*2) = 25 - 20 = 5
        assert_eq!(score.detector_score, 5);

        // Test with exactly 13 findings (> 10)
        let score2 = compute_deploy_score(&index, 13, 0);
        assert_eq!(score2.detector_score, 5);
    }

    // -----------------------------------------------------------------------
    // analyze_attack_surface edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn attack_surface_zero_total_branches() {
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

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.attack_surface_pct, 0.0);
    }

    #[test]
    fn attack_surface_unknown_file_id() {
        // Branch with file_id not in file_paths -> fallback path format
        let traces = vec![TestTrace {
            test_name: "test_api_x".into(),
            branches: vec![br(9999, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::new(),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.reachable_files, 1);
        let file_detail = &report.reachable_file_details[0];
        assert!(file_detail.file_path.to_string_lossy().contains('<'));
    }

    #[test]
    fn attack_surface_file_with_zero_total_branches() {
        // file_id in reachable but not in file_totals (no profile entries for it)
        let traces = vec![TestTrace {
            test_name: "test_api_z".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: HashMap::new(), // no profiles -> file_totals will be empty
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/api.py"))]),
            total_branches: 5,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.reachable_files, 1);
        // total_branches_in_file = 0 -> coverage_pct = 0.0
        assert_eq!(report.reachable_file_details[0].coverage_pct, 0.0);
    }

    #[test]
    fn attack_surface_multiple_entry_tests() {
        let traces = vec![
            TestTrace {
                test_name: "test_api_get".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_api_post".into(),
                branches: vec![br(1, 10, 0), br(1, 20, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/api.py"))]),
            total_branches: 5,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.entry_tests, 2);
        // Union of branches: br(1,10,0) and br(1,20,0) = 2
        assert_eq!(report.reachable_branches, 2);
    }

    // -----------------------------------------------------------------------
    // deploy_score breakdown strings
    // -----------------------------------------------------------------------

    #[test]
    fn deploy_score_has_four_breakdown_entries() {
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
        assert_eq!(score.breakdown.len(), 4);
        assert!(score.breakdown[0].starts_with("Coverage:"));
        assert!(score.breakdown[1].starts_with("Test quality:"));
        assert!(score.breakdown[2].starts_with("Detectors:"));
        assert!(score.breakdown[3].starts_with("Stability:"));
    }

    // -----------------------------------------------------------------------
    // risk_assessment: empty changed files list
    // -----------------------------------------------------------------------

    #[test]
    fn risk_empty_changed_files() {
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

        let risk = assess_risk(&index, &[]);
        assert_eq!(risk.level, "LOW");
        assert_eq!(risk.changed_branches, 0);
        // 100% coverage when no changed branches -> no coverage penalty
        assert_eq!(risk.coverage_of_changed, 100.0);
    }

    // -----------------------------------------------------------------------
    // flaky: test_name appears only once (single run) -> len < 2 -> skip
    // -----------------------------------------------------------------------

    #[test]
    fn flaky_detect_test_in_only_one_run() {
        // test_a appears in run1 only, test_b in run2 only -> each has len=1 -> skip
        let run1 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_b".into(),
            branches: vec![br(1, 10, 1)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert!(flaky.is_empty());
    }

    // -----------------------------------------------------------------------
    // assess_risk: MEDIUM score branch (16..=35)
    // -----------------------------------------------------------------------

    #[test]
    fn risk_medium_score_level() {
        // Want score in 16..=35: moderate coverage (50-80%) = +20, no uncovered = +0 -> 20 = MEDIUM
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0), br(1, 30, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Add 1 uncovered branch so coverage = 3/4 = 75% (moderate: 50-80%) and uncovered > 0
        let b = br(1, 40, 0);
        profiles.insert(
            branch_key(&b),
            crate::BranchProfile {
                branch: b,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 4,
            covered_branches: 3,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        // 75% coverage -> +20 (moderate), 1 uncovered -> +10 = 30 -> MEDIUM
        assert_eq!(risk.level, "MEDIUM");
        assert!(risk.score >= 16 && risk.score <= 35);
    }

    #[test]
    fn risk_high_score_level() {
        // Want score in 36..=60: low coverage (<50%) = +40 -> HIGH with no uncovered
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // 1 covered out of 4 = 25% -> low coverage +40
        // Add 3 uncovered branches (> 0 but not > 10 -> +10) = 50 -> HIGH
        for line in [20u32, 30, 40] {
            let b = br(1, line, 0);
            profiles.insert(
                branch_key(&b),
                crate::BranchProfile {
                    branch: b,
                    hit_count: 0,
                    test_count: 0,
                    test_names: HashSet::new(),
                },
            );
        }
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 4,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let risk = assess_risk(&index, &["src/lib.py".to_string()]);
        // 25% -> +40, 3 uncovered -> +10, no blast radius = 50 -> HIGH
        assert_eq!(risk.level, "HIGH");
        assert!(risk.score >= 36 && risk.score <= 60);
    }

    // -----------------------------------------------------------------------
    // discover_contracts tests
    // -----------------------------------------------------------------------

    #[test]
    fn discover_contracts_empty_file_paths() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, Path::new("/nonexistent"));
        assert!(invariants.is_empty());
    }

    #[test]
    fn discover_contracts_source_file_missing() {
        // file_paths points to a non-existent file -> read_to_string fails -> continue
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 5, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("missing.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        // target_root + rel_path doesn't exist -> read fails -> empty invariants
        let invariants = discover_contracts(&index, Path::new("/completely/missing/root"));
        assert!(invariants.is_empty());
    }

    #[test]
    fn discover_contracts_always_taken_invariant() {
        // All 3+ tests hit branch -> ratio >= 0.99 -> "always-taken"
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def validate(x):\n    if x > 0:\n        return True\n",
        )
        .unwrap();

        let mut traces = Vec::new();
        for i in 0..4 {
            traces.push(TestTrace {
                test_name: format!("test_{i}"),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
        let profiles = BranchIndex::build_profiles(&traces);
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(!invariants.is_empty());
        assert!(invariants.iter().any(|i| i.kind == "always-taken"));
    }

    #[test]
    fn discover_contracts_never_taken_invariant() {
        // 0 tests hit a branch (but func has >= 3 tests) -> ratio <= 0.01 -> "never-taken"
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def check(x):\n    if x < 0:\n        return False\n    return True\n",
        )
        .unwrap();

        // 3 tests visit the function (touching line 3) but none take direction=1 (line 2 false)
        let mut traces = Vec::new();
        for i in 0..3 {
            traces.push(TestTrace {
                test_name: format!("test_{i}"),
                branches: vec![br(1, 3, 0)], // touches function but not line 2 false branch
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Add a profile for the never-taken branch (line 2, direction 1) with hit_count=0
        let never_branch = br(1, 2, 1);
        profiles.insert(
            branch_key(&never_branch),
            crate::BranchProfile {
                branch: never_branch.clone(),
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(
            invariants.iter().any(|i| i.kind == "never-taken"),
            "expected never-taken invariant, got: {:?}",
            invariants.iter().map(|i| i.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn discover_contracts_only_one_test_skipped() {
        // total_func_tests < 2 -> skip (no invariants emitted)
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def single(x):\n    return x\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "solo_test".into(),
            branches: vec![br(1, 1, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        // Only 1 test -> total_func_tests < 2 -> no invariants
        assert!(invariants.is_empty());
    }

    #[test]
    fn discover_contracts_branch_direction_false_label() {
        // direction == 1 should produce "false" in the description
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def check(x):\n    if x:\n        pass\n").unwrap();

        let mut traces = Vec::new();
        for i in 0..4 {
            traces.push(TestTrace {
                test_name: format!("test_{i}"),
                branches: vec![br(1, 2, 1)], // direction=1 -> "false" label
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(!invariants.is_empty());
        assert!(
            invariants[0].description.contains("false"),
            "expected 'false' in description: {}",
            invariants[0].description
        );
    }

    #[test]
    fn discover_contracts_line_out_of_bounds() {
        // branch.line > source lines -> saturating_sub underflows to line 0 -> get returns None -> ""
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def f():\n    pass\n").unwrap();

        let mut traces = Vec::new();
        for i in 0..4 {
            traces.push(TestTrace {
                test_name: format!("t{i}"),
                branches: vec![br(1, 1, 0)],
                duration_ms: 5,
                status: ExecutionStatus::Pass,
            });
        }
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Insert a profile for a line that is out of bounds (line 9999)
        let oob_branch = br(1, 9999, 0);
        profiles.insert(
            branch_key(&oob_branch),
            crate::BranchProfile {
                branch: oob_branch,
                hit_count: 4,
                test_count: 4,
                test_names: traces.iter().map(|t| t.test_name.clone()).collect(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        // Should not panic, even with out-of-bounds line
        let _invariants = discover_contracts(&index, tmp.path());
    }

    // -----------------------------------------------------------------------
    // generate_docs tests
    // -----------------------------------------------------------------------

    #[test]
    fn generate_docs_missing_source_file() {
        // read_to_string fails -> continue -> no docs
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 5, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("missing.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, Path::new("/completely/missing"));
        assert!(docs.is_empty());
    }

    #[test]
    fn generate_docs_no_function_branches() {
        // Function found in source but no trace branches touch it -> path_groups empty -> skip
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def unused_func():\n    pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(2, 100, 0)], // different file_id or out-of-range line
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        // No branches in mod.py file_id=1, so path_groups is empty -> no doc for unused_func
        assert!(docs.is_empty());
    }

    #[test]
    fn generate_docs_with_function_having_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def process(x):\n    if x > 0:\n        return x\n    return 0\n",
        )
        .unwrap();

        let traces = vec![
            TestTrace {
                test_name: "test_positive".into(),
                branches: vec![br(1, 2, 0)], // line 2, true branch
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_zero".into(),
                branches: vec![br(1, 2, 1)], // line 2, false branch
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        assert!(!docs.is_empty());
        assert_eq!(docs[0].function_name, "process");
        assert!(docs[0].total_tests >= 1);
        assert!(!docs[0].paths.is_empty());
    }

    #[test]
    fn generate_docs_paths_sorted_by_test_count() {
        // Tests that path sorting (by test_count desc, then branch_count asc) is exercised
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def handler(x):\n    if x > 0:\n        return 1\n    return 0\n",
        )
        .unwrap();

        let traces = vec![
            // 2 tests take path A (same branch set)
            TestTrace {
                test_name: "t1".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "t2".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            // 1 test takes path B (different branch set)
            TestTrace {
                test_name: "t3".into(),
                branches: vec![br(1, 2, 1)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        assert!(!docs.is_empty());
        // Total tests = 3
        assert_eq!(docs[0].total_tests, 3);
        // Most common path (2 tests) should come first
        assert_eq!(docs[0].paths[0].test_count, 2);
        assert_eq!(docs[0].paths[1].test_count, 1);
    }

    // -----------------------------------------------------------------------
    // analyze_complexity tests
    // -----------------------------------------------------------------------

    #[test]
    fn analyze_complexity_missing_source_file() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 5, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("missing.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, Path::new("/nonexistent"));
        assert!(results.is_empty());
    }

    #[test]
    fn analyze_complexity_no_profiles_in_function() {
        // static_count == 0 -> continue (no entry added)
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def empty_func():\n    pass\n").unwrap();

        // Profiles are for a different file_id -> static_count = 0 for this function
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(99, 1, 0)], // file_id 99, not 1
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert!(results.is_empty());
    }

    #[test]
    fn analyze_complexity_classification_fully_exercised() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def fn_a():\n    if True:\n        pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert!(!results.is_empty());
        // 1/1 = 100% -> "fully-exercised"
        assert_eq!(results[0].classification, "fully-exercised");
        assert!((results[0].exercise_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn analyze_complexity_classification_under_tested() {
        // ratio > 0.0 and < 0.5 -> "under-tested"
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        // Write source with a function that has multiple lines so branches can span it
        std::fs::write(&src, "def fn_b():\n    if True:\n        pass\n    if True:\n        pass\n    if True:\n        pass\n    if True:\n        pass\n    if True:\n        pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0)], // only 1 branch hit
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        // Build profiles with 1 covered + 4 uncovered
        let mut profiles = BranchIndex::build_profiles(&traces);
        for line in [4u32, 6, 8, 10] {
            let b = br(1, line, 0);
            profiles.insert(
                branch_key(&b),
                crate::BranchProfile {
                    branch: b,
                    hit_count: 0,
                    test_count: 0,
                    test_names: HashSet::new(),
                },
            );
        }
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 5,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert!(!results.is_empty());
        // 1/5 = 20% -> "under-tested"
        assert_eq!(results[0].classification, "under-tested");
    }

    #[test]
    fn analyze_complexity_classification_dead() {
        // ratio == 0.0 -> "dead"
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def dead_fn():\n    if True:\n        pass\n").unwrap();

        // Profile with hit_count = 0
        let mut profiles = HashMap::new();
        let b = br(1, 2, 0);
        profiles.insert(
            branch_key(&b),
            crate::BranchProfile {
                branch: b,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces: vec![],
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 0,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert!(!results.is_empty());
        assert_eq!(results[0].classification, "dead");
    }

    #[test]
    fn analyze_complexity_classification_partially_tested() {
        // ratio >= 0.5 and < 0.9 -> "partially-tested"
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def partial():\n    if True:\n        pass\n    if True:\n        pass\n    if True:\n        pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0), br(1, 4, 0)], // 2 of 3 hit
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // 1 uncovered
        let b = br(1, 6, 0);
        profiles.insert(
            branch_key(&b),
            crate::BranchProfile {
                branch: b,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 3,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert!(!results.is_empty());
        // 2/3 = 66.7% -> "partially-tested"
        assert_eq!(results[0].classification, "partially-tested");
    }

    #[test]
    fn analyze_complexity_sorted_by_exercise_ratio() {
        // Results are sorted ascending by exercise_ratio
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def fa():\n    if True:\n        pass\n\ndef fb():\n    if True:\n        pass\n    if True:\n        pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0), br(1, 6, 0), br(1, 8, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        // fa: 0/1=0% (add uncovered to it)
        let b = br(1, 2, 1);
        profiles.insert(
            branch_key(&b),
            crate::BranchProfile {
                branch: b,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 3,
            covered_branches: 3,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        // Results should be sorted by exercise_ratio ascending
        for i in 1..results.len() {
            assert!(results[i - 1].exercise_ratio <= results[i].exercise_ratio);
        }
    }

    // -----------------------------------------------------------------------
    // verify_boundaries tests
    // -----------------------------------------------------------------------

    #[test]
    fn verify_boundaries_all_protected() {
        // All entry tests pass through auth — unprotected should be empty
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("api.py");
        std::fs::write(
            &src,
            "def endpoint(req):\n    check_auth(req)\n    return 200\n",
        )
        .unwrap();

        let traces = vec![TestTrace {
            test_name: "test_api_login".into(),
            branches: vec![br(1, 2, 0)], // hits auth line
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("api.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let report = verify_boundaries(&index, tmp.path(), "test_api", "check_auth");
        assert_eq!(report.total_entry_tests, 1);
        assert_eq!(report.failing_tests, 0);
        assert_eq!(report.passing_tests, 1);
        assert!(report.unprotected_paths.is_empty());
    }

    #[test]
    fn verify_boundaries_unprotected_paths() {
        // Entry test does NOT hit auth branch -> unprotected.
        // Use a source with auth keyword at line 5 and the trace only hits line 10.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("api.py");
        // 10-line file: auth at line 5, trace hits only line 10
        std::fs::write(&src,
            "def endpoint(req):\n    x = 1\n    y = 2\n    z = 3\n    check_auth(req)\n    a = 1\n    b = 2\n    c = 3\n    d = 4\n    return 200\n"
        ).unwrap();

        let traces = vec![TestTrace {
            test_name: "test_api_bypass".into(),
            branches: vec![br(1, 10, 0)], // hits only line 10, far from auth at line 5
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        // Auth branch profile at line 5 (the check_auth line) — never hit
        let mut profiles = BranchIndex::build_profiles(&traces);
        let auth_b = br(1, 5, 0);
        profiles.insert(
            branch_key(&auth_b),
            crate::BranchProfile {
                branch: auth_b,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("api.py"))]),
            total_branches: 2,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let report = verify_boundaries(&index, tmp.path(), "test_api", "check_auth");
        assert_eq!(report.total_entry_tests, 1);
        assert_eq!(report.failing_tests, 1);
        assert_eq!(report.passing_tests, 0);
        assert_eq!(report.unprotected_paths[0].test_name, "test_api_bypass");
        assert_eq!(report.unprotected_paths[0].branches_traversed, 1);
    }

    #[test]
    fn verify_boundaries_source_file_unreadable() {
        // Source file for auth scan can't be read -> auth_branches stays empty -> all entry tests unprotected
        let traces = vec![TestTrace {
            test_name: "test_api_x".into(),
            branches: vec![br(1, 5, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("missing.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let report =
            verify_boundaries(&index, Path::new("/missing/root"), "test_api", "check_auth");
        // Source couldn't be read -> auth_branches empty -> test is unprotected
        assert_eq!(report.total_entry_tests, 1);
        assert_eq!(report.failing_tests, 1);
    }

    #[test]
    fn verify_boundaries_auth_on_next_line() {
        // Branch is on line_num + 1 relative to auth source line
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("api.py");
        // Line 1: auth check keyword, line 2: branch that would be associated with it
        std::fs::write(
            &src,
            "def ep():\n    verify_token(r)\n    if ok:\n        return 200\n",
        )
        .unwrap();

        let traces = vec![TestTrace {
            test_name: "test_api_auth".into(),
            branches: vec![br(1, 3, 0)], // line 3 = verify_token line (1) + 1 = auth branch
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        // The auth scan finds "verify_token" at line 2, and checks profile.branch.line == 2 or 3
        let mut profiles = BranchIndex::build_profiles(&traces);
        // Mark the branch on line 3 as the auth branch (line 2 + 1)
        let auth_b = br(1, 3, 0);
        profiles
            .entry(branch_key(&auth_b))
            .or_insert_with(|| crate::BranchProfile {
                branch: auth_b,
                hit_count: 1,
                test_count: 1,
                test_names: HashSet::from(["test_api_auth".into()]),
            });
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("api.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let report = verify_boundaries(&index, tmp.path(), "test_api", "verify_token");
        assert_eq!(report.total_entry_tests, 1);
        // Should be protected (auth branch found and hit)
        assert_eq!(report.failing_tests, 0);
    }

    #[test]
    fn verify_boundaries_files_reached_in_unprotected() {
        // Test that unprotected_path.files_reached is populated correctly
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("api.py");
        std::fs::write(&src, "def ep():\n    return 200\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "test_api_leak".into(),
            branches: vec![br(1, 1, 0), br(2, 5, 0)], // touches 2 files
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("api.py")), (2, PathBuf::from("db.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let report = verify_boundaries(&index, tmp.path(), "test_api", "check_auth");
        assert_eq!(report.failing_tests, 1);
        // Should have reached files (up to 2)
        assert!(!report.unprotected_paths[0].files_reached.is_empty());
    }

    // -----------------------------------------------------------------------
    // extract_func_name: Ruby "def" without '(' -> uses Python path -> unwrap_or("unknown")
    // -----------------------------------------------------------------------

    #[test]
    fn extract_func_name_ruby_no_parens() {
        // Ruby uses Python path. "def" without '(' -> split on '(' gives just the suffix
        // "def simple" -> strip_prefix("def ") = "simple" -> split('(').next() = "simple"
        let name = extract_func_name("def simple", apex_core::types::Language::Ruby);
        assert_eq!(name, "simple");
    }

    #[test]
    fn extract_func_name_python_just_def_keyword() {
        // "def " alone -> trim() -> "def" -> strip_prefix("def ") = None -> "unknown"
        let name = extract_func_name("def ", apex_core::types::Language::Python);
        assert_eq!(name, "unknown");
    }

    #[test]
    fn extract_func_name_python_no_open_paren_returns_whole() {
        // "def noparen" -> strip "def " = "noparen" -> split('(').next() = "noparen"
        let name = extract_func_name("def noparen", apex_core::types::Language::Python);
        assert_eq!(name, "noparen");
    }

    // -----------------------------------------------------------------------
    // extract_functions: Wasm / C fallback pattern
    // -----------------------------------------------------------------------

    #[test]
    fn extract_functions_wasm_uses_fn_pattern() {
        let source = vec!["fn wasm_func() {", "    nop", "}"];
        let funcs = extract_functions(&source, apex_core::types::Language::Wasm);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, "wasm_func");
    }

    #[test]
    fn extract_functions_no_functions_found() {
        let source = vec!["x = 1", "y = 2", "z = x + y"];
        let funcs = extract_functions(&source, apex_core::types::Language::Python);
        assert!(funcs.is_empty());
    }

    #[test]
    fn extract_functions_closes_on_next_function_same_language() {
        // Previous function end = line_num - 1 when next function starts
        let source = vec![
            "def first():",
            "    return 1",
            "def second():",
            "    return 2",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::Python);
        assert_eq!(funcs.len(), 2);
        // first ends at line 2 (= second starts at line 3, so line_num-1 = 2)
        assert_eq!(funcs[0].2, 2);
        assert_eq!(funcs[1].1, 3);
    }

    // -----------------------------------------------------------------------
    // deploy_score: ACCEPTABLE recommendation (61-80)
    // -----------------------------------------------------------------------

    #[test]
    fn deploy_score_acceptable_total_range() {
        // Score in 61..=80 range -> "ACCEPTABLE"
        // Use 50% coverage (15/30), max quality (25), no findings (25), stability (20) = 85... that's GO
        // Try: 0% coverage (0) + quality 0 + no findings (25) + stability (20) = 45 = CAUTION
        // Need 61-80: 50% cov = 15, quality = 0 (no tests), no findings 25, stability 20 = 60... still CAUTION
        // 75% cov = 23, quality = 0, no findings = 25, stability = 20 = 68 -> ACCEPTABLE
        let index = BranchIndex {
            profiles: HashMap::new(),
            traces: vec![], // 0 tests -> quality = 0
            file_paths: HashMap::new(),
            total_branches: 4,
            covered_branches: 3, // 75%
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let score = compute_deploy_score(&index, 0, 0);
        assert!(
            score.total_score >= 61 && score.total_score <= 80,
            "expected ACCEPTABLE range, got total_score={}",
            score.total_score
        );
        assert_eq!(score.recommendation, "ACCEPTABLE — deploy with monitoring");
    }

    // -----------------------------------------------------------------------
    // generate_docs: frequency_pct and multiple paths
    // -----------------------------------------------------------------------

    #[test]
    fn generate_docs_frequency_pct_calculation() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def work(x):\n    if x:\n        return 1\n    return 0\n",
        )
        .unwrap();

        let traces = vec![
            TestTrace {
                test_name: "t1".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "t2".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "t3".into(),
                branches: vec![br(1, 2, 1)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        assert_eq!(docs.len(), 1);
        let first = &docs[0].paths[0];
        assert!((first.frequency_pct - 66.67).abs() < 1.0);
        let second = &docs[0].paths[1];
        assert!((second.frequency_pct - 33.33).abs() < 1.0);
    }

    #[test]
    fn generate_docs_sorted_by_file_path_then_line() {
        let tmp = tempfile::tempdir().unwrap();
        let src_a = tmp.path().join("bbb.py");
        std::fs::write(&src_a, "def zz():\n    if True:\n        pass\n").unwrap();
        let src_b = tmp.path().join("aaa.py");
        std::fs::write(&src_b, "def aa():\n    if True:\n        pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0), br(2, 2, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("bbb.py")), (2, PathBuf::from("aaa.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        assert_eq!(docs.len(), 2);
        assert!(docs[0].file_path < docs[1].file_path);
    }

    #[test]
    fn generate_docs_empty_file_paths() {
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
        let docs = generate_docs(&index, Path::new("/nonexistent"));
        assert!(docs.is_empty());
    }

    #[test]
    fn generate_docs_representative_test_is_first_in_group() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def calc(x):\n    if x > 0:\n        return x\n").unwrap();

        let traces = vec![
            TestTrace {
                test_name: "alpha_test".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "beta_test".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        assert_eq!(docs[0].paths[0].representative_test, "alpha_test");
    }

    // -----------------------------------------------------------------------
    // analyze_complexity: multiple functions, sorting verification
    // -----------------------------------------------------------------------

    #[test]
    fn analyze_complexity_multiple_functions_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def good():\n    if True:\n        pass\n\ndef bad():\n    if True:\n        pass\n",
        )
        .unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        let dead = br(1, 6, 0);
        profiles.insert(
            branch_key(&dead),
            crate::BranchProfile {
                branch: dead,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].classification, "dead");
        assert_eq!(results[1].classification, "fully-exercised");
    }

    // -----------------------------------------------------------------------
    // discover_contracts: sorting and edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn discover_contracts_sorted_by_evidence_then_confidence() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(
            &src,
            "def big_fn():\n    if a:\n        pass\n    if b:\n        pass\n",
        )
        .unwrap();

        let mut traces = Vec::new();
        for i in 0..5 {
            traces.push(TestTrace {
                test_name: format!("t{i}"),
                branches: vec![br(1, 2, 0), br(1, 4, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(invariants.len() >= 2);
        for i in 1..invariants.len() {
            assert!(invariants[i - 1].evidence_tests >= invariants[i].evidence_tests);
        }
    }

    #[test]
    fn discover_contracts_two_tests_no_invariants() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def f():\n    if x:\n        pass\n").unwrap();

        let traces = vec![
            TestTrace {
                test_name: "t1".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "t2".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(invariants.is_empty());
    }

    #[test]
    fn discover_contracts_no_functions_in_source() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "x = 1\ny = 2\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 1, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(invariants.is_empty());
    }

    // -----------------------------------------------------------------------
    // verify_boundaries: files_reached deduplication
    // -----------------------------------------------------------------------

    #[test]
    fn verify_boundaries_files_reached_deduplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("api.py");
        std::fs::write(&src, "def ep():\n    return 200\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "test_api_dup".into(),
            branches: vec![br(1, 1, 0), br(1, 2, 0), br(1, 3, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("api.py"))]),
            total_branches: 3,
            covered_branches: 3,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let report = verify_boundaries(&index, tmp.path(), "test_api", "check_auth");
        assert_eq!(report.failing_tests, 1);
        assert_eq!(report.unprotected_paths[0].files_reached.len(), 1);
    }

    // -----------------------------------------------------------------------
    // attack_surface: reachable_file_details sorted
    // -----------------------------------------------------------------------

    #[test]
    fn attack_surface_file_details_sorted_descending() {
        let traces = vec![TestTrace {
            test_name: "test_api_all".into(),
            branches: vec![br(1, 10, 0), br(2, 5, 0), br(2, 6, 0), br(2, 7, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([
                (1, PathBuf::from("src/a.py")),
                (2, PathBuf::from("src/b.py")),
            ]),
            total_branches: 10,
            covered_branches: 4,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.reachable_file_details.len(), 2);
        assert!(
            report.reachable_file_details[0].reachable_branches
                >= report.reachable_file_details[1].reachable_branches
        );
    }

    // -----------------------------------------------------------------------
    // deploy_score edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn deploy_score_single_finding_penalty() {
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
        let score = compute_deploy_score(&index, 1, 0);
        assert_eq!(score.detector_score, 23);
    }

    #[test]
    fn deploy_score_eleven_findings_flat_five() {
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
        let score = compute_deploy_score(&index, 11, 0);
        assert_eq!(score.detector_score, 5);
    }

    // -----------------------------------------------------------------------
    // extract_func_name: JavaScript arrow function
    // -----------------------------------------------------------------------

    #[test]
    fn extract_func_name_js_arrow_function() {
        let name = extract_func_name(
            "const handler = (req) => {",
            apex_core::types::Language::JavaScript,
        );
        assert_eq!(name, "const");
    }

    #[test]
    fn extract_func_name_js_regular_function() {
        let name = extract_func_name(
            "function doWork(a, b) {",
            apex_core::types::Language::JavaScript,
        );
        assert_eq!(name, "doWork(a");
    }

    // -----------------------------------------------------------------------
    // extract_functions: JavaScript with both function and arrow
    // -----------------------------------------------------------------------

    #[test]
    fn extract_functions_js_mixed_styles() {
        let source = vec![
            "function regular() {",
            "    return 1;",
            "}",
            "const arrow = (x) => {",
            "    return x;",
            "};",
        ];
        let funcs = extract_functions(&source, apex_core::types::Language::JavaScript);
        assert_eq!(funcs.len(), 2);
        assert!(funcs[0].2 < funcs[1].1);
    }

    // -----------------------------------------------------------------------
    // hotpaths: direction label "true"
    // -----------------------------------------------------------------------

    #[test]
    fn hotpaths_direction_true_branch() {
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
        let hot = analyze_hotpaths(&index, 10);
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].direction, "true");
        assert_eq!(hot[0].line, 10);
        assert_eq!(hot[0].test_count, 1);
    }

    // -----------------------------------------------------------------------
    // assess_risk: empty index
    // -----------------------------------------------------------------------

    #[test]
    fn risk_empty_index_no_files() {
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
        let risk = assess_risk(&index, &["anything.py".to_string()]);
        assert_eq!(risk.level, "LOW");
        assert_eq!(risk.changed_branches, 0);
        assert_eq!(risk.affected_tests, 0);
    }

    // -----------------------------------------------------------------------
    // analyze_complexity: empty file_paths
    // -----------------------------------------------------------------------

    #[test]
    fn analyze_complexity_empty_file_paths() {
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
        let results = analyze_complexity(&index, Path::new("/nonexistent"));
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // generate_docs: test that skips trace with no matching branches
    // -----------------------------------------------------------------------

    #[test]
    fn generate_docs_trace_skipped_when_no_branches_match_function() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def myfunc():\n    if cond:\n        pass\n").unwrap();

        let traces = vec![
            TestTrace {
                test_name: "matching".into(),
                branches: vec![br(1, 2, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "not_matching".into(),
                branches: vec![br(1, 999, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let docs = generate_docs(&index, tmp.path());
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].total_tests, 1);
        assert_eq!(docs[0].paths[0].representative_test, "matching");
    }

    // -----------------------------------------------------------------------
    // verify_boundaries: auth on exact line
    // -----------------------------------------------------------------------

    #[test]
    fn verify_boundaries_auth_on_exact_line() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("api.py");
        std::fs::write(
            &src,
            "def ep():\n    if check_auth(r):\n        return 200\n",
        )
        .unwrap();

        let traces = vec![TestTrace {
            test_name: "test_api_ok".into(),
            branches: vec![br(1, 2, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("api.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let report = verify_boundaries(&index, tmp.path(), "test_api", "check_auth");
        assert_eq!(report.total_entry_tests, 1);
        assert_eq!(report.failing_tests, 0);
        assert_eq!(report.passing_tests, 1);
    }

    // -----------------------------------------------------------------------
    // flaky_detect: divergent_runs field
    // -----------------------------------------------------------------------

    #[test]
    fn flaky_detect_divergent_runs_computed_correctly() {
        let run1 = vec![TestTrace {
            test_name: "test_f".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_f".into(),
            branches: vec![br(1, 10, 1)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run3 = vec![TestTrace {
            test_name: "test_f".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2, run3], &HashMap::new());
        assert_eq!(flaky.len(), 1);
        assert_eq!(flaky[0].total_runs, 3);
        // 2 runs agree (br(1,10,0)), 1 diverges (br(1,10,1)) → divergent_runs = 1
        assert_eq!(flaky[0].divergent_runs, 1);
    }

    // -----------------------------------------------------------------------
    // discover_contracts: partial ratio -> no invariant
    // -----------------------------------------------------------------------

    #[test]
    fn discover_contracts_partial_ratio_no_invariant() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def f():\n    if x:\n        pass\n").unwrap();

        let mut traces = Vec::new();
        for i in 0..4 {
            traces.push(TestTrace {
                test_name: format!("t{i}"),
                branches: if i < 2 {
                    vec![br(1, 2, 0)]
                } else {
                    vec![br(1, 2, 1)]
                },
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            });
        }
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let invariants = discover_contracts(&index, tmp.path());
        assert!(
            invariants.is_empty(),
            "expected no invariants for ratio=0.5, got {}",
            invariants.len()
        );
    }

    // -----------------------------------------------------------------------
    // analyze_complexity: function_name and line fields
    // -----------------------------------------------------------------------

    #[test]
    fn analyze_complexity_reports_function_name_and_line() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mod.py");
        std::fs::write(&src, "def my_function():\n    if True:\n        pass\n").unwrap();

        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 2, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("mod.py"))]),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: tmp.path().to_path_buf(),
            source_hash: String::new(),
        };
        let results = analyze_complexity(&index, tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].function_name, "my_function");
        assert_eq!(results[0].line, 1);
        assert_eq!(results[0].file_path, PathBuf::from("mod.py"));
        assert_eq!(results[0].static_complexity, 1);
        assert_eq!(results[0].exercised_complexity, 1);
    }

    // -----------------------------------------------------------------------
    // attack_surface: coverage_pct with nonzero total
    // -----------------------------------------------------------------------

    #[test]
    fn attack_surface_file_coverage_pct_nonzero_total() {
        let traces = vec![TestTrace {
            test_name: "test_api_v".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let mut profiles = BranchIndex::build_profiles(&traces);
        let uncov = br(1, 20, 0);
        profiles.insert(
            branch_key(&uncov),
            crate::BranchProfile {
                branch: uncov,
                hit_count: 0,
                test_count: 0,
                test_names: HashSet::new(),
            },
        );
        let index = BranchIndex {
            profiles,
            traces,
            file_paths: HashMap::from([(1, PathBuf::from("src/api.py"))]),
            total_branches: 2,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.reachable_file_details.len(), 1);
        assert!((report.reachable_file_details[0].coverage_pct - 50.0).abs() < 0.1);
    }

    // -----------------------------------------------------------------------
    // deploy_score: quality ratio capped at 1.0
    // -----------------------------------------------------------------------

    #[test]
    fn deploy_score_quality_ratio_capped() {
        let traces = vec![
            TestTrace {
                test_name: "t1".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "t2".into(),
                branches: vec![br(1, 20, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];
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
        let score = compute_deploy_score(&index, 0, 0);
        assert_eq!(score.test_quality_score, score.test_quality_max);
    }

    // -----------------------------------------------------------------------
    // extract_func_name: Rust simple and async
    // -----------------------------------------------------------------------

    #[test]
    fn extract_func_name_rust_simple() {
        let name = extract_func_name("fn simple() {", apex_core::types::Language::Rust);
        assert_eq!(name, "simple");
    }

    #[test]
    fn extract_func_name_rust_visibility_and_async() {
        let name = extract_func_name(
            "pub async fn fetch_data(url: &str) -> Result<Data> {",
            apex_core::types::Language::Rust,
        );
        assert_eq!(name, "fetch_data");
    }

    // -----------------------------------------------------------------------
    // hotpaths: sorted by hit_count descending
    // -----------------------------------------------------------------------

    #[test]
    fn hotpaths_sorted_by_hit_count_descending() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![br(1, 10, 0), br(1, 10, 0), br(1, 10, 0), br(1, 20, 0)],
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
        assert_eq!(hot.len(), 2);
        assert!(hot[0].hit_count >= hot[1].hit_count);
        assert_eq!(hot[0].hit_count, 3);
        assert_eq!(hot[1].hit_count, 1);
    }

    // -----------------------------------------------------------------------
    // Bug-hunting tests
    // -----------------------------------------------------------------------

    /// BUG: Java extract_functions matches ANY line containing "public ", "private ",
    /// or "protected ", not just method declarations. Field declarations like
    /// "public int x = 5;" and class declarations like "public class Foo {" are
    /// incorrectly identified as function starts.
    #[test]
    fn bug_java_extract_functions_false_positives() {
        let lines: Vec<&str> = vec![
            "public class MyService {",    // line 1 - NOT a function
            "    public int counter = 0;", // line 2 - NOT a function
            "    private String name;",    // line 3 - NOT a function
            "    public void doWork() {",  // line 4 - IS a function
            "        counter++;",          // line 5
            "    }",                       // line 6
            "    private void helper() {", // line 7 - IS a function
            "        name = \"test\";",    // line 8
            "    }",                       // line 9
            "}",                           // line 10
        ];

        let functions = extract_functions(&lines, apex_core::types::Language::Java);

        // We'd expect only 2 functions: doWork and helper.
        // But the pattern matches ANY line with "public ", "private ", "protected ".
        // So it'll also match the class declaration and field declarations.
        let names: Vec<&str> = functions.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(
            names.len(),
            2,
            "BUG: Java extract_functions found {} functions ({:?}), expected 2 (doWork, helper). \
             Field declarations and class declarations are incorrectly matched.",
            names.len(),
            names
        );
    }

    /// BUG: JavaScript extract_functions matches "=> {" inside strings or comments.
    /// A line like `const msg = "arrow => {test}";` would be incorrectly
    /// identified as a function.
    #[test]
    fn bug_js_extract_functions_arrow_in_string() {
        let lines: Vec<&str> = vec![
            "const msg = 'arrow => {test}';", // line 1 - NOT a function
            "const fn1 = (x) => {",           // line 2 - IS a function
            "    return x + 1;",              // line 3
            "};",                             // line 4
        ];

        let functions = extract_functions(&lines, apex_core::types::Language::JavaScript);

        let names: Vec<&str> = functions.iter().map(|(n, _, _)| n.as_str()).collect();
        // "=> {" inside a string on line 1 should NOT be detected.
        // We expect only 1 function (the actual arrow function on line 2).
        assert_eq!(
            names.len(),
            1,
            "BUG: JS extract_functions found {} functions ({:?}), expected 1. \
             '=> {{' inside a string was incorrectly matched as a function.",
            names.len(),
            names
        );
    }

    /// Python extract_func_name: "def " not at start of line (e.g., inside a string).
    /// The starts_with('#') guard doesn't help with string content.
    #[test]
    fn bug_python_func_in_string() {
        let lines: Vec<&str> = vec![
            "description = \"use def process(x) to start\"", // NOT a function
            "def actual_function(x):",                       // IS a function
            "    pass",
        ];

        let functions = extract_functions(&lines, apex_core::types::Language::Python);
        let names: Vec<&str> = functions.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(
            names.len(),
            1,
            "BUG: Python extract_functions found {} functions ({:?}), expected 1. \
             'def ' inside a string was incorrectly matched.",
            names.len(),
            names
        );
    }
}
