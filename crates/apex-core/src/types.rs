use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeedId(pub Uuid);

impl SeedId {
    pub fn new() -> Self {
        SeedId(Uuid::new_v4())
    }
}

impl Default for SeedId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotId(pub Uuid);

impl SnapshotId {
    pub fn new() -> Self {
        SnapshotId(Uuid::new_v4())
    }
}

impl Default for SnapshotId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// BranchId
// ---------------------------------------------------------------------------

/// Uniquely identifies one direction of a conditional branch in source code.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct BranchId {
    /// FNV-1a hash of the path relative to repo root.
    pub file_id: u64,
    pub line: u32,
    pub col: u16,
    /// 0 = taken / true branch, 1 = not-taken / false branch.
    pub direction: u8,
    /// Disambiguates macro-expanded duplicates on the same line.
    pub discriminator: u16,
    /// For MC/DC: index of the individual condition within a compound decision.
    /// `None` for plain branch coverage; `Some(i)` for condition `i`.
    #[serde(default)]
    pub condition_index: Option<u8>,
}

impl BranchId {
    pub fn new(file_id: u64, line: u32, col: u16, direction: u8) -> Self {
        BranchId {
            file_id,
            line,
            col,
            direction,
            discriminator: 0,
            condition_index: None,
        }
    }

    /// Create a BranchId for MC/DC condition-level tracking.
    pub fn new_mcdc(
        file_id: u64,
        line: u32,
        col: u16,
        direction: u8,
        condition_index: Option<u8>,
    ) -> Self {
        BranchId {
            file_id,
            line,
            col,
            direction,
            discriminator: 0,
            condition_index,
        }
    }
}

// ---------------------------------------------------------------------------
// CoverageLevel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoverageLevel {
    Statement,
    Branch,
    Mcdc,
}

impl std::fmt::Display for CoverageLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverageLevel::Statement => write!(f, "statement"),
            CoverageLevel::Branch => write!(f, "branch"),
            CoverageLevel::Mcdc => write!(f, "mcdc"),
        }
    }
}

// ---------------------------------------------------------------------------
// BranchState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BranchState {
    Uncovered,
    Covered {
        hit_count: u32,
        first_seed_id: SeedId,
    },
    /// Proven unreachable by the SMT solver.
    Unreachable,
    /// Excluded by user configuration.
    Suppressed,
}

