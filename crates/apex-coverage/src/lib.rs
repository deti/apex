//! Coverage tracking for APEX — bitmap-based edge coverage oracle with delta computation.

pub mod compound;
pub mod heuristic;
pub mod mutation;
pub mod oracle;
pub mod oracle_gap;
pub mod semantic;

pub use compound::{CompoundOracle, CoverageSignal};
pub use heuristic::{branch_distance, BranchHeuristic, CmpOp};
pub use oracle::{CoverageOracle, DeltaCoverage};
pub use oracle_gap::OracleGapScore;
pub use semantic::{extract_signals, SemanticSignals};
