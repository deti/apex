pub mod c_cpp;
pub mod csharp;
pub mod go;
pub mod ruby;
pub mod java;
pub mod javascript;
pub mod python;
pub mod rust;
pub mod swift;

use apex_core::types::Language;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::graph::CallGraph;

pub trait CallGraphExtractor: Send + Sync {
    fn language(&self) -> Language;
    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph;
}

/// Build a call graph using the appropriate extractor for the language.
pub fn build_call_graph(sources: &HashMap<PathBuf, String>, lang: Language) -> CallGraph {
    match lang {
        Language::Rust => rust::RustExtractor.extract(sources),
        Language::Python => python::PythonExtractor.extract(sources),
        Language::JavaScript => javascript::JsExtractor.extract(sources),
        Language::Java | Language::Kotlin => java::JavaExtractor.extract(sources),
        Language::Go => go::GoExtractor.extract(sources),
        Language::C | Language::Cpp => c_cpp::CCppExtractor.extract(sources),
        Language::Ruby => ruby::RubyExtractor.extract(sources),
        _ => CallGraph::default(),
    }
}
