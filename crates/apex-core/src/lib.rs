//! Core foundation for APEX — shared types, traits, configuration, and error handling.
//!
//! All other APEX crates depend on `apex-core` for common abstractions.

pub mod agent_report;
pub mod command;
pub mod config;
pub mod error;
pub mod fixture_runner;
pub mod git;
pub mod hash;
pub mod llm;
pub mod path_shim;
pub mod traits;
pub mod types;

pub use config::ApexConfig;
pub use error::{ApexError, Result};
