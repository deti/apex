//! Query execution engine — evaluates parsed queries against a CPG.

use std::collections::HashMap;

use anyhow::Result;
use regex::Regex;

use crate::taint::reachable_by;
use crate::taint_rules::TaintRuleSet;
use crate::{Cpg, NodeId};

use super::builtins::{
    node_location, node_name, resolve_assignments, resolve_calls, resolve_functions,
    resolve_node_type, resolve_sinks, resolve_sources,
};
use super::{Condition, DataSource, Field, QueryExpr};

/// The result of executing a query: a table of rows.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Each row is a list of (field_name, value) pairs.
    pub rows: Vec<Vec<(String, String)>>,
}

impl QueryResult {
    /// Number of result rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the result set is empty.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Execute a parsed query against a CPG with the given taint rules.
///
/// The query is processed in three phases:
/// 1. **Bind** — resolve `from` clauses to sets of CPG node IDs.
/// 2. **Filter** — apply `where` conditions to eliminate non-matching bindings.
/// 3. **Project** — extract `select` fields from surviving bindings.
pub fn execute_query(
    exprs: &[QueryExpr],
    cpg: &Cpg,
    taint_rules: &TaintRuleSet,
) -> Result<QueryResult> {
    // Phase 1: collect bindings from `from` clauses.
    let mut bindings: HashMap<String, Vec<NodeId>> = HashMap::new();
    let mut where_conditions: Vec<&Condition> = Vec::new();
    let mut select_fields: Vec<&Field> = Vec::new();

    for expr in exprs {
        match expr {
            QueryExpr::From { variable, source } => {
                let nodes = resolve_data_source(cpg, source, taint_rules);
                bindings.insert(variable.clone(), nodes);
            }
            QueryExpr::Where { condition } => {
                where_conditions.push(condition);
            }
            QueryExpr::Select { fields } => {
                select_fields.extend(fields.iter());
            }
        }
    }

    // If no select fields specified, we default to all bindings' locations.
    if select_fields.is_empty() && !bindings.is_empty() {
        // Return a row count equal to the cartesian product, projected as empty rows.
        let rows = compute_matching_rows(cpg, &bindings, &where_conditions, taint_rules)?;
        let result_rows: Vec<Vec<(String, String)>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|(var, nid)| {
                        let node = cpg.node(*nid);
                        let loc = node.map(node_location).unwrap_or_default();
                        (var.clone(), loc)
                    })
                    .collect()
            })
            .collect();
        return Ok(QueryResult { rows: result_rows });
    }

    // Phase 2+3: compute matching rows and project selected fields.
    let rows = compute_matching_rows(cpg, &bindings, &where_conditions, taint_rules)?;

    let result_rows: Vec<Vec<(String, String)>> = rows
        .iter()
        .map(|row| {
            select_fields
                .iter()
                .map(|field| {
                    let value = project_field(cpg, &row_to_map(row), field);
                    (format!("{}.{}", field.variable, field.attribute), value)
                })
                .collect()
        })
        .collect();

    Ok(QueryResult { rows: result_rows })
}

/// Resolve a DataSource to a list of node IDs.
fn resolve_data_source(cpg: &Cpg, source: &DataSource, rules: &TaintRuleSet) -> Vec<NodeId> {
    match source {
        DataSource::Sources(kind) => resolve_sources(cpg, kind, rules),
        DataSource::Sinks(kind) => resolve_sinks(cpg, kind, rules),
        DataSource::Calls => resolve_calls(cpg),
        DataSource::Assignments => resolve_assignments(cpg),
        DataSource::Functions => resolve_functions(cpg),
        DataSource::NodeType(name) => resolve_node_type(cpg, name),
    }
}

