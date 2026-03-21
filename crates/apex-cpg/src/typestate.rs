//! Type-state analysis for resource lifecycle tracking.
//!
//! Tracks object states (Created → Active → Consumed) through CFG edges
//! and detects violations: use-after-close (CWE-416), double-free (CWE-675),
//! resource leak (CWE-404), and double-acquire (deadlock).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{Cpg, EdgeKind, NodeId, NodeKind};

// ---------------------------------------------------------------------------
// State machine types
// ---------------------------------------------------------------------------

/// Lifecycle state of a tracked resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceState {
    Unknown,
    Created,
    Active,
    Consumed,
    Error,
}

impl std::fmt::Display for ResourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceState::Unknown => write!(f, "Unknown"),
            ResourceState::Created => write!(f, "Created"),
            ResourceState::Active => write!(f, "Active"),
            ResourceState::Consumed => write!(f, "Consumed"),
            ResourceState::Error => write!(f, "Error"),
        }
    }
}

/// Kind of type-state violation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ViolationKind {
    /// CWE-416: using a resource after it has been closed/freed.
    UseAfterClose,
    /// CWE-675: closing/freeing a resource that is already consumed.
    DoubleFree,
    /// CWE-404: resource not closed before end of scope.
    ResourceLeak,
    /// Deadlock: acquiring a lock that is already held.
    DoubleAcquire,
}

impl ViolationKind {
    /// Return the primary CWE ID for this violation kind.
    pub fn cwe_id(&self) -> u32 {
        match self {
            ViolationKind::UseAfterClose => 416,
            ViolationKind::DoubleFree => 675,
            ViolationKind::ResourceLeak => 404,
            ViolationKind::DoubleAcquire => 764,
        }
    }
}

impl std::fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViolationKind::UseAfterClose => write!(f, "use-after-close"),
            ViolationKind::DoubleFree => write!(f, "double-free/double-close"),
            ViolationKind::ResourceLeak => write!(f, "resource-leak"),
            ViolationKind::DoubleAcquire => write!(f, "double-acquire"),
        }
    }
}

/// A transition in the state machine: (from_state, method_pattern, to_state).
#[derive(Debug, Clone)]
pub struct Transition {
    pub from: ResourceState,
    pub method: String,
    pub to: ResourceState,
}

/// A violation rule: calling `method` while in `state` produces `kind`.
#[derive(Debug, Clone)]
pub struct ViolationRule {
    pub state: ResourceState,
    pub method: String,
    pub kind: ViolationKind,
}

/// A state machine describing the lifecycle of a resource type.
#[derive(Debug, Clone)]
pub struct StateMachine {
    pub name: String,
    /// Patterns that create the resource (matched against call names).
    pub create_patterns: Vec<String>,
    /// Valid transitions.
    pub transitions: Vec<Transition>,
    /// Rules that trigger violations.
    pub violations: Vec<ViolationRule>,
    /// Whether end-of-scope in Active or Created state is a leak.
    pub leak_on_scope_exit: bool,
}

/// A detected type-state violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeStateViolation {
    pub variable: String,
    pub kind: ViolationKind,
    pub line: u32,
    pub state_at_violation: ResourceState,
    pub method: String,
    pub machine_name: String,
}

// ---------------------------------------------------------------------------
// Built-in state machines
// ---------------------------------------------------------------------------

/// File resource state machine.
pub fn file_state_machine() -> StateMachine {
    StateMachine {
        name: "File".into(),
        create_patterns: vec![
            "open".into(),
            "File.open".into(),
            "File::open".into(),
            "fopen".into(),
            "OpenFile".into(),
        ],
        transitions: vec![
            Transition {
                from: ResourceState::Created,
                method: "read".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "write".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "readline".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "readlines".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "read".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "write".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "readline".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "readlines".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "close".into(),
                to: ResourceState::Consumed,
            },
            Transition {
                from: ResourceState::Active,
                method: "close".into(),
                to: ResourceState::Consumed,
            },
        ],
        violations: vec![
            ViolationRule {
                state: ResourceState::Consumed,
                method: "read".into(),
                kind: ViolationKind::UseAfterClose,
            },
            ViolationRule {
                state: ResourceState::Consumed,
                method: "write".into(),
                kind: ViolationKind::UseAfterClose,
            },
            ViolationRule {
                state: ResourceState::Consumed,
                method: "readline".into(),
                kind: ViolationKind::UseAfterClose,
            },
            ViolationRule {
                state: ResourceState::Consumed,
                method: "readlines".into(),
                kind: ViolationKind::UseAfterClose,
            },
            ViolationRule {
                state: ResourceState::Consumed,
                method: "close".into(),
                kind: ViolationKind::DoubleFree,
            },
        ],
        leak_on_scope_exit: true,
    }
}

