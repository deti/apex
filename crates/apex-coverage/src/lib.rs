//! Coverage tracking for APEX — bitmap-based edge coverage oracle with delta computation.

pub mod heuristic;
pub mod mutation;
pub mod oracle;
pub mod oracle_gap;
pub mod semantic;

pub use heuristic::{BranchHeuristic, CmpOp, branch_distance};
pub use oracle::{CoverageOracle, DeltaCoverage};
pub use oracle_gap::OracleGapScore;
pub use semantic::{SemanticSignals, extract_signals};