/// Compute matching rows from the cartesian product of bindings, filtered by conditions.
///
/// Each row is a Vec of (variable_name, NodeId).
fn compute_matching_rows(
    cpg: &Cpg,
    bindings: &HashMap<String, Vec<NodeId>>,
    conditions: &[&Condition],
    rules: &TaintRuleSet,
) -> Result<Vec<Vec<(String, NodeId)>>> {
    if bindings.is_empty() {
        return Ok(Vec::new());
    }

    // Build the cartesian product of all bindings.
    let vars: Vec<String> = bindings.keys().cloned().collect();
    let mut rows: Vec<Vec<(String, NodeId)>> = vec![vec![]];

    for var in &vars {
        let node_ids = &bindings[var];
        let mut new_rows = Vec::new();
        for row in &rows {
            for &nid in node_ids {
                let mut new_row = row.clone();
                new_row.push((var.clone(), nid));
                new_rows.push(new_row);
            }
        }
        rows = new_rows;
    }

    // Filter by conditions.
    for cond in conditions {
        rows.retain(|row| {
            let map = row_to_map(row);
            evaluate_condition(cpg, &map, cond, rules)
        });
    }

    Ok(rows)
}

/// Convert a row to a variable -> NodeId map.
fn row_to_map(row: &[(String, NodeId)]) -> HashMap<String, NodeId> {
    row.iter().cloned().collect()
}

/// Evaluate a condition against a set of variable bindings.
#[allow(clippy::only_used_in_recursion)]
fn evaluate_condition(
    cpg: &Cpg,
    bindings: &HashMap<String, NodeId>,
    condition: &Condition,
    rules: &TaintRuleSet,
) -> bool {
    match condition {
        Condition::Flows(src_var, sink_var) => {
            let Some(&src_id) = bindings.get(src_var.as_str()) else {
                return false;
            };
            let Some(&sink_id) = bindings.get(sink_var.as_str()) else {
                return false;
            };
            // Use existing backward taint BFS to check reachability.
            let flows = reachable_by(cpg, &[sink_id], &[src_id], 20);
            !flows.is_empty()
        }
        Condition::Sanitized(src_var, sink_var, _kind) => {
            // Check if there's a flow AND the flow passes through a sanitizer.
            let Some(&src_id) = bindings.get(src_var.as_str()) else {
                return false;
            };
            let Some(&sink_id) = bindings.get(sink_var.as_str()) else {
                return false;
            };
            // A flow exists without sanitizers means it's NOT sanitized.
            // We check: does a flow exist if we ignore sanitizers?
            // If yes but not with sanitizers, then it's sanitized.
            let flows_with_sanitizers = reachable_by(cpg, &[sink_id], &[src_id], 20);
            // If there are no flows (sanitizer blocked them), the flow IS sanitized.
            // We need to check if there would be a flow without the sanitizer.
            // For simplicity: "sanitized" is true when the flow is blocked by sanitizers.
            flows_with_sanitizers.is_empty() && {
                // Check if a path structurally exists by looking at all ReachingDef edges
                // This is a heuristic — a full implementation would remove sanitizer nodes
                // and re-run the analysis.
                has_structural_path(cpg, src_id, sink_id)
            }
        }
        Condition::Matches(var, pattern) => {
            let Some(&nid) = bindings.get(var.as_str()) else {
                return false;
            };
            let Some(node) = cpg.node(nid) else {
                return false;
            };
            let name = node_name(node);
            match Regex::new(pattern) {
                Ok(re) => re.is_match(name),
                Err(_) => false,
            }
        }
        Condition::And(left, right) => {
            evaluate_condition(cpg, bindings, left, rules)
                && evaluate_condition(cpg, bindings, right, rules)
        }
        Condition::Or(left, right) => {
            evaluate_condition(cpg, bindings, left, rules)
                || evaluate_condition(cpg, bindings, right, rules)
        }
        Condition::Not(inner) => !evaluate_condition(cpg, bindings, inner, rules),
    }
}

/// Check if a structural path exists from source to sink via ReachingDef edges
/// (ignoring sanitizer blocking). Used as a heuristic for the `sanitized` predicate.
fn has_structural_path(cpg: &Cpg, source: NodeId, sink: NodeId) -> bool {
    use std::collections::{HashSet, VecDeque};
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(sink);
    visited.insert(sink);
    while let Some(current) = queue.pop_front() {
        for (from, _to, kind) in cpg.edges_to(current) {
            if !matches!(kind, crate::EdgeKind::ReachingDef { .. }) {
                continue;
            }
            if *from == source {
                return true;
            }
            if visited.insert(*from) {
                queue.push_back(*from);
            }
        }
    }
    false
}

