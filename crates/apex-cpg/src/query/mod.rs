//! CodeQL-inspired query engine for the CPG.
//!
//! Provides a Datalog-inspired DSL for querying the Code Property Graph:
//!
//! ```text
//! from source in Sources("UserInput")
//! from sink in Sinks("SQLInjection")
//! where flows(source, sink)
//! select source.location, sink.location
//! ```

pub mod builtins;
pub mod executor;
pub mod parser;

pub use executor::{execute_query, QueryResult};
pub use parser::parse_query;

/// A single clause in a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExpr {
    From {
        variable: String,
        source: DataSource,
    },
    Where {
        condition: Condition,
    },
    Select {
        fields: Vec<Field>,
    },
}

/// Data sources that a `from` clause can bind to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataSource {
    /// Taint sources matching a kind (e.g., "UserInput").
    Sources(String),
    /// Taint sinks matching a kind (e.g., "SQLInjection").
    Sinks(String),
    /// All call expressions in the CPG.
    Calls,
    /// All assignment nodes in the CPG.
    Assignments,
    /// All function/method definitions in the CPG.
    Functions,
    /// Nodes matching a specific CPG node type name.
    NodeType(String),
}

/// Conditions for the `where` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `flows(a, b)` — a taint path exists from variable `a` to variable `b`.
    Flows(String, String),
    /// `sanitized(a, b, kind)` — flow from `a` to `b` passes through sanitizer of `kind`.
    Sanitized(String, String, String),
    /// `matches(variable, regex)` — a field of the variable matches the regex pattern.
    Matches(String, String),
    /// Conjunction of two conditions.
    And(Box<Condition>, Box<Condition>),
    /// Disjunction of two conditions.
    Or(Box<Condition>, Box<Condition>),
    /// Negation of a condition.
    Not(Box<Condition>),
}

/// A field reference in a `select` clause: `variable.attribute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub variable: String,
    pub attribute: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_expr_types_constructible() {
        let from = QueryExpr::From {
            variable: "src".into(),
            source: DataSource::Sources("UserInput".into()),
        };
        let wh = QueryExpr::Where {
            condition: Condition::Flows("src".into(), "sink".into()),
        };
        let sel = QueryExpr::Select {
            fields: vec![Field {
                variable: "src".into(),
                attribute: "location".into(),
            }],
        };
        assert!(matches!(from, QueryExpr::From { .. }));
        assert!(matches!(wh, QueryExpr::Where { .. }));
        assert!(matches!(sel, QueryExpr::Select { .. }));
    }

    #[test]
    fn condition_compound_types() {
        let c = Condition::And(
            Box::new(Condition::Flows("a".into(), "b".into())),
            Box::new(Condition::Not(Box::new(Condition::Sanitized(
                "a".into(),
                "b".into(),
                "xss".into(),
            )))),
        );
        assert!(matches!(c, Condition::And(..)));
    }
}
