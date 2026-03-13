//! Coverage tracking for APEX — bitmap-based edge coverage oracle with delta computation.

pub mod heuristic;
pub mod oracle;

pub use heuristic::{BranchHeuristic, CmpOp, branch_distance};
pub use oracle::{CoverageOracle, DeltaCoverage};