/// Mutex/Lock resource state machine.
pub fn mutex_state_machine() -> StateMachine {
    StateMachine {
        name: "Mutex".into(),
        create_patterns: vec![
            "Mutex::new".into(),
            "Lock::new".into(),
            "RwLock::new".into(),
            "threading.Lock".into(),
            "Lock()".into(),
            "RLock()".into(),
            "Semaphore".into(),
        ],
        transitions: vec![
            Transition {
                from: ResourceState::Created,
                method: "lock".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "acquire".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "unlock".into(),
                to: ResourceState::Consumed,
            },
            Transition {
                from: ResourceState::Active,
                method: "release".into(),
                to: ResourceState::Consumed,
            },
            // Re-acquirable after release
            Transition {
                from: ResourceState::Consumed,
                method: "lock".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Consumed,
                method: "acquire".into(),
                to: ResourceState::Active,
            },
        ],
        violations: vec![
            ViolationRule {
                state: ResourceState::Active,
                method: "lock".into(),
                kind: ViolationKind::DoubleAcquire,
            },
            ViolationRule {
                state: ResourceState::Active,
                method: "acquire".into(),
                kind: ViolationKind::DoubleAcquire,
            },
        ],
        leak_on_scope_exit: true,
    }
}

/// Database connection state machine.
pub fn db_connection_state_machine() -> StateMachine {
    StateMachine {
        name: "DBConnection".into(),
        create_patterns: vec![
            "connect".into(),
            "Connection".into(),
            "create_connection".into(),
            "DriverManager.getConnection".into(),
            "psycopg2.connect".into(),
            "sqlite3.connect".into(),
            "mysql.connector.connect".into(),
        ],
        transitions: vec![
            Transition {
                from: ResourceState::Created,
                method: "query".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "execute".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "cursor".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "query".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "execute".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "commit".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Active,
                method: "rollback".into(),
                to: ResourceState::Active,
            },
            Transition {
                from: ResourceState::Created,
                method: "close".into(),
                to: ResourceState::Consumed,
            },
            Transition {
                from: ResourceState::Active,
                method: "close".into(),
                to: ResourceState::Consumed,
            },
        ],
        violations: vec![
            ViolationRule {
                state: ResourceState::Consumed,
                method: "query".into(),
                kind: ViolationKind::UseAfterClose,
            },
            ViolationRule {
                state: ResourceState::Consumed,
                method: "execute".into(),
                kind: ViolationKind::UseAfterClose,
            },
            ViolationRule {
                state: ResourceState::Consumed,
                method: "close".into(),
                kind: ViolationKind::DoubleFree,
            },
        ],
        leak_on_scope_exit: true,
    }
}

/// Returns all built-in state machines.
pub fn builtin_state_machines() -> Vec<StateMachine> {
    vec![
        file_state_machine(),
        mutex_state_machine(),
        db_connection_state_machine(),
    ]
}

// ---------------------------------------------------------------------------
// Analysis engine
// ---------------------------------------------------------------------------

/// Per-variable state at a given program point.
#[derive(Debug, Clone)]
struct VarState {
    state: ResourceState,
    machine_idx: usize,
}

