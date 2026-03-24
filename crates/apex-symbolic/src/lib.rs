//! Symbolic constraint solving for APEX.
//!
//! Includes an SMT-LIB2 solver, portfolio strategies, caching,
//! and optional Z3/Kani integration behind feature flags.

pub mod bmc;
pub mod cache;
pub mod diversity;
pub mod gradient;
pub mod landscape;
pub mod llm_solver;
pub mod path_decomp;
pub mod portfolio;
pub mod smtlib;
pub mod solver;
pub mod summaries;
pub mod traits;

pub use cache::CachingSolver;
pub use diversity::DiversitySolver;
pub use gradient::GradientSolver;
pub use landscape::{LandscapeAnalyzer, StrategyHint};
pub use llm_solver::{constraints_to_prompt, parse_llm_solution, LlmSolver};
pub use path_decomp::PathDecomposer;
pub use portfolio::PortfolioSolver;
pub use solver::{solve, SymbolicSession, Z3Solver};
pub use traits::{Solver, SolverLogic};
