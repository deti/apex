use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;

static RE_FN_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").unwrap()
});

static RE_IMPL_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"impl(?:<[^>]*>)?\s+(?:(\w+)\s+for\s+)?(\w+)").unwrap()
});

static RE_CALL_SIMPLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\w+)\s*\(").unwrap()
});

static RE_CALL_SELF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"self\.(\w+)\s*\(").unwrap()
});

static RE_CALL_QUALIFIED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\w+)::(\w+)\s*\(").unwrap()
});

static RE_TEST_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[(?:tokio::)?test").unwrap()
});

static RE_HTTP_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[(get|post|put|delete)\(").unwrap()
});

static RE_BLOCK_KEYWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(if|else|for|while|match|loop)\b").unwrap()
});

/// Keywords that should not be treated as function calls.
const RUST_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "match", "loop", "return", "let", "mut", "const", "static",
    "struct", "enum", "trait", "impl", "fn", "pub", "use", "mod", "crate", "self", "super",
    "where", "type", "as", "in", "ref", "move", "async", "await", "unsafe", "extern", "dyn",
    "box", "yield", "macro_rules", "cfg", "derive", "allow", "deny", "warn",
];

pub struct RustExtractor;

/// Intermediate representation of a function found during the first pass.
struct FnInfo {
    #[allow(dead_code)]
    name: String,
    qualified_name: String,
    file: PathBuf,
    start_line: u32,
    end_line: u32,
    entry_kind: Option<EntryPointKind>,
    body_start_line: u32,
}

impl CallGraphExtractor for RustExtractor {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        if sources.is_empty() {
            return graph;
        }

        let mut next_id: u32 = 0;
        let mut all_fns: Vec<(FnId, FnInfo)> = Vec::new();

        // Check if any file contains clap indicators (file-level flag).
        let clap_files: HashMap<&PathBuf, bool> = sources
            .iter()
            .map(|(path, src)| {
                let has_clap =
                    src.contains("clap::Parser") || src.contains("#[derive(Parser)]");
                (path, has_clap)
            })
            .collect();

        // --- Pass 1: Extract functions ---
        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();
            let has_clap = clap_files.get(path).copied().unwrap_or(false);
            let fns = extract_functions(&lines, path, has_clap);
            for info in fns {
                let id = FnId(next_id);
                next_id += 1;
                graph.nodes.push(FnNode {
                    id,
                    name: info.qualified_name.clone(),
                    file: info.file.clone(),
                    start_line: info.start_line,
                    end_line: info.end_line,
                    entry_kind: info.entry_kind,
                });
                all_fns.push((id, info));
            }
        }

        // Build name index for callee resolution.
        graph.build_indices();

        // --- Pass 2: Extract calls ---
        for (caller_id, info) in &all_fns {
            let source = match sources.get(&info.file) {
                Some(s) => s,
                None => continue,
            };
            let lines: Vec<&str> = source.lines().collect();
            let body_start = info.body_start_line as usize;
            let body_end = info.end_line as usize;

            if body_start == 0 || body_end == 0 || body_start > lines.len() {
                continue;
            }

            let mut block_id: u32 = 0;

            for (line_idx, line) in lines
                .iter()
                .enumerate()
                .take(body_end.min(lines.len()))
                .skip(body_start - 1)
            {
                let line_num = (line_idx + 1) as u32;

                // Track block boundaries.
                if RE_BLOCK_KEYWORD.is_match(line) {
                    block_id += 1;
                }

                let callees = extract_calls_from_line(line);
                for callee_name in callees {
                    // Resolve callee against the name index.
                    let resolved = resolve_callee(&callee_name, &graph);
                    for callee_id in resolved {
                        // Don't create self-edges.
                        if callee_id == *caller_id {
                            continue;
                        }
                        graph.edges.push(CallEdge {
                            caller: *caller_id,
                            callee: callee_id,
                            call_site_line: line_num,
                            call_site_block: if block_id > 0 {
                                Some(block_id)
                            } else {
                                None
                            },
                        });
                    }
                }
            }
        }

        // Rebuild indices with edges included.
        graph.build_indices();
        graph
    }
}