/// Analyze a CPG for type-state violations using the given state machines.
///
/// Walks CFG edges, tracks variable states, and reports violations when
/// methods are called in invalid states. At merge points (nodes with
/// multiple incoming CFG edges), takes the union (worst-case) of states.
pub fn analyze_typestate(cpg: &Cpg, machines: &[StateMachine]) -> Vec<TypeStateViolation> {
    let mut violations = Vec::new();

    // Build CFG adjacency from CFG edges.
    let mut cfg_succs: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut cfg_preds: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut all_nodes: HashSet<NodeId> = HashSet::new();

    for (from, to, _kind) in cpg.edges().filter(|(_, _, k)| matches!(k, EdgeKind::Cfg)) {
        cfg_succs.entry(*from).or_default().push(*to);
        cfg_preds.entry(*to).or_default().push(*from);
        all_nodes.insert(*from);
        all_nodes.insert(*to);
    }

    // Find entry nodes (nodes with no CFG predecessors).
    let entry_nodes: Vec<NodeId> = all_nodes
        .iter()
        .filter(|n| cfg_preds.get(n).is_none_or(|p| p.is_empty()))
        .copied()
        .collect();

    // State maps: node_id -> (variable_name -> VarState)
    let mut node_states: HashMap<NodeId, HashMap<String, VarState>> = HashMap::new();

    // BFS worklist
    let mut worklist: VecDeque<NodeId> = VecDeque::new();
    for &entry in &entry_nodes {
        worklist.push_back(entry);
        node_states.insert(entry, HashMap::new());
    }

    let mut visited_count: HashMap<NodeId, u32> = HashMap::new();
    let max_iterations = all_nodes.len() as u32 * 3 + 10;

    while let Some(node_id) = worklist.pop_front() {
        let count = visited_count.entry(node_id).or_insert(0);
        *count += 1;
        if *count > max_iterations {
            continue; // Prevent infinite loops on cyclic CFGs.
        }

        // Merge predecessor states at this node.
        let mut merged_state: HashMap<String, VarState> = HashMap::new();
        if let Some(preds) = cfg_preds.get(&node_id) {
            for &pred in preds {
                if let Some(pred_state) = node_states.get(&pred) {
                    for (var, vs) in pred_state {
                        merged_state
                            .entry(var.clone())
                            .and_modify(|existing| {
                                // At merge points, take worst-case state.
                                existing.state = merge_states(existing.state, vs.state);
                            })
                            .or_insert_with(|| vs.clone());
                    }
                }
            }
        } else if let Some(existing) = node_states.get(&node_id) {
            merged_state = existing.clone();
        }

        // Process this node.
        if let Some(kind) = cpg.node(node_id) {
            process_node(
                kind,
                node_id,
                &mut merged_state,
                machines,
                &mut violations,
                cpg,
            );
        }

        // Check if state changed; if so, propagate to successors.
        let state_changed = node_states
            .get(&node_id)
            .is_none_or(|old| *old != merged_state);

        if state_changed {
            node_states.insert(node_id, merged_state);
            if let Some(succs) = cfg_succs.get(&node_id) {
                for &succ in succs {
                    worklist.push_back(succ);
                }
            }
        }
    }

    // Check for resource leaks at exit nodes (nodes with no CFG successors).
    let exit_nodes: Vec<NodeId> = all_nodes
        .iter()
        .filter(|n| cfg_succs.get(n).is_none_or(|s| s.is_empty()))
        .copied()
        .collect();

    for &exit in &exit_nodes {
        if let Some(state_map) = node_states.get(&exit) {
            let line = cpg
                .node(exit)
                .and_then(node_line)
                .unwrap_or(0);
            for (var, vs) in state_map {
                let machine = &machines[vs.machine_idx];
                if machine.leak_on_scope_exit
                    && matches!(vs.state, ResourceState::Created | ResourceState::Active)
                {
                    violations.push(TypeStateViolation {
                        variable: var.clone(),
                        kind: ViolationKind::ResourceLeak,
                        line,
                        state_at_violation: vs.state,
                        method: "<end-of-scope>".into(),
                        machine_name: machine.name.clone(),
                    });
                }
            }
        }
    }

    violations
}

/// Merge two states at a control-flow join point.
/// Takes the "worst case" — if either path has the resource consumed, we
/// consider it potentially consumed (to catch use-after-close on one branch).
fn merge_states(a: ResourceState, b: ResourceState) -> ResourceState {
    if a == b {
        return a;
    }
    // If either path consumed the resource, it may be consumed.
    if a == ResourceState::Consumed || b == ResourceState::Consumed {
        return ResourceState::Consumed;
    }
    // If either is in error state, propagate error.
    if a == ResourceState::Error || b == ResourceState::Error {
        return ResourceState::Error;
    }
    // Active + Created → Active (more progressed state)
    if (a == ResourceState::Active && b == ResourceState::Created)
        || (a == ResourceState::Created && b == ResourceState::Active)
    {
        return ResourceState::Active;
    }
    // Unknown + anything → the known state
    if a == ResourceState::Unknown {
        return b;
    }
    if b == ResourceState::Unknown {
        return a;
    }
    a
}

