//! Concolic execution engine for APEX — combines concrete execution
//! with symbolic constraint collection for systematic path exploration.

pub mod boundary;
pub mod c_conditions;
pub mod condition_tree;
pub mod csharp_conditions;
pub mod go_conditions;
pub mod java_conditions;
pub mod js_conditions;
pub mod python;
pub mod ruby_conditions;
pub mod rust_conditions;
pub mod search;
pub mod selective;
pub mod static_strategy;
pub mod swift_conditions;
pub mod symcc;
pub mod taint;

pub use boundary::boundary_values;
pub use c_conditions::parse_c_conditions;
pub use csharp_conditions::parse_csharp_conditions;
pub use go_conditions::parse_go_conditions;
pub use java_conditions::parse_java_conditions;
pub use js_conditions::parse_js_condition;
pub use python::PythonConcolicStrategy;
pub use ruby_conditions::parse_ruby_conditions;
pub use rust_conditions::parse_rust_conditions;
pub use static_strategy::StaticConcolicStrategy;
pub use swift_conditions::parse_swift_conditions;
pub use symcc::{PathConstraint, SymCcBackend, SymCcStrategy};
