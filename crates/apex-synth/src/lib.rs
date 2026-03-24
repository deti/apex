//! Template-based test synthesis for APEX using Tera templates.
//!
//! Generates test files for pytest, Jest, JUnit, and cargo-test.
//! Also provides LLM-guided closed-loop refinement via `llm` and `segment`.

pub mod c_test;
pub mod classify;
pub mod cot;
pub mod coverup;
pub mod cpg_context;
pub mod delta;
pub mod eliminate;
pub mod error_classify;
pub mod extractor;
pub mod few_shot;
pub mod go;
pub mod gtest;
pub mod jest;
pub mod junit;
pub mod kotlin;
pub mod llm;
pub mod mutation_gen;
pub mod mutation_hint;
pub mod prompt_registry;
pub mod property;
pub mod python;
pub mod rspec;
pub mod rust;
pub mod segment;
pub mod strategy;
pub mod wasm;
pub mod xctest;
pub mod xunit;

pub use c_test::CTestSynthesizer;
pub use classify::{GapClassifier, GapKind};
pub use cot::build_cot_prompt;
pub use coverup::CoverUpStrategy;
pub use cpg_context::build_cpg_prompt_context;
pub use delta::{coverage_delta, format_delta_summary};
pub use eliminate::eliminate_irrelevant;
pub use error_classify::{classify_test_error, refinement_prompt, ErrorKind};
pub use extractor::{best_test_block, extract_code_blocks, CodeBlock};
pub use few_shot::{format_few_shot_block, FewShotBank, FewShotExample};
pub use go::GoTestSynthesizer;
pub use gtest::CppTestSynthesizer;
pub use jest::JestSynthesizer;
pub use junit::JUnitSynthesizer;
pub use kotlin::KotlinTestSynthesizer;
pub use llm::{
    CoverageGap, LlmConfig, LlmMessage, LlmRole, LlmSynthesizer, SynthAttempt, TestResult,
};
pub use mutation_gen::{GeneratedTest, Mutation, MutationOperator, MutationTestGenerator};
pub use mutation_hint::{format_hints_block, MutationHint};
pub use prompt_registry::PromptRegistry;
pub use property::{InferredProperty, PropertyInferer};
pub use python::PytestSynthesizer;
pub use rspec::RubyTestSynthesizer;
pub use rust::CargoTestSynthesizer;
pub use segment::{clean_error_output, extract_segment, CodeSegment};
pub use strategy::{GapHistory, PromptStrategy};
pub use wasm::WasmTestSynthesizer;
pub use xctest::SwiftTestSynthesizer;
pub use xunit::CSharpTestSynthesizer;
