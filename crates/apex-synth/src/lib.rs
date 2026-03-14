//! Template-based test synthesis for APEX using Tera templates.
//!
//! Generates test files for pytest, Jest, JUnit, and cargo-test.
//! Also provides LLM-guided closed-loop refinement via `llm` and `segment`.

pub mod classify;
pub mod coverup;
pub mod eliminate;
pub mod jest;
pub mod junit;
pub mod llm;
pub mod python;
pub mod rust;
pub mod segment;
pub mod strategy;

pub use classify::{GapClassifier, GapKind};
pub use coverup::CoverUpStrategy;
pub use eliminate::eliminate_irrelevant;
pub use jest::JestSynthesizer;
pub use junit::JUnitSynthesizer;
pub use llm::{CoverageGap, LlmConfig, LlmMessage, LlmRole, LlmSynthesizer, SynthAttempt, TestResult};
pub use python::PytestSynthesizer;
pub use rust::CargoTestSynthesizer;
pub use segment::{clean_error_output, extract_segment, CodeSegment};
pub use strategy::{GapHistory, PromptStrategy};