/// Project a field from a node: extract the requested attribute.
fn project_field(cpg: &Cpg, bindings: &HashMap<String, NodeId>, field: &Field) -> String {
    let Some(&nid) = bindings.get(field.variable.as_str()) else {
        return String::new();
    };
    let Some(node) = cpg.node(nid) else {
        return String::new();
    };
    match field.attribute.as_str() {
        "name" => node_name(node).to_string(),
        "location" => node_location(node),
        "line" => super::builtins::node_line(node)
            .map(|l| l.to_string())
            .unwrap_or_default(),
        "file" => super::builtins::node_file(node).unwrap_or("").to_string(),
        _ => format!("<unknown attribute: {}>", field.attribute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeKind, NodeKind};

    /// Helper: build a small CPG with a taint flow from a Parameter to a Call sink.
    fn cpg_with_flow() -> Cpg {
        let mut cpg = Cpg::new();
        // 0: Method
        cpg.add_node(NodeKind::Method {
            name: "handle".into(),
            file: "app.py".into(),
            line: 1,
        });
        // 1: Parameter (source)
        cpg.add_node(NodeKind::Parameter {
            name: "user_input".into(),
            index: 0,
        });
        // 2: Assignment
        cpg.add_node(NodeKind::Assignment {
            lhs: "query".into(),
            line: 2,
        });
        // 3: Call (sink: cursor.execute)
        cpg.add_node(NodeKind::Call {
            name: "cursor.execute".into(),
            line: 3,
        });

        // Edges: param -> assignment -> sink via ReachingDef
        cpg.add_edge(
            1,
            2,
            EdgeKind::ReachingDef {
                variable: "user_input".into(),
            },
        );
        cpg.add_edge(
            2,
            3,
            EdgeKind::ReachingDef {
                variable: "query".into(),
            },
        );
        cpg.add_edge(0, 1, EdgeKind::Ast);
        cpg
    }

    /// Helper: build a CPG where a sanitizer blocks the flow.
    fn cpg_with_sanitized_flow() -> Cpg {
        let mut cpg = Cpg::new();
        // 0: Parameter (source)
        cpg.add_node(NodeKind::Parameter {
            name: "user_input".into(),
            index: 0,
        });
        // 1: Call (sanitizer: shlex.quote — must match PYTHON_SANITIZERS)
        cpg.add_node(NodeKind::Call {
            name: "shlex.quote".into(),
            line: 2,
        });
        // 2: Call (sink: eval)
        cpg.add_node(NodeKind::Call {
            name: "eval".into(),
            line: 3,
        });

        // param -> sanitizer -> sink
        cpg.add_edge(
            0,
            1,
            EdgeKind::ReachingDef {
                variable: "user_input".into(),
            },
        );
        cpg.add_edge(
            1,
            2,
            EdgeKind::ReachingDef {
                variable: "safe".into(),
            },
        );
        cpg
    }

    #[test]
    fn execute_finds_calls_matching_pattern() {
        let cpg = cpg_with_flow();
        let rules = TaintRuleSet::python_defaults();
        let query = vec![
            QueryExpr::From {
                variable: "c".into(),
                source: DataSource::Calls,
            },
            QueryExpr::Where {
                condition: Condition::Matches("c".into(), "cursor.*".into()),
            },
            QueryExpr::Select {
                fields: vec![super::super::Field {
                    variable: "c".into(),
                    attribute: "name".into(),
                }],
            },
        ];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.rows[0][0].1, "cursor.execute");
    }

    #[test]
    fn execute_flows_source_to_sink() {
        let cpg = cpg_with_flow();
        let rules = TaintRuleSet::python_defaults();
        let query = vec![
            QueryExpr::From {
                variable: "source".into(),
                source: DataSource::Sources("UserInput".into()),
            },
            QueryExpr::From {
                variable: "sink".into(),
                source: DataSource::Sinks("SQLInjection".into()),
            },
            QueryExpr::Where {
                condition: Condition::Flows("source".into(), "sink".into()),
            },
            QueryExpr::Select {
                fields: vec![
                    super::super::Field {
                        variable: "source".into(),
                        attribute: "name".into(),
                    },
                    super::super::Field {
                        variable: "sink".into(),
                        attribute: "name".into(),
                    },
                ],
            },
        ];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        assert!(!result.is_empty(), "should find flow from source to sink");
        // The source should be the parameter
        let source_name = &result.rows[0][0].1;
        assert_eq!(source_name, "user_input");
        let sink_name = &result.rows[0][1].1;
        assert_eq!(sink_name, "cursor.execute");
    }

    #[test]
    fn execute_no_results_when_sanitized() {
        let cpg = cpg_with_sanitized_flow();
        let rules = TaintRuleSet::python_defaults();
        let query = vec![
            QueryExpr::From {
                variable: "source".into(),
                source: DataSource::Sources("UserInput".into()),
            },
            QueryExpr::From {
                variable: "sink".into(),
                source: DataSource::Sinks("Eval".into()),
            },
            QueryExpr::Where {
                condition: Condition::Flows("source".into(), "sink".into()),
            },
            QueryExpr::Select {
                fields: vec![super::super::Field {
                    variable: "source".into(),
                    attribute: "name".into(),
                }],
            },
        ];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        // The sanitizer should block the flow, so no results
        assert!(
            result.is_empty(),
            "sanitizer should block flow; got {} rows",
            result.len()
        );
    }

    #[test]
    fn execute_select_projects_fields() {
        let cpg = cpg_with_flow();
        let rules = TaintRuleSet::python_defaults();
        let query = vec![
            QueryExpr::From {
                variable: "f".into(),
                source: DataSource::Functions,
            },
            QueryExpr::Select {
                fields: vec![
                    super::super::Field {
                        variable: "f".into(),
                        attribute: "name".into(),
                    },
                    super::super::Field {
                        variable: "f".into(),
                        attribute: "file".into(),
                    },
                    super::super::Field {
                        variable: "f".into(),
                        attribute: "line".into(),
                    },
                ],
            },
        ];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.rows[0][0].1, "handle");
        assert_eq!(result.rows[0][1].1, "app.py");
        assert_eq!(result.rows[0][2].1, "1");
    }

    #[test]
    fn execute_empty_bindings_returns_empty() {
        let cpg = Cpg::new();
        let rules = TaintRuleSet::empty();
        let query = vec![QueryExpr::From {
            variable: "x".into(),
            source: DataSource::Calls,
        }];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn execute_not_condition() {
        let cpg = cpg_with_flow();
        let rules = TaintRuleSet::python_defaults();
        // Select calls that do NOT match "print"
        let query = vec![
            QueryExpr::From {
                variable: "c".into(),
                source: DataSource::Calls,
            },
            QueryExpr::Where {
                condition: Condition::Not(Box::new(Condition::Matches(
                    "c".into(),
                    "^print$".into(),
                ))),
            },
            QueryExpr::Select {
                fields: vec![super::super::Field {
                    variable: "c".into(),
                    attribute: "name".into(),
                }],
            },
        ];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        // Only cursor.execute should remain (no "print" calls in this CPG)
        assert_eq!(result.len(), 1);
        assert_eq!(result.rows[0][0].1, "cursor.execute");
    }

    #[test]
    fn execute_sanitized_condition() {
        let cpg = cpg_with_sanitized_flow();
        let rules = TaintRuleSet::python_defaults();
        let query = vec![
            QueryExpr::From {
                variable: "source".into(),
                source: DataSource::Sources("UserInput".into()),
            },
            QueryExpr::From {
                variable: "sink".into(),
                source: DataSource::Sinks("Eval".into()),
            },
            QueryExpr::Where {
                condition: Condition::Sanitized("source".into(), "sink".into(), "xss".into()),
            },
            QueryExpr::Select {
                fields: vec![super::super::Field {
                    variable: "source".into(),
                    attribute: "name".into(),
                }],
            },
        ];
        let result = execute_query(&query, &cpg, &rules).unwrap();
        // The flow IS sanitized (escape blocks it), so the sanitized condition should match
        assert!(
            !result.is_empty(),
            "sanitized condition should match when flow is blocked by sanitizer"
        );
    }

    #[test]
    fn execute_from_string_query() {
        let cpg = cpg_with_flow();
        let rules = TaintRuleSet::python_defaults();
        let query_str = r#"from c in Calls
where matches(c, "cursor.*")
select c.name"#;
        let exprs = super::super::parser::parse_query(query_str).unwrap();
        let result = execute_query(&exprs, &cpg, &rules).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.rows[0][0].1, "cursor.execute");
    }
}
