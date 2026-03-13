//! Symbolic constraint solving for APEX.
//!
//! Includes an SMT-LIB2 solver, portfolio strategies, caching,
//! and optional Z3/Kani integration behind feature flags.

pub mod bmc;
pub mod cache;
pub mod gradient;
pub mod portfolio;
pub mod smtlib;
pub mod solver;
pub mod summaries;
pub mod traits;

pub use cache::CachingSolver;
pub use gradient::GradientSolver;
pub use portfolio::PortfolioSolver;
pub use solver::{solve, SymbolicSession, Z3Solver};
pub use traits::{Solver, SolverLogic};