/// State for a function currently being tracked (its opening brace was found).
struct OpenFn {
    info: FnInfo,
    /// Brace depth at which the function's opening `{` was counted.
    /// The function ends when brace_depth drops back to `open_depth - 1`.
    open_depth: i32,
}

/// Extract all function definitions from a single file's lines.
///
/// Uses a single-pass approach with a stack of open functions to handle
/// nested functions correctly.
fn extract_functions(lines: &[&str], path: &Path, has_clap: bool) -> Vec<FnInfo> {
    let mut result = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut current_impl_type: Option<String> = None;
    let mut impl_brace_depth: Option<i32> = None;

    // Stack of functions whose opening brace was found but closing brace not yet.
    let mut fn_stack: Vec<OpenFn> = Vec::new();

    // Pending state: fn definition seen but opening brace not yet found.
    let mut pending_fn: Option<(FnInfo, i32)> = None; // (info, brace_depth_before_open)

    // Track pending attributes from preceding lines.
    let mut pending_test = false;
    let mut pending_http = false;

    for (i, &line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let line_num = (i + 1) as u32;

        // Track attributes on preceding lines.
        if RE_TEST_ATTR.is_match(trimmed) {
            pending_test = true;
        }
        if RE_HTTP_ATTR.is_match(trimmed) {
            pending_http = true;
        }

        // Skip comment-only lines for brace counting.
        let is_comment = trimmed.starts_with("//");

        // Detect impl blocks (only on lines without fn definitions).
        if !is_comment
            && (trimmed.starts_with("impl") || trimmed.starts_with("pub impl"))
            && !trimmed.contains("fn ")
        {
            if let Some(caps) = RE_IMPL_BLOCK.captures(trimmed) {
                let type_name = caps.get(2).map(|m| m.as_str().to_string());
                current_impl_type = type_name;
                impl_brace_depth = Some(brace_depth);
                // Don't skip -- fall through to brace counting below.
            }
        }

        // Detect fn definitions (but not on comment lines).
        let mut fn_detected = false;
        if !is_comment {
            if let Some(caps) = RE_FN_DEF.captures(trimmed) {
                let fn_name = caps[1].to_string();
                let is_pub = trimmed.contains("pub ");

                let qualified_name = if let Some(ref impl_type) = current_impl_type {
                    format!("{}::{}", impl_type, fn_name)
                } else {
                    fn_name.clone()
                };

                let entry_kind = classify_entry_point(
                    &fn_name,
                    is_pub,
                    brace_depth,
                    pending_test,
                    pending_http,
                    has_clap,
                );

                pending_test = false;
                pending_http = false;
                fn_detected = true;

                // Check if this is a bodyless declaration (ends with `;`, no `{`).
                if trimmed.ends_with(';') && !trimmed.contains('{') {
                    // Trait method declaration or extern fn -- skip.
                } else {
                    // This fn might have a body. Set pending_fn; the opening brace
                    // will be detected during brace counting on this or a later line.
                    pending_fn = Some((
                        FnInfo {
                            name: fn_name,
                            qualified_name,
                            file: path.to_path_buf(),
                            start_line: line_num,
                            end_line: line_num, // updated when closing brace found
                            entry_kind,
                            body_start_line: line_num, // updated when opening brace found
                        },
                        brace_depth,
                    ));
                }
            }
        }

        // Count braces on this line.
        if !is_comment {
            for ch in line.chars() {
                match ch {
                    '{' => {
                        brace_depth += 1;

                        // If there's a pending fn, this is its opening brace.
                        if let Some((mut info, _pre_depth)) = pending_fn.take() {
                            info.body_start_line = line_num;
                            fn_stack.push(OpenFn {
                                info,
                                open_depth: brace_depth,
                            });
                        }
                    }
                    '}' => {
                        brace_depth -= 1;

                        // Check if any open function closes at this depth.
                        if let Some(top) = fn_stack.last() {
                            if brace_depth < top.open_depth {
                                let mut closed = fn_stack.pop().unwrap();
                                closed.info.end_line = line_num;
                                result.push(closed.info);
                            }
                        }

                        // Check if impl block ended.
                        if let Some(impl_depth) = impl_brace_depth {
                            if brace_depth <= impl_depth {
                                current_impl_type = None;
                                impl_brace_depth = None;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // If pending_fn is still set and the line ended with `;` (multi-line bodyless fn).
        if let Some((_, _)) = &pending_fn {
            if trimmed.ends_with(';') {
                pending_fn = None; // bodyless
            }
        }

        // Reset pending attributes if this line is not an attribute, not blank, not a comment,
        // and we didn't just detect a fn (the fn handler already cleared them).
        if !fn_detected
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("//")
        {
            pending_test = false;
            pending_http = false;
        }
    }

    // Any functions still on the stack (unclosed braces) -- close them at EOF.
    while let Some(mut open) = fn_stack.pop() {
        open.info.end_line = lines.len() as u32;
        result.push(open.info);
    }

    result
}

/// Classify a function's entry point kind based on context.
fn classify_entry_point(
    fn_name: &str,
    is_pub: bool,
    brace_depth: i32,
    is_test: bool,
    is_http_handler: bool,
    has_clap: bool,
) -> Option<EntryPointKind> {
    if is_test {
        return Some(EntryPointKind::Test);
    }
    if fn_name == "main" && has_clap {
        return Some(EntryPointKind::CliEntry);
    }
    if fn_name == "main" {
        return Some(EntryPointKind::Main);
    }
    if is_http_handler {
        return Some(EntryPointKind::HttpHandler);
    }
    // pub fn at crate root (brace depth 0).
    if is_pub && brace_depth == 0 {
        return Some(EntryPointKind::PublicApi);
    }
    None
}

/// Extract callee names from a single line of code.
fn extract_calls_from_line(line: &str) -> Vec<String> {
    let trimmed = line.trim();

    // Skip comments and attributes.
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return Vec::new();
    }

    // Skip lines that are just use/mod statements.
    if trimmed.starts_with("use ") || trimmed.starts_with("mod ") {
        return Vec::new();
    }

    let mut callees = Vec::new();

    // self.method() calls.
    for caps in RE_CALL_SELF.captures_iter(line) {
        let method = caps[1].to_string();
        if !is_keyword(&method) {
            callees.push(method);
        }
    }

    // Qualified::method() calls.
    for caps in RE_CALL_QUALIFIED.captures_iter(line) {
        let module = &caps[1];
        let func = caps[2].to_string();
        if !is_keyword(&func) && !is_keyword(module) {
            // Add both qualified and unqualified forms for resolution.
            callees.push(format!("{}::{}", module, func));
            callees.push(func);
        }
    }

    // Simple function() calls.
    for caps in RE_CALL_SIMPLE.captures_iter(line) {
        let name = caps[1].to_string();
        if !is_keyword(&name) && !callees.contains(&name) {
            callees.push(name);
        }
    }

    callees
}

/// Check if a name is a Rust keyword (should not be treated as a call).
fn is_keyword(name: &str) -> bool {
    RUST_KEYWORDS.contains(&name)
}

/// Resolve a callee name against the graph's by_name index.
/// Returns all matching FnIds.
fn resolve_callee(name: &str, graph: &CallGraph) -> Vec<FnId> {
    // Try exact match first.
    let exact = graph.fns_named(name);
    if !exact.is_empty() {
        return exact.to_vec();
    }

    // For qualified names like "Type::method", try just the method name.
    if let Some((_prefix, method)) = name.split_once("::") {
        let by_method = graph.fns_named(method);
        if !by_method.is_empty() {
            return by_method.to_vec();
        }
    }

    // For unqualified names, check if any qualified name ends with "::name".
    let suffix = format!("::{}", name);
    let mut matches = Vec::new();
    for (qualified_name, ids) in &graph.by_name {
        if qualified_name.ends_with(&suffix) {
            matches.extend(ids);
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sources(files: Vec<(&str, &str)>) -> HashMap<PathBuf, String> {
        files
            .into_iter()
            .map(|(path, src)| (PathBuf::from(path), src.to_string()))
            .collect()
    }

    #[test]
    fn simple_function_and_call() {
        let src = r#"
fn foo() {
    bar();
}

fn bar() {
    println!("hello");
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        assert_eq!(graph.node_count(), 2);
        let foo_ids = graph.fns_named("foo");
        assert_eq!(foo_ids.len(), 1);
        let bar_ids = graph.fns_named("bar");
        assert_eq!(bar_ids.len(), 1);

        // foo calls bar.
        let foo_edges = graph.callees_of.get(&foo_ids[0]);
        assert!(foo_edges.is_some());
        let edge_idx = foo_edges.unwrap()[0];
        assert_eq!(graph.edges[edge_idx].callee, bar_ids[0]);
    }

    #[test]
    fn impl_block_method_detection() {
        let src = r#"
struct MyStruct;

impl MyStruct {
    fn new() -> Self {
        MyStruct
    }

    fn process(&self) {
        self.helper();
    }

    fn helper(&self) {
    }
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        // Should have 3 methods: MyStruct::new, MyStruct::process, MyStruct::helper.
        assert_eq!(graph.node_count(), 3);

        let new_ids = graph.fns_named("MyStruct::new");
        assert_eq!(new_ids.len(), 1, "MyStruct::new not found");

        let process_ids = graph.fns_named("MyStruct::process");
        assert_eq!(process_ids.len(), 1, "MyStruct::process not found");

        let helper_ids = graph.fns_named("MyStruct::helper");
        assert_eq!(helper_ids.len(), 1, "MyStruct::helper not found");

        // process calls helper via self.helper().
        let process_edges = graph.callees_of.get(&process_ids[0]);
        assert!(process_edges.is_some(), "process should have outgoing edges");
        let has_helper_edge = process_edges.unwrap().iter().any(|&idx| {
            graph.edges[idx].callee == helper_ids[0]
        });
        assert!(has_helper_edge, "process should call helper");
    }

    #[test]
    fn async_fn_detection() {
        let src = r#"
async fn fetch_data() {
    process().await;
}

async fn process() {
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        assert_eq!(graph.node_count(), 2);
        let fetch_ids = graph.fns_named("fetch_data");
        assert_eq!(fetch_ids.len(), 1);
        let process_ids = graph.fns_named("process");
        assert_eq!(process_ids.len(), 1);

        // fetch_data calls process.
        let edges = graph.callees_of.get(&fetch_ids[0]);
        assert!(edges.is_some());
        let has_process_call = edges.unwrap().iter().any(|&idx| {
            graph.edges[idx].callee == process_ids[0]
        });
        assert!(has_process_call);
    }

    #[test]
    fn entry_point_test_attribute() {
        let src = r#"
#[test]
fn test_something() {
    assert!(true);
}

#[tokio::test]
async fn test_async() {
    assert!(true);
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        assert_eq!(graph.node_count(), 2);
        for node in &graph.nodes {
            assert_eq!(
                node.entry_kind,
                Some(EntryPointKind::Test),
                "{} should be Test entry point",
                node.name
            );
        }
    }

    #[test]
    fn entry_point_main() {
        let src = r#"
fn main() {
    run();
}

fn run() {
}
"#;
        let sources = make_sources(vec![("src/main.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        let main_ids = graph.fns_named("main");
        assert_eq!(main_ids.len(), 1);
        let main_node = graph.node(main_ids[0]).unwrap();
        assert_eq!(main_node.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn entry_point_http_handler() {
        let src = r#"
#[get("/users")]
async fn list_users() {
}

#[post("/users")]
async fn create_user() {
}
"#;
        let sources = make_sources(vec![("src/api.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        assert_eq!(graph.node_count(), 2);
        for node in &graph.nodes {
            assert_eq!(
                node.entry_kind,
                Some(EntryPointKind::HttpHandler),
                "{} should be HttpHandler",
                node.name
            );
        }
    }

    #[test]
    fn entry_point_public_api() {
        let src = r#"
pub fn public_function() {
    private_helper();
}

fn private_helper() {
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        let pub_ids = graph.fns_named("public_function");
        assert_eq!(pub_ids.len(), 1);
        let pub_node = graph.node(pub_ids[0]).unwrap();
        assert_eq!(pub_node.entry_kind, Some(EntryPointKind::PublicApi));

        let priv_ids = graph.fns_named("private_helper");
        assert_eq!(priv_ids.len(), 1);
        let priv_node = graph.node(priv_ids[0]).unwrap();
        assert_eq!(priv_node.entry_kind, None);
    }

    #[test]
    fn entry_point_cli_clap() {
        let src = r#"
use clap::Parser;

#[derive(Parser)]
struct Cli {
    name: String,
}

fn main() {
    let cli = Cli::parse();
}
"#;
        let sources = make_sources(vec![("src/main.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        let main_ids = graph.fns_named("main");
        assert_eq!(main_ids.len(), 1);
        let main_node = graph.node(main_ids[0]).unwrap();
        assert_eq!(
            main_node.entry_kind,
            Some(EntryPointKind::CliEntry),
            "main in clap file should be CliEntry"
        );
    }

    #[test]
    fn cross_file_call_resolution() {
        let file_a = r#"
fn caller_fn() {
    callee_fn();
}
"#;
        let file_b = r#"
pub fn callee_fn() {
    println!("called");
}
"#;
        let sources = make_sources(vec![("src/a.rs", file_a), ("src/b.rs", file_b)]);
        let graph = RustExtractor.extract(&sources);

        let caller_ids = graph.fns_named("caller_fn");
        assert_eq!(caller_ids.len(), 1);
        let callee_ids = graph.fns_named("callee_fn");
        assert_eq!(callee_ids.len(), 1);

        // caller_fn should have an edge to callee_fn.
        let edges = graph.callees_of.get(&caller_ids[0]);
        assert!(edges.is_some(), "caller_fn should have outgoing edges");
        let has_callee_edge = edges.unwrap().iter().any(|&idx| {
            graph.edges[idx].callee == callee_ids[0]
        });
        assert!(has_callee_edge, "caller_fn should call callee_fn across files");
    }

    #[test]
    fn nested_functions_are_separate_nodes() {
        let src = r#"
fn outer() {
    fn inner() {
        println!("inner");
    }
    inner();
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        // Both outer and inner should be separate nodes.
        assert!(graph.node_count() >= 2);
        let outer_ids = graph.fns_named("outer");
        assert_eq!(outer_ids.len(), 1);
        let inner_ids = graph.fns_named("inner");
        assert_eq!(inner_ids.len(), 1);

        // outer calls inner.
        let edges = graph.callees_of.get(&outer_ids[0]);
        assert!(edges.is_some());
    }

    #[test]
    fn block_id_assignment() {
        let src = r#"
fn complex() {
    setup();
    if condition {
        branch_a();
    } else {
        branch_b();
    }
    for item in items {
        loop_body();
    }
}

fn setup() {}
fn branch_a() {}
fn branch_b() {}
fn loop_body() {}
fn condition() -> bool { true }
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        let complex_ids = graph.fns_named("complex");
        assert_eq!(complex_ids.len(), 1);
        let edges = graph.callees_of.get(&complex_ids[0]);
        assert!(edges.is_some());

        // Collect all edges from complex.
        let complex_edges: Vec<_> = edges
            .unwrap()
            .iter()
            .map(|&idx| &graph.edges[idx])
            .collect();

        // setup() call should have no block (it's before any control flow).
        let setup_ids = graph.fns_named("setup");
        assert!(!setup_ids.is_empty());
        let setup_edge = complex_edges.iter().find(|e| e.callee == setup_ids[0]);
        assert!(setup_edge.is_some());
        assert_eq!(setup_edge.unwrap().call_site_block, None);

        // branch_a and branch_b should have block IDs.
        let branch_a_ids = graph.fns_named("branch_a");
        if !branch_a_ids.is_empty() {
            let branch_a_edge = complex_edges
                .iter()
                .find(|e| e.callee == branch_a_ids[0]);
            if let Some(edge) = branch_a_edge {
                assert!(
                    edge.call_site_block.is_some(),
                    "branch_a should have a block ID"
                );
            }
        }
    }

    #[test]
    fn empty_source_returns_empty_graph() {
        let sources: HashMap<PathBuf, String> = HashMap::new();
        let graph = RustExtractor.extract(&sources);
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn function_start_and_end_lines() {
        let src = "fn first() {\n    let x = 1;\n    let y = 2;\n}\n\nfn second() {\n    let a = 3;\n}\n";
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        assert_eq!(graph.node_count(), 2);

        let first_ids = graph.fns_named("first");
        let first = graph.node(first_ids[0]).unwrap();
        assert_eq!(first.start_line, 1);
        assert_eq!(first.end_line, 4);

        let second_ids = graph.fns_named("second");
        let second = graph.node(second_ids[0]).unwrap();
        assert_eq!(second.start_line, 6);
        assert_eq!(second.end_line, 8);
    }

    #[test]
    fn pub_async_fn_detection() {
        let src = r#"
pub async fn serve() {
    handle().await;
}

async fn handle() {
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        assert_eq!(graph.node_count(), 2);
        let serve_ids = graph.fns_named("serve");
        assert_eq!(serve_ids.len(), 1);
        let serve_node = graph.node(serve_ids[0]).unwrap();
        assert_eq!(serve_node.entry_kind, Some(EntryPointKind::PublicApi));
    }

    #[test]
    fn qualified_call_resolution() {
        let src = r#"
struct Db;

impl Db {
    fn query(&self) {
    }
}

fn handler() {
    let db = Db;
    Db::query(&db);
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        let handler_ids = graph.fns_named("handler");
        assert_eq!(handler_ids.len(), 1);
        let query_ids = graph.fns_named("Db::query");
        assert_eq!(query_ids.len(), 1);

        let edges = graph.callees_of.get(&handler_ids[0]);
        assert!(edges.is_some());
        let has_query = edges
            .unwrap()
            .iter()
            .any(|&idx| graph.edges[idx].callee == query_ids[0]);
        assert!(has_query, "handler should call Db::query");
    }

    #[test]
    fn trait_impl_methods() {
        let src = r#"
trait Processor {
    fn process(&self);
}

struct Worker;

impl Processor for Worker {
    fn process(&self) {
        do_work();
    }
}

fn do_work() {
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        // Worker::process should exist (impl Processor for Worker).
        let process_ids = graph.fns_named("Worker::process");
        assert_eq!(process_ids.len(), 1, "Worker::process should be found");
    }

    #[test]
    fn multiple_impl_blocks() {
        let src = r#"
struct Alpha;
struct Beta;

impl Alpha {
    fn run(&self) {
    }
}

impl Beta {
    fn run(&self) {
    }
}
"#;
        let sources = make_sources(vec![("src/lib.rs", src)]);
        let graph = RustExtractor.extract(&sources);

        let alpha_run = graph.fns_named("Alpha::run");
        assert_eq!(alpha_run.len(), 1);
        let beta_run = graph.fns_named("Beta::run");
        assert_eq!(beta_run.len(), 1);
        assert_ne!(alpha_run[0], beta_run[0]);
    }
}
