//! Multi-language instrumentation for APEX.
//!
//! Provides `Instrumentor` implementations for Python, JavaScript, Java, Rust,
//! LLVM IR (feature-gated), and WebAssembly (feature-gated).

pub mod c_coverage;
pub mod csharp;
pub mod go;
pub mod import;
pub mod java;
pub mod javascript;
pub mod llvm;
pub mod python;
pub mod ruby;
pub mod rust_cov;
pub mod rustc_wrapper;
pub mod source_map;
pub mod swift;
pub mod v8_coverage;
pub mod wasm;

pub use c_coverage::CCoverageInstrumentor;
pub use csharp::CSharpInstrumentor;
pub use go::GoInstrumentor;
pub use java::JavaInstrumentor;
pub use javascript::JavaScriptInstrumentor;
pub use llvm::LlvmInstrumentor;
pub use python::PythonInstrumentor;
pub use ruby::RubyInstrumentor;
pub use rust_cov::RustCovInstrumentor;
pub use swift::SwiftInstrumentor;
pub use wasm::WasmInstrumentor;