/// Process a single CPG node, updating variable states and recording violations.
fn process_node(
    kind: &NodeKind,
    _node_id: NodeId,
    state_map: &mut HashMap<String, VarState>,
    machines: &[StateMachine],
    violations: &mut Vec<TypeStateViolation>,
    cpg: &Cpg,
) {
    match kind {
        NodeKind::Assignment { lhs, line } => {
            // Check if the RHS is a resource creation call.
            // Look at argument edges from this assignment to find calls.
            let rhs_call = find_rhs_call(cpg, _node_id);
            if let Some(call_name) = rhs_call {
                for (idx, machine) in machines.iter().enumerate() {
                    if machine
                        .create_patterns
                        .iter()
                        .any(|p| call_name.contains(p.as_str()))
                    {
                        state_map.insert(
                            lhs.clone(),
                            VarState {
                                state: ResourceState::Created,
                                machine_idx: idx,
                            },
                        );
                        break;
                    }
                }
            }
            let _ = line;
        }
        NodeKind::Call { name, line } => {
            // Check if this call is a method on a tracked variable.
            // Convention: "var.method" or just "method" for known patterns.
            if let Some((var_name, method_name)) = split_method_call(name) {
                if let Some(vs) = state_map.get(&var_name).cloned() {
                    let machine = &machines[vs.machine_idx];

                    // Check for violations first.
                    let mut found_violation = false;
                    for rule in &machine.violations {
                        if vs.state == rule.state && method_name == rule.method {
                            violations.push(TypeStateViolation {
                                variable: var_name.clone(),
                                kind: rule.kind.clone(),
                                line: *line,
                                state_at_violation: vs.state,
                                method: method_name.clone(),
                                machine_name: machine.name.clone(),
                            });
                            found_violation = true;
                            break;
                        }
                    }

                    // Apply transition if no violation or even after violation.
                    if !found_violation {
                        for transition in &machine.transitions {
                            if vs.state == transition.from && method_name == transition.method {
                                state_map.insert(
                                    var_name.clone(),
                                    VarState {
                                        state: transition.to,
                                        machine_idx: vs.machine_idx,
                                    },
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // Also check if this is a creation call assigned via method pattern
            // (handles `x = open(...)` where the call node itself contains "open").
            for (idx, machine) in machines.iter().enumerate() {
                if machine
                    .create_patterns
                    .iter()
                    .any(|p| name.contains(p.as_str()))
                {
                    // Find what variable this is assigned to via AST edges.
                    if let Some(var_name) = find_assignment_target(cpg, _node_id) {
                        state_map.insert(
                            var_name,
                            VarState {
                                state: ResourceState::Created,
                                machine_idx: idx,
                            },
                        );
                    }
                    break;
                }
            }
        }
        _ => {}
    }
}

/// Split a call name like "f.read" into ("f", "read").
fn split_method_call(name: &str) -> Option<(String, String)> {
    let dot_pos = name.rfind('.')?;
    if dot_pos == 0 || dot_pos == name.len() - 1 {
        return None;
    }
    Some((name[..dot_pos].into(), name[dot_pos + 1..].into()))
}

/// Find the RHS call name for an assignment node by traversing AST edges.
fn find_rhs_call(cpg: &Cpg, assignment_node: NodeId) -> Option<String> {
    for edge in cpg.edges_from(assignment_node) {
        if matches!(edge.2, EdgeKind::Ast | EdgeKind::Argument { .. }) {
            if let Some(NodeKind::Call { name, .. }) = cpg.node(edge.1) {
                return Some(name.clone());
            }
        }
    }
    None
}

/// Find the assignment target (LHS variable) for a call node by looking at
/// reverse AST edges.
fn find_assignment_target(cpg: &Cpg, call_node: NodeId) -> Option<String> {
    for edge in cpg.edges_to(call_node) {
        if matches!(edge.2, EdgeKind::Ast | EdgeKind::Argument { .. }) {
            if let Some(NodeKind::Assignment { lhs, .. }) = cpg.node(edge.0) {
                return Some(lhs.clone());
            }
        }
    }
    None
}

/// Extract line from a node kind.
fn node_line(kind: &NodeKind) -> Option<u32> {
    match kind {
        NodeKind::Call { line, .. }
        | NodeKind::Identifier { line, .. }
        | NodeKind::Literal { line, .. }
        | NodeKind::Return { line }
        | NodeKind::ControlStructure { line, .. }
        | NodeKind::Assignment { line, .. }
        | NodeKind::Method { line, .. } => Some(*line),
        NodeKind::Parameter { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// Source-level analysis (pattern-based, no CPG required)
// ---------------------------------------------------------------------------

/// A source-level type-state violation found by pattern matching.
/// Used when no CPG is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTypeStateViolation {
    pub variable: String,
    pub kind: ViolationKind,
    pub line: u32,
    pub machine_name: String,
    pub message: String,
}

/// Analyze source code for type-state violations using pattern matching.
///
/// This is the primary entry point for the detector — it works on raw source
/// text and does not require a pre-built CPG.
pub fn analyze_source(source: &str, machines: &[StateMachine]) -> Vec<SourceTypeStateViolation> {
    let mut violations = Vec::new();
    // variable_name -> (state, machine_idx)
    let mut tracked: HashMap<String, (ResourceState, usize)> = HashMap::new();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let line_num = (line_idx + 1) as u32;

        // Skip comments and empty lines.
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
        {
            continue;
        }

        // Check for context manager patterns (Python `with ... as var:`)
        // which handle cleanup automatically — skip tracking.
        if trimmed.starts_with("with ") {
            continue;
        }

        // Check for resource creation: `var = create_pattern(...)`
        for (idx, machine) in machines.iter().enumerate() {
            for pattern in &machine.create_patterns {
                if let Some(var) = extract_assignment_to_create(trimmed, pattern) {
                    tracked.insert(var, (ResourceState::Created, idx));
                }
            }
        }

        // Check for method calls on tracked variables: `var.method(...)`
        for (var, (state, machine_idx)) in tracked.clone() {
            let machine = &machines[machine_idx];

            // Check all method calls on this variable.
            let method_call_prefix = format!("{}.", var);
            if !trimmed.contains(&method_call_prefix) {
                continue;
            }

            // Extract method name from `var.method(` pattern.
            if let Some(method) = extract_method_call(trimmed, &var) {
                // Check violations first.
                let mut found_violation = false;
                for rule in &machine.violations {
                    if state == rule.state && method == rule.method {
                        violations.push(SourceTypeStateViolation {
                            variable: var.clone(),
                            kind: rule.kind.clone(),
                            line: line_num,
                            machine_name: machine.name.clone(),
                            message: format!(
                                "'{}' is {} but {} was called ({})",
                                var, state, method, rule.kind
                            ),
                        });
                        found_violation = true;
                        break;
                    }
                }

                // Apply transition.
                if !found_violation {
                    for transition in &machine.transitions {
                        if state == transition.from && method == transition.method {
                            tracked.insert(var.clone(), (transition.to, machine_idx));
                            break;
                        }
                    }
                }
            }
        }
    }

    // Check for leaks at end of scope.
    for (var, (state, machine_idx)) in &tracked {
        let machine = &machines[*machine_idx];
        if machine.leak_on_scope_exit
            && matches!(state, ResourceState::Created | ResourceState::Active)
        {
            violations.push(SourceTypeStateViolation {
                variable: var.clone(),
                kind: ViolationKind::ResourceLeak,
                line: 0, // End of source — no specific line.
                machine_name: machine.name.clone(),
                message: format!(
                    "'{}' ({}) is {} at end of scope — possible resource leak",
                    var, machine.name, state
                ),
            });
        }
    }

    violations
}

/// Extract variable name from patterns like `var = open(...)`, `var = File.open(...)`.
fn extract_assignment_to_create(line: &str, create_pattern: &str) -> Option<String> {
    // Match `identifier = ...create_pattern(...`
    let eq_pos = line.find('=')?;
    // Make sure it's not `==`, `!=`, `<=`, `>=`
    if eq_pos == 0 {
        return None;
    }
    let before_eq = line[..eq_pos].trim_end();
    let after_eq = line[eq_pos + 1..].trim_start();

    // Check it's not `==`
    if line.as_bytes().get(eq_pos + 1) == Some(&b'=')
        || line.as_bytes().get(eq_pos.wrapping_sub(1)) == Some(&b'!')
        || line.as_bytes().get(eq_pos.wrapping_sub(1)) == Some(&b'<')
        || line.as_bytes().get(eq_pos.wrapping_sub(1)) == Some(&b'>')
    {
        return None;
    }

    if !after_eq.contains(create_pattern) {
        return None;
    }

    // Extract variable name — last identifier token before `=`.
    let var = before_eq
        .split_whitespace()
        .last()?
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');

    if var.is_empty() || var.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    Some(var.to_string())
}

/// Extract method name from `var.method(` in a line.
fn extract_method_call(line: &str, var: &str) -> Option<String> {
    let prefix = format!("{}.", var);
    let start = line.find(&prefix)?;
    let after_dot = &line[start + prefix.len()..];
    let method_end = after_dot.find(|c: char| !c.is_alphanumeric() && c != '_')?;
    let method = &after_dot[..method_end];
    if method.is_empty() {
        return None;
    }
    Some(method.to_string())
}

// ---------------------------------------------------------------------------
// Equality for HashMap<String, VarState> used in fixpoint check
// ---------------------------------------------------------------------------

impl PartialEq for VarState {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state && self.machine_idx == other.machine_idx
    }
}

impl Eq for VarState {}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- State machine construction ----

    #[test]
    fn file_machine_has_create_patterns() {
        let m = file_state_machine();
        assert!(m.create_patterns.contains(&"open".to_string()));
        assert!(m.create_patterns.contains(&"File::open".to_string()));
    }

    #[test]
    fn mutex_machine_has_lock_unlock() {
        let m = mutex_state_machine();
        assert!(m
            .transitions
            .iter()
            .any(|t| t.method == "lock" && t.from == ResourceState::Created));
        assert!(m
            .transitions
            .iter()
            .any(|t| t.method == "unlock" && t.from == ResourceState::Active));
    }

    #[test]
    fn db_machine_has_query_close() {
        let m = db_connection_state_machine();
        assert!(m
            .transitions
            .iter()
            .any(|t| t.method == "query" && t.from == ResourceState::Created));
        assert!(m
            .transitions
            .iter()
            .any(|t| t.method == "close" && t.from == ResourceState::Active));
    }

    #[test]
    fn builtin_machines_count() {
        assert_eq!(builtin_state_machines().len(), 3);
    }

    // ---- ViolationKind CWE IDs ----

    #[test]
    fn violation_kind_cwe_ids() {
        assert_eq!(ViolationKind::UseAfterClose.cwe_id(), 416);
        assert_eq!(ViolationKind::DoubleFree.cwe_id(), 675);
        assert_eq!(ViolationKind::ResourceLeak.cwe_id(), 404);
        assert_eq!(ViolationKind::DoubleAcquire.cwe_id(), 764);
    }

    // ---- split_method_call ----

    #[test]
    fn split_method_call_basic() {
        assert_eq!(
            split_method_call("f.read"),
            Some(("f".into(), "read".into()))
        );
    }

    #[test]
    fn split_method_call_nested() {
        assert_eq!(
            split_method_call("obj.method.call"),
            Some(("obj.method".into(), "call".into()))
        );
    }

    #[test]
    fn split_method_call_no_dot() {
        assert_eq!(split_method_call("read"), None);
    }

    #[test]
    fn split_method_call_trailing_dot() {
        assert_eq!(split_method_call("f."), None);
    }

    // ---- extract_assignment_to_create ----

    #[test]
    fn extract_assignment_python_open() {
        assert_eq!(
            extract_assignment_to_create("f = open('test.txt', 'r')", "open"),
            Some("f".into())
        );
    }

    #[test]
    fn extract_assignment_rust_file_open() {
        assert_eq!(
            extract_assignment_to_create("let f = File::open(path)?;", "File::open"),
            Some("f".into())
        );
    }

    #[test]
    fn extract_assignment_comparison_not_matched() {
        assert_eq!(
            extract_assignment_to_create("if x == open('test')", "open"),
            None
        );
    }

    #[test]
    fn extract_assignment_no_equals() {
        assert_eq!(
            extract_assignment_to_create("open('test.txt')", "open"),
            None
        );
    }

    // ---- extract_method_call ----

    #[test]
    fn extract_method_basic() {
        assert_eq!(
            extract_method_call("f.read(1024)", "f"),
            Some("read".into())
        );
    }

    #[test]
    fn extract_method_close() {
        assert_eq!(extract_method_call("f.close()", "f"), Some("close".into()));
    }

    #[test]
    fn extract_method_no_match() {
        assert_eq!(extract_method_call("g.close()", "f"), None);
    }

    // ---- Source-level analysis: Python file handling ----

    #[test]
    fn source_python_file_use_after_close() {
        let src = r#"
f = open('data.txt', 'r')
data = f.read()
f.close()
f.read()
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations.iter().any(|v| v.kind == ViolationKind::UseAfterClose
                && v.variable == "f"
                && v.line == 5),
            "expected use-after-close on line 5, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_python_file_double_close() {
        let src = r#"
f = open('data.txt', 'r')
f.close()
f.close()
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::DoubleFree && v.variable == "f"),
            "expected double-close, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_python_file_resource_leak() {
        let src = r#"
f = open('data.txt', 'r')
data = f.read()
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ResourceLeak && v.variable == "f"),
            "expected resource leak, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_python_file_with_statement_no_violation() {
        let src = r#"
with open('data.txt', 'r') as f:
    data = f.read()
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations.is_empty(),
            "expected no violations with context manager, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_python_file_proper_close() {
        let src = r#"
f = open('data.txt', 'r')
data = f.read()
f.close()
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations.is_empty(),
            "expected no violations with proper close, got: {:?}",
            violations
        );
    }

    // ---- Source-level analysis: Mutex ----

    #[test]
    fn source_mutex_double_lock() {
        let src = r#"
lock = Lock()
lock.acquire()
lock.acquire()
"#;
        let machines = vec![mutex_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::DoubleAcquire && v.variable == "lock"),
            "expected double-acquire, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_mutex_leak_no_release() {
        let src = r#"
lock = Lock()
lock.acquire()
"#;
        let machines = vec![mutex_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ResourceLeak && v.variable == "lock"),
            "expected resource leak (unreleased lock), got: {:?}",
            violations
        );
    }

    #[test]
    fn source_mutex_proper_usage() {
        let src = r#"
lock = Lock()
lock.acquire()
lock.release()
"#;
        let machines = vec![mutex_state_machine()];
        let violations = analyze_source(src, &machines);
        // After release, state is Consumed — no leak.
        assert!(
            violations.is_empty(),
            "expected no violations, got: {:?}",
            violations
        );
    }

    // ---- Source-level analysis: DB Connection ----

    #[test]
    fn source_db_use_after_close() {
        let src = r#"
conn = sqlite3.connect('test.db')
conn.execute('SELECT 1')
conn.close()
conn.execute('SELECT 2')
"#;
        let machines = vec![db_connection_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations.iter().any(|v| v.kind == ViolationKind::UseAfterClose
                && v.variable == "conn"
                && v.line == 5),
            "expected use-after-close, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_db_connection_leak() {
        let src = r#"
conn = psycopg2.connect(dsn)
conn.execute('SELECT 1')
"#;
        let machines = vec![db_connection_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ResourceLeak && v.variable == "conn"),
            "expected resource leak, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_db_double_close() {
        let src = r#"
conn = sqlite3.connect('test.db')
conn.close()
conn.close()
"#;
        let machines = vec![db_connection_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::DoubleFree && v.variable == "conn"),
            "expected double-close, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_db_proper_usage() {
        let src = r#"
conn = sqlite3.connect('test.db')
conn.execute('CREATE TABLE t(id INT)')
conn.execute('INSERT INTO t VALUES(1)')
conn.commit()
conn.close()
"#;
        let machines = vec![db_connection_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations.is_empty(),
            "expected no violations, got: {:?}",
            violations
        );
    }

    // ---- CPG-level analysis ----

    fn build_file_open_close_cpg() -> Cpg {
        let mut cpg = Cpg::new();
        // open() call
        let open_call = cpg.add_node(NodeKind::Call {
            name: "open".into(),
            line: 1,
        });
        // assignment: f = open(...)
        let assign = cpg.add_node(NodeKind::Assignment {
            lhs: "f".into(),
            line: 1,
        });
        cpg.add_edge(assign, open_call, EdgeKind::Ast);
        // f.read()
        let read_call = cpg.add_node(NodeKind::Call {
            name: "f.read".into(),
            line: 2,
        });
        // f.close()
        let close_call = cpg.add_node(NodeKind::Call {
            name: "f.close".into(),
            line: 3,
        });

        // CFG edges
        cpg.add_edge(assign, read_call, EdgeKind::Cfg);
        cpg.add_edge(read_call, close_call, EdgeKind::Cfg);

        cpg
    }

    #[test]
    fn cpg_file_proper_usage_no_violations() {
        let cpg = build_file_open_close_cpg();
        let machines = builtin_state_machines();
        let violations = analyze_typestate(&cpg, &machines);
        assert!(
            violations.is_empty(),
            "expected no violations for open-read-close, got: {:?}",
            violations
        );
    }

    #[test]
    fn cpg_file_use_after_close() {
        let mut cpg = build_file_open_close_cpg();
        // Add f.read() after f.close()
        let read_after = cpg.add_node(NodeKind::Call {
            name: "f.read".into(),
            line: 4,
        });
        // Node 3 is f.close() at id=3
        cpg.add_edge(3, read_after, EdgeKind::Cfg);

        let machines = builtin_state_machines();
        let violations = analyze_typestate(&cpg, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::UseAfterClose && v.variable == "f"),
            "expected use-after-close, got: {:?}",
            violations
        );
    }

    #[test]
    fn cpg_file_resource_leak() {
        let mut cpg = Cpg::new();
        let open_call = cpg.add_node(NodeKind::Call {
            name: "open".into(),
            line: 1,
        });
        let assign = cpg.add_node(NodeKind::Assignment {
            lhs: "f".into(),
            line: 1,
        });
        cpg.add_edge(assign, open_call, EdgeKind::Ast);
        let read_call = cpg.add_node(NodeKind::Call {
            name: "f.read".into(),
            line: 2,
        });
        cpg.add_edge(assign, read_call, EdgeKind::Cfg);
        // No close! -> leak

        let machines = builtin_state_machines();
        let violations = analyze_typestate(&cpg, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ResourceLeak && v.variable == "f"),
            "expected resource leak, got: {:?}",
            violations
        );
    }

    // ---- merge_states ----

    #[test]
    fn merge_same_state() {
        assert_eq!(
            merge_states(ResourceState::Active, ResourceState::Active),
            ResourceState::Active
        );
    }

    #[test]
    fn merge_consumed_wins() {
        assert_eq!(
            merge_states(ResourceState::Active, ResourceState::Consumed),
            ResourceState::Consumed
        );
        assert_eq!(
            merge_states(ResourceState::Consumed, ResourceState::Created),
            ResourceState::Consumed
        );
    }

    #[test]
    fn merge_error_propagates() {
        assert_eq!(
            merge_states(ResourceState::Active, ResourceState::Error),
            ResourceState::Error
        );
    }

    #[test]
    fn merge_active_created() {
        assert_eq!(
            merge_states(ResourceState::Active, ResourceState::Created),
            ResourceState::Active
        );
    }

    #[test]
    fn merge_unknown_yields_known() {
        assert_eq!(
            merge_states(ResourceState::Unknown, ResourceState::Active),
            ResourceState::Active
        );
        assert_eq!(
            merge_states(ResourceState::Created, ResourceState::Unknown),
            ResourceState::Created
        );
    }

    // ---- ResourceState / ViolationKind Display ----

    #[test]
    fn resource_state_display() {
        assert_eq!(format!("{}", ResourceState::Created), "Created");
        assert_eq!(format!("{}", ResourceState::Consumed), "Consumed");
    }

    #[test]
    fn violation_kind_display() {
        assert_eq!(format!("{}", ViolationKind::UseAfterClose), "use-after-close");
        assert_eq!(
            format!("{}", ViolationKind::DoubleFree),
            "double-free/double-close"
        );
        assert_eq!(format!("{}", ViolationKind::ResourceLeak), "resource-leak");
        assert_eq!(format!("{}", ViolationKind::DoubleAcquire), "double-acquire");
    }

    // ---- Multiple variables tracked simultaneously ----

    #[test]
    fn source_multiple_files_tracked() {
        let src = r#"
f1 = open('a.txt')
f2 = open('b.txt')
f1.read()
f1.close()
f2.read()
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        // f2 is never closed → leak
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ResourceLeak && v.variable == "f2"),
            "expected leak for f2, got: {:?}",
            violations
        );
        // f1 is properly closed — no leak for f1
        assert!(
            !violations
                .iter()
                .any(|v| v.variable == "f1" && v.kind == ViolationKind::ResourceLeak),
            "f1 should not leak, got: {:?}",
            violations
        );
    }

    // ---- Rust-style patterns ----

    #[test]
    fn source_rust_file_open_read_close() {
        let src = r#"
let f = File::open("data.txt").unwrap();
let data = f.read();
f.close();
"#;
        let machines = vec![file_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations.is_empty(),
            "expected no violations, got: {:?}",
            violations
        );
    }

    #[test]
    fn source_rust_mutex_double_lock() {
        let src = r#"
let m = Mutex::new(0);
m.lock();
m.lock();
"#;
        let machines = vec![mutex_state_machine()];
        let violations = analyze_source(src, &machines);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::DoubleAcquire && v.variable == "m"),
            "expected double-acquire for mutex, got: {:?}",
            violations
        );
    }

    // ---- Edge cases ----

    #[test]
    fn source_empty_input() {
        let violations = analyze_source("", &builtin_state_machines());
        assert!(violations.is_empty());
    }

    #[test]
    fn source_no_tracked_resources() {
        let src = r#"
x = 42
y = x + 1
print(y)
"#;
        let violations = analyze_source(src, &builtin_state_machines());
        assert!(violations.is_empty());
    }

    #[test]
    fn source_comment_lines_ignored() {
        let src = r#"
# f = open('data.txt')
// f.read()
"#;
        let violations = analyze_source(src, &builtin_state_machines());
        assert!(violations.is_empty());
    }
}
