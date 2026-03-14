//! Multi-language instrumentation for APEX.
//!
//! Provides `Instrumentor` implementations for Python, JavaScript, Java, Rust,
//! LLVM IR (feature-gated), and WebAssembly (feature-gated).

pub mod java;
pub mod javascript;
pub mod llvm;
pub mod python;
pub mod rust_cov;
pub mod rustc_wrapper;
pub mod source_map;
pub mod v8_coverage;
pub mod wasm;

pub use java::JavaInstrumentor;
pub use javascript::JavaScriptInstrumentor;
pub use llvm::LlvmInstrumentor;
pub use python::PythonInstrumentor;
pub use rust_cov::RustCovInstrumentor;
pub use wasm::WasmInstrumentor;