// ---------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Python,
    JavaScript,
    Java,
    C,
    Rust,
    Wasm,
    Ruby,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::Java => "java",
            Language::C => "c",
            Language::Rust => "rust",
            Language::Wasm => "wasm",
            Language::Ruby => "ruby",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for Language {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "python" | "py" => Ok(Language::Python),
            "javascript" | "js" | "node" => Ok(Language::JavaScript),
            "java" => Ok(Language::Java),
            "c" => Ok(Language::C),
            "rust" | "rs" => Ok(Language::Rust),
            "wasm" => Ok(Language::Wasm),
            "ruby" | "rb" => Ok(Language::Ruby),
            other => Err(format!("unknown language: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Target
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub root: PathBuf,
    pub language: Language,
    /// Entry-point command for the test suite (e.g. `["pytest", "-q"]`).
    pub test_command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentedTarget {
    pub target: Target,
    /// All branches discovered (executed + missing).
    pub branch_ids: Vec<BranchId>,
    /// Branches that were hit during the instrumentation run.
    pub executed_branch_ids: Vec<BranchId>,
    /// Maps FNV-1a file_id → repo-relative path (for human-readable reports).
    pub file_paths: std::collections::HashMap<u64, PathBuf>,
    pub work_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Seeds / Inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeedOrigin {
    Corpus,
    Fuzzer,
    Concolic,
    Symbolic,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSeed {
    pub id: SeedId,
    pub data: Bytes,
    pub origin: SeedOrigin,
    pub target_branches: Vec<BranchId>,
    pub priority: f32,
}

impl InputSeed {
    pub fn new(data: impl Into<Bytes>, origin: SeedOrigin) -> Self {
        InputSeed {
            id: SeedId::new(),
            data: data.into(),
            origin,
            target_branches: Vec::new(),
            priority: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Execution results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pass,
    Fail,
    Timeout,
    Crash,
    OomKill,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub lines_hit: Vec<(u64, u32)>, // (file_id, line)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub seed_id: SeedId,
    pub status: ExecutionStatus,
    /// Branches newly covered by this run (delta vs oracle before the run).
    pub new_branches: Vec<BranchId>,
    pub trace: Option<ExecutionTrace>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

// ---------------------------------------------------------------------------
// Agent / synthesis types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCandidate {
    pub id: Uuid,
    pub code: String,
    pub target_branches: Vec<BranchId>,
    pub reasoning: String,
    pub language: Language,
}

impl TestCandidate {
    pub fn new(code: String, language: Language) -> Self {
        TestCandidate {
            id: Uuid::new_v4(),
            code,
            target_branches: Vec::new(),
            reasoning: String::new(),
            language,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizedTest {
    pub path: PathBuf,
    pub content: String,
    pub covers_branches: Vec<BranchId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageGapReport {
    pub total_branches: usize,
    pub covered_branches: usize,
    pub uncovered: Vec<UncoveredBranch>,
}

impl CoverageGapReport {
    pub fn coverage_percent(&self) -> f64 {
        if self.total_branches == 0 {
            return 100.0;
        }
        (self.covered_branches as f64 / self.total_branches as f64) * 100.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UncoveredBranch {
    pub branch: BranchId,
    pub file_path: PathBuf,
    pub source_line: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceContext {
    pub file_path: PathBuf,
    pub lines: Vec<String>,
    pub start_line: u32,
}

// ---------------------------------------------------------------------------
// Exploration context (passed to Strategy impls)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationContext {
    pub target: Target,
    pub uncovered_branches: Vec<BranchId>,
    pub iteration: u64,
}

// ---------------------------------------------------------------------------
// Bug tracking
// ---------------------------------------------------------------------------

/// Classification of a discovered bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BugClass {
    Crash,
    AssertionFailure,
    Timeout,
    OomKill,
}

impl std::fmt::Display for BugClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BugClass::Crash => write!(f, "crash"),
            BugClass::AssertionFailure => write!(f, "assertion_failure"),
            BugClass::Timeout => write!(f, "timeout"),
            BugClass::OomKill => write!(f, "oom_kill"),
        }
    }
}

impl BugClass {
    /// Classify an `ExecutionStatus` into a bug class, if it represents a bug.
    /// `Pass` is not a bug; `Fail` is classified as `AssertionFailure`.
    pub fn from_status(status: ExecutionStatus) -> Option<Self> {
        match status {
            ExecutionStatus::Pass => None,
            ExecutionStatus::Fail => Some(BugClass::AssertionFailure),
            ExecutionStatus::Timeout => Some(BugClass::Timeout),
            ExecutionStatus::Crash => Some(BugClass::Crash),
            ExecutionStatus::OomKill => Some(BugClass::OomKill),
        }
    }
}

/// A single bug discovered during exploration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugReport {
    pub id: Uuid,
    pub class: BugClass,
    /// The seed that triggered this bug.
    pub seed_id: SeedId,
    /// Stderr or crash output snippet.
    pub message: String,
    /// File + line where the bug manifested (if known).
    pub location: Option<String>,
    /// Branches that were active when the bug was found.
    pub triggering_branches: Vec<BranchId>,
    /// Wall-clock time when discovered.
    pub discovered_at_iteration: u64,
}

impl BugReport {
    pub fn new(class: BugClass, seed_id: SeedId, message: String) -> Self {
        BugReport {
            id: Uuid::new_v4(),
            class,
            seed_id,
            message,
            location: None,
            triggering_branches: Vec::new(),
            discovered_at_iteration: 0,
        }
    }

    /// Deduplication key: (class, location or first 128 chars of message).
    pub fn dedup_key(&self) -> String {
        let loc = self
            .location
            .as_deref()
            .unwrap_or_else(|| &self.message[..self.message.len().min(128)]);
        format!("{}:{}", self.class, loc)
    }
}

/// Aggregated summary of all bugs found during a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BugSummary {
    pub total: usize,
    pub by_class: std::collections::HashMap<String, usize>,
    pub reports: Vec<BugReport>,
}

impl BugSummary {
    pub fn new(reports: Vec<BugReport>) -> Self {
        let total = reports.len();
        let mut by_class: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for r in &reports {
            *by_class.entry(r.class.to_string()).or_default() += 1;
        }
        BugSummary {
            total,
            by_class,
            reports,
        }
    }
}

// ---------------------------------------------------------------------------
// Path constraints (for symbolic/concolic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConstraint {
    pub branch: BranchId,
    /// SMTLIB2 assertion string.
    pub smtlib2: String,
    pub direction_taken: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_id_equality() {
        let a = BranchId::new(42, 10, 0, 0);
        let b = BranchId::new(42, 10, 0, 0);
        let c = BranchId::new(42, 10, 0, 1); // different direction
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn branch_id_hash_consistent() {
        use std::collections::HashSet;
        let a = BranchId::new(1, 5, 0, 0);
        let b = BranchId::new(1, 5, 0, 0);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn language_parse_roundtrip() {
        for lang in ["python", "js", "java", "c", "rust", "wasm", "ruby"] {
            let parsed: Language = lang.parse().unwrap();
            let display = parsed.to_string();
            let reparsed: Language = display.parse().unwrap();
            assert_eq!(parsed, reparsed);
        }
    }

    #[test]
    fn language_parse_aliases() {
        assert_eq!("py".parse::<Language>().unwrap(), Language::Python);
        assert_eq!("node".parse::<Language>().unwrap(), Language::JavaScript);
        assert_eq!("rs".parse::<Language>().unwrap(), Language::Rust);
        assert_eq!("rb".parse::<Language>().unwrap(), Language::Ruby);
        assert_eq!("ruby".parse::<Language>().unwrap(), Language::Ruby);
        assert!("unknown".parse::<Language>().is_err());
    }

    #[test]
    fn coverage_gap_report_percent() {
        let report = CoverageGapReport {
            total_branches: 100,
            covered_branches: 75,
            uncovered: vec![],
        };
        assert!((report.coverage_percent() - 75.0).abs() < 0.01);

        let empty = CoverageGapReport {
            total_branches: 0,
            covered_branches: 0,
            uncovered: vec![],
        };
        assert!((empty.coverage_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn input_seed_creation() {
        let seed = InputSeed::new(vec![1, 2, 3], SeedOrigin::Fuzzer);
        assert_eq!(seed.data.as_ref(), &[1, 2, 3]);
        assert_eq!(seed.origin, SeedOrigin::Fuzzer);
        assert_eq!(seed.priority, 1.0);
        assert!(seed.target_branches.is_empty());
    }

    #[test]
    fn test_candidate_creation() {
        let tc = TestCandidate::new("def test(): pass".into(), Language::Python);
        assert_eq!(tc.language, Language::Python);
        assert!(tc.reasoning.is_empty());
        assert!(tc.target_branches.is_empty());
    }

    #[test]
    fn execution_trace_construction() {
        let trace = ExecutionTrace {
            lines_hit: vec![(1, 10), (1, 20), (2, 5)],
        };
        assert_eq!(trace.lines_hit.len(), 3);
        assert_eq!(trace.lines_hit[0], (1, 10));
        assert_eq!(trace.lines_hit[2], (2, 5));
    }

    #[test]
    fn seed_origin_all_variants() {
        let variants = [
            SeedOrigin::Corpus,
            SeedOrigin::Fuzzer,
            SeedOrigin::Concolic,
            SeedOrigin::Symbolic,
            SeedOrigin::Agent,
        ];
        // All variants are distinct
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn execution_status_all_variants() {
        let variants = [
            ExecutionStatus::Pass,
            ExecutionStatus::Fail,
            ExecutionStatus::Timeout,
            ExecutionStatus::Crash,
            ExecutionStatus::OomKill,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn branch_state_variants() {
        let uncovered = BranchState::Uncovered;
        let covered = BranchState::Covered {
            hit_count: 5,
            first_seed_id: SeedId::new(),
        };
        let unreachable = BranchState::Unreachable;
        let suppressed = BranchState::Suppressed;

        let dbg_uncovered = format!("{:?}", uncovered);
        let dbg_covered = format!("{:?}", covered);
        let dbg_unreachable = format!("{:?}", unreachable);
        let dbg_suppressed = format!("{:?}", suppressed);

        assert!(dbg_uncovered.contains("Uncovered"));
        assert!(dbg_covered.contains("Covered"));
        assert!(dbg_covered.contains("hit_count: 5"));
        assert!(dbg_unreachable.contains("Unreachable"));
        assert!(dbg_suppressed.contains("Suppressed"));
    }

    #[test]
    fn uncovered_branch_construction() {
        let ub = UncoveredBranch {
            branch: BranchId::new(99, 42, 3, 1),
            file_path: PathBuf::from("src/main.rs"),
            source_line: Some("if x > 0 {".into()),
        };
        assert_eq!(ub.branch.file_id, 99);
        assert_eq!(ub.branch.line, 42);
        assert_eq!(ub.file_path, PathBuf::from("src/main.rs"));
        assert_eq!(ub.source_line.as_deref(), Some("if x > 0 {"));
    }

    #[test]
    fn source_context_construction() {
        let ctx = SourceContext {
            file_path: PathBuf::from("lib.py"),
            lines: vec!["def foo():".into(), "    pass".into()],
            start_line: 10,
        };
        assert_eq!(ctx.file_path, PathBuf::from("lib.py"));
        assert_eq!(ctx.lines.len(), 2);
        assert_eq!(ctx.start_line, 10);
    }

    #[test]
    fn coverage_gap_report_with_data() {
        let report = CoverageGapReport {
            total_branches: 10,
            covered_branches: 7,
            uncovered: vec![
                UncoveredBranch {
                    branch: BranchId::new(1, 5, 0, 0),
                    file_path: PathBuf::from("a.py"),
                    source_line: None,
                },
                UncoveredBranch {
                    branch: BranchId::new(1, 8, 0, 1),
                    file_path: PathBuf::from("b.py"),
                    source_line: Some("else:".into()),
                },
            ],
        };
        assert!((report.coverage_percent() - 70.0).abs() < 0.01);
        assert_eq!(report.uncovered.len(), 2);
    }

    #[test]
    fn coverage_gap_report_100_percent() {
        let report = CoverageGapReport {
            total_branches: 50,
            covered_branches: 50,
            uncovered: vec![],
        };
        assert!((report.coverage_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn synthesized_test_construction() {
        let st = SynthesizedTest {
            path: PathBuf::from("tests/test_foo.py"),
            content: "def test_foo(): assert True".into(),
            covers_branches: vec![BranchId::new(1, 10, 0, 0), BranchId::new(1, 10, 0, 1)],
        };
        assert_eq!(st.path, PathBuf::from("tests/test_foo.py"));
        assert!(st.content.contains("test_foo"));
        assert_eq!(st.covers_branches.len(), 2);
    }

    #[test]
    fn exploration_context_construction() {
        let ctx = ExplorationContext {
            target: Target {
                root: PathBuf::from("/project"),
                language: Language::Rust,
                test_command: vec!["cargo".into(), "test".into()],
            },
            uncovered_branches: vec![BranchId::new(1, 1, 0, 0)],
            iteration: 42,
        };
        assert_eq!(ctx.target.language, Language::Rust);
        assert_eq!(ctx.uncovered_branches.len(), 1);
        assert_eq!(ctx.iteration, 42);
    }

    #[test]
    fn path_constraint_construction() {
        let pc = PathConstraint {
            branch: BranchId::new(5, 20, 0, 0),
            smtlib2: "(assert (> x 0))".into(),
            direction_taken: true,
        };
        assert_eq!(pc.branch.file_id, 5);
        assert_eq!(pc.smtlib2, "(assert (> x 0))");
        assert!(pc.direction_taken);
    }

    // -----------------------------------------------------------------------
    // MC/DC BranchId + CoverageLevel
    // -----------------------------------------------------------------------

    #[test]
    fn branch_id_with_condition_index() {
        let b = BranchId::new_mcdc(1, 10, 0, 0, Some(2));
        assert_eq!(b.condition_index, Some(2));
    }

    #[test]
    fn branch_id_new_has_no_condition_index() {
        let b = BranchId::new(1, 10, 0, 0);
        assert_eq!(b.condition_index, None);
    }

    #[test]
    fn coverage_level_display() {
        assert_eq!(CoverageLevel::Statement.to_string(), "statement");
        assert_eq!(CoverageLevel::Branch.to_string(), "branch");
        assert_eq!(CoverageLevel::Mcdc.to_string(), "mcdc");
    }

    #[test]
    fn branch_id_mcdc_distinct_from_plain() {
        let plain = BranchId::new(1, 10, 0, 0);
        let mcdc = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
        assert_ne!(plain, mcdc, "condition_index must participate in equality");
    }

    #[test]
    fn branch_id_mcdc_hash_consistent() {
        use std::collections::HashSet;
        let a = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
        let b = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn instrumented_target_construction() {
        use std::collections::HashMap;
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, PathBuf::from("src/main.rs"));
        file_paths.insert(2u64, PathBuf::from("src/lib.rs"));

        let it = InstrumentedTarget {
            target: Target {
                root: PathBuf::from("/project"),
                language: Language::C,
                test_command: vec!["make".into(), "test".into()],
            },
            branch_ids: vec![BranchId::new(1, 1, 0, 0), BranchId::new(2, 5, 0, 0)],
            executed_branch_ids: vec![BranchId::new(1, 1, 0, 0)],
            file_paths,
            work_dir: PathBuf::from("/tmp/work"),
        };
        assert_eq!(it.branch_ids.len(), 2);
        assert_eq!(it.executed_branch_ids.len(), 1);
        assert_eq!(it.file_paths.len(), 2);
        assert_eq!(it.work_dir, PathBuf::from("/tmp/work"));
    }

    #[test]
    fn test_candidate_new_various_languages() {
        let languages = [
            Language::Python,
            Language::JavaScript,
            Language::Java,
            Language::C,
            Language::Rust,
            Language::Wasm,
            Language::Ruby,
        ];
        for lang in languages {
            let tc = TestCandidate::new("code".into(), lang);
            assert_eq!(tc.language, lang);
            assert_eq!(tc.code, "code");
        }
    }

    #[test]
    fn seed_id_default_uses_new() {
        let id = SeedId::default();
        // Should produce a valid v4 UUID (non-nil)
        assert_ne!(id.0, Uuid::nil());
    }

    #[test]
    fn snapshot_id_default_uses_new() {
        let id = SnapshotId::default();
        assert_ne!(id.0, Uuid::nil());
    }

    #[test]
    fn language_display_all_variants() {
        assert_eq!(Language::Python.to_string(), "python");
        assert_eq!(Language::JavaScript.to_string(), "javascript");
        assert_eq!(Language::Java.to_string(), "java");
        assert_eq!(Language::C.to_string(), "c");
        assert_eq!(Language::Rust.to_string(), "rust");
        assert_eq!(Language::Wasm.to_string(), "wasm");
        assert_eq!(Language::Ruby.to_string(), "ruby");
    }

    #[test]
    fn language_parse_error_message() {
        let err = "foobar".parse::<Language>().unwrap_err();
        assert!(
            err.contains("foobar"),
            "error should contain the invalid string: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Bug tracking types
    // -----------------------------------------------------------------------

    #[test]
    fn bug_class_display() {
        assert_eq!(BugClass::Crash.to_string(), "crash");
        assert_eq!(BugClass::AssertionFailure.to_string(), "assertion_failure");
        assert_eq!(BugClass::Timeout.to_string(), "timeout");
        assert_eq!(BugClass::OomKill.to_string(), "oom_kill");
    }

    #[test]
    fn bug_class_from_status() {
        assert_eq!(BugClass::from_status(ExecutionStatus::Pass), None);
        assert_eq!(
            BugClass::from_status(ExecutionStatus::Fail),
            Some(BugClass::AssertionFailure)
        );
        assert_eq!(
            BugClass::from_status(ExecutionStatus::Timeout),
            Some(BugClass::Timeout)
        );
        assert_eq!(
            BugClass::from_status(ExecutionStatus::Crash),
            Some(BugClass::Crash)
        );
        assert_eq!(
            BugClass::from_status(ExecutionStatus::OomKill),
            Some(BugClass::OomKill)
        );
    }

    #[test]
    fn bug_report_new_defaults() {
        let report = BugReport::new(BugClass::Crash, SeedId::new(), "segfault at 0x0".into());
        assert_eq!(report.class, BugClass::Crash);
        assert_eq!(report.message, "segfault at 0x0");
        assert!(report.location.is_none());
        assert!(report.triggering_branches.is_empty());
        assert_eq!(report.discovered_at_iteration, 0);
    }

    #[test]
    fn bug_report_dedup_key_with_location() {
        let mut report = BugReport::new(BugClass::Crash, SeedId::new(), "seg".into());
        report.location = Some("src/main.rs:42".into());
        assert_eq!(report.dedup_key(), "crash:src/main.rs:42");
    }

    #[test]
    fn bug_report_dedup_key_without_location() {
        let report = BugReport::new(BugClass::Timeout, SeedId::new(), "timed out".into());
        assert_eq!(report.dedup_key(), "timeout:timed out");
    }

    #[test]
    fn bug_report_dedup_key_truncates_long_message() {
        let long_msg = "x".repeat(300);
        let report = BugReport::new(BugClass::Crash, SeedId::new(), long_msg);
        let key = report.dedup_key();
        // "crash:" prefix + 128 chars of message
        assert_eq!(key.len(), "crash:".len() + 128);
    }

    #[test]
    fn bug_summary_new() {
        let reports = vec![
            BugReport::new(BugClass::Crash, SeedId::new(), "a".into()),
            BugReport::new(BugClass::Crash, SeedId::new(), "b".into()),
            BugReport::new(BugClass::Timeout, SeedId::new(), "c".into()),
        ];
        let summary = BugSummary::new(reports);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.by_class["crash"], 2);
        assert_eq!(summary.by_class["timeout"], 1);
        assert_eq!(summary.reports.len(), 3);
    }

    #[test]
    fn bug_summary_empty() {
        let summary = BugSummary::new(vec![]);
        assert_eq!(summary.total, 0);
        assert!(summary.by_class.is_empty());
        assert!(summary.reports.is_empty());
    }

    #[test]
    fn bug_summary_default() {
        let summary = BugSummary::default();
        assert_eq!(summary.total, 0);
        assert!(summary.by_class.is_empty());
        assert!(summary.reports.is_empty());
    }
}
