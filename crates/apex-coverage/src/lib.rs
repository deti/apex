//! Coverage tracking for APEX — bitmap-based edge coverage oracle with delta computation.

pub mod heuristic;
pub mod oracle;

pub use heuristic::{branch_distance, BranchHeuristic, CmpOp};
pub use oracle::{CoverageOracle, DeltaCoverage};
