use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct JsExtractor;

/// A function definition found during extraction.
#[derive(Debug)]
struct FnDef {
    name: String,
    start_line: u32,
    end_line: u32,
}

/// A call site found during extraction.
#[derive(Debug)]
struct CallSite {
    callee_name: String,
    line: u32,
    block_id: Option<u32>,
}

impl CallGraphExtractor for JsExtractor {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        // Map: (file, fn_name) -> FnId for cross-file resolution
        let mut fn_index: HashMap<String, Vec<(PathBuf, FnId)>> = HashMap::new();
        // Deferred edges: (caller FnId, callee name, call line, block)
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let functions = extract_functions(source);
            let file_entry = detect_file_entry_points(source, path);

            // Register all functions as nodes.
            let mut local_fns: Vec<(FnId, &FnDef)> = Vec::new();
            for func in &functions {
                let id = FnId(next_id);
                next_id += 1;

                let entry_kind = classify_function(func, source, path, &file_entry);

                graph.nodes.push(FnNode {
                    id,
                    name: func.name.clone(),
                    file: path.clone(),
                    start_line: func.start_line,
                    end_line: func.end_line,
                    entry_kind,
                });

                fn_index
                    .entry(func.name.clone())
                    .or_default()
                    .push((path.clone(), id));
                local_fns.push((id, func));
            }

            // Extract calls within each function body.
            let lines: Vec<&str> = source.lines().collect();
            for &(fn_id, func) in &local_fns {
                let body_start = func.start_line as usize;
                let body_end = (func.end_line as usize).min(lines.len());
                if body_start == 0 || body_end == 0 {
                    continue;
                }
                let body_lines = &lines[(body_start - 1)..body_end.min(lines.len())];
                let calls =
                    extract_calls_in_body(body_lines, func.start_line, &func.name);
                for cs in calls {
                    pending_edges.push((fn_id, cs.callee_name, cs.line, cs.block_id));
                }
            }
        }

        // Resolve pending edges.
        for (caller_id, callee_name, line, block) in pending_edges {
            // Find matching callee — prefer same file, otherwise first match.
            let caller_file = graph
                .nodes
                .iter()
                .find(|n| n.id == caller_id)
                .map(|n| &n.file);
            if let Some(candidates) = fn_index.get(&callee_name) {
                let callee_id = if let Some(caller_f) = caller_file {
                    // Prefer same-file match.
                    candidates
                        .iter()
                        .find(|(f, _)| f == caller_f)
                        .or_else(|| candidates.first())
                        .map(|(_, id)| *id)
                } else {
                    candidates.first().map(|(_, id)| *id)
                };

                if let Some(cid) = callee_id {
                    if cid != caller_id {
                        graph.edges.push(CallEdge {
                            caller: caller_id,
                            callee: cid,
                            call_site_line: line,
                            call_site_block: block,
                        });
                    }
                }
            }
        }

        graph.build_indices();
        graph
    }
}

/// Flags detected at the file level for entry point classification.
#[derive(Debug, Default)]
struct FileEntryFlags {
    has_require_main: bool,
    is_main_file: bool,
    has_commander: bool,
    has_yargs: bool,
    has_dot_command: bool,
}

fn detect_file_entry_points(source: &str, path: &Path) -> FileEntryFlags {
    let fname = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");
    let has_require_main = source.contains("require.main === module")
        || source.contains("require.main===module");
    let is_main_file = fname == "index.js"
        || fname == "main.js"
        || fname == "index.ts"
        || fname == "main.ts";
    let has_commander = source.contains("commander") || source.contains("yargs");
    let has_yargs = source.contains("yargs");
    let has_dot_command = source.contains(".command(");

    FileEntryFlags {
        has_require_main,
        is_main_file,
        has_commander,
        has_yargs,
        has_dot_command,
    }
}

fn classify_function(
    func: &FnDef,
    source: &str,
    path: &Path,
    file_flags: &FileEntryFlags,
) -> Option<EntryPointKind> {
    let lines: Vec<&str> = source.lines().collect();
    let start_idx = (func.start_line as usize).saturating_sub(1);
    let fn_line = lines.get(start_idx).unwrap_or(&"");

    // Test patterns: describe/it/test
    if func.name == "describe" || func.name == "it" || func.name == "test" {
        return Some(EntryPointKind::Test);
    }

    // HTTP handler patterns: check the line that defines this function
    // Look at surrounding context for app.get/router.post patterns
    let name = &func.name;
    // Check if this function is registered as a route handler
    for line in &lines {
        let trimmed = line.trim();
        if (trimmed.contains("app.get(")
            || trimmed.contains("app.post(")
            || trimmed.contains("app.put(")
            || trimmed.contains("app.delete(")
            || trimmed.contains("router.get(")
            || trimmed.contains("router.post(")
            || trimmed.contains("router.put(")
            || trimmed.contains("router.delete("))
            && trimmed.contains(name)
        {
            return Some(EntryPointKind::HttpHandler);
        }
    }

    // Also check if the function definition line itself is inside an app/router call
    if fn_line.contains("app.get(")
        || fn_line.contains("app.post(")
        || fn_line.contains("router.get(")
        || fn_line.contains("router.post(")
    {
        return Some(EntryPointKind::HttpHandler);
    }

    // PublicApi: export default function, module.exports, export function
    if fn_line.contains("export default function")
        || fn_line.contains("export function")
        || fn_line.contains("export const")
        || fn_line.contains("export let")
    {
        return Some(EntryPointKind::PublicApi);
    }
    // Check if function is assigned to module.exports
    for line in &lines {
        if line.contains("module.exports") && line.contains(name) {
            return Some(EntryPointKind::PublicApi);
        }
    }

    // CliEntry: file uses commander/yargs/.command(
    if file_flags.has_commander || file_flags.has_yargs || file_flags.has_dot_command {
        // Only mark as CLI entry if the function is the main export or top-level
        let fname_str = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if fname_str == "cli.js" || fname_str == "cli.ts" || func.name == "main" {
            return Some(EntryPointKind::CliEntry);
        }
    }

    // Main: require.main === module or index.js/main.js with top-level code
    if file_flags.has_require_main && func.name == "main" {
        return Some(EntryPointKind::Main);
    }
    if file_flags.is_main_file && func.name == "main" {
        return Some(EntryPointKind::Main);
    }

    None
}

/// Extract function definitions from JS/TS source.
fn extract_functions(source: &str) -> Vec<FnDef> {
    let mut functions = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    let re_fn_decl =
        Regex::new(r"^\s*(?:async\s+)?function\s+(\w+)\s*\(").unwrap();
    let re_arrow =
        Regex::new(r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s+)?(?:\([^)]*\)|[a-zA-Z_]\w*)\s*=>")
            .unwrap();
    let re_method =
        Regex::new(r"^\s*(?:async\s+)?(\w+)\s*\([^)]*\)\s*\{").unwrap();
    let re_export_fn =
        Regex::new(r"^\s*export\s+(?:default\s+)?(?:async\s+)?function\s+(\w+)\s*\(")
            .unwrap();
    // Test framework patterns: describe('name', () => { ... })
    let re_test_call =
        Regex::new(r"^\s*(describe|it|test)\s*\(").unwrap();

    // Track class context via brace depth.
    let mut in_class = false;
    let mut class_brace_depth = 0;

    for (i, line) in lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // Detect class start.
        if (trimmed.starts_with("class ") || trimmed.contains("class "))
            && trimmed.contains('{')
        {
            in_class = true;
            class_brace_depth = 1;
            // Count additional braces on the same line.
            for ch in trimmed.chars().skip(trimmed.find('{').unwrap_or(0) + 1) {
                match ch {
                    '{' => class_brace_depth += 1,
                    '}' => class_brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        if in_class {
            // Update brace depth.
            for ch in trimmed.chars() {
                match ch {
                    '{' => class_brace_depth += 1,
                    '}' => class_brace_depth -= 1,
                    _ => {}
                }
            }
            if class_brace_depth <= 0 {
                in_class = false;
                continue;
            }
            // Inside class: look for methods.
            if let Some(caps) = re_method.captures(line) {
                let name = caps.get(1).unwrap().as_str().to_string();
                // Skip keywords.
                if matches!(
                    name.as_str(),
                    "if" | "else" | "for" | "while" | "switch" | "catch" | "class"
                        | "return" | "new" | "typeof" | "delete" | "void"
                ) {
                    continue;
                }
                let end_line = find_brace_end(&lines, i);
                functions.push(FnDef {
                    name,
                    start_line: line_num,
                    end_line,
                });
            }
            continue;
        }

        // Test framework calls (describe/it/test) — treat as pseudo-functions.
        if let Some(caps) = re_test_call.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let end_line = find_brace_end(&lines, i);
            functions.push(FnDef {
                name,
                start_line: line_num,
                end_line,
            });
            continue;
        }

        // Export function declarations.
        if let Some(caps) = re_export_fn.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let end_line = find_brace_end(&lines, i);
            functions.push(FnDef {
                name,
                start_line: line_num,
                end_line,
            });
            continue;
        }

        // Regular function declarations.
        if let Some(caps) = re_fn_decl.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let end_line = find_brace_end(&lines, i);
            functions.push(FnDef {
                name,
                start_line: line_num,
                end_line,
            });
            continue;
        }

        // Arrow functions.
        if let Some(caps) = re_arrow.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let end_line = find_arrow_end(&lines, i);
            functions.push(FnDef {
                name,
                start_line: line_num,
                end_line,
            });
            continue;
        }
    }

    functions
}

/// Find the closing brace for a block starting at `start_idx`.
fn find_brace_end(lines: &[&str], start_idx: usize) -> u32 {
    let mut depth = 0i32;
    let mut found_open = false;
    for (i, line) in lines.iter().enumerate().skip(start_idx) {
        // Skip string contents for brace counting (simple heuristic).
        for ch in strip_strings(line).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    found_open = true;
                }
                '}' => {
                    depth -= 1;
                    if found_open && depth == 0 {
                        return (i + 1) as u32;
                    }
                }
                _ => {}
            }
        }
    }
    // If no closing brace found, return start line.
    (start_idx + 1) as u32
}

/// Find the end of an arrow function (either brace-delimited or single expression).
fn find_arrow_end(lines: &[&str], start_idx: usize) -> u32 {
    let line = lines[start_idx];
    // If arrow body has braces, use brace matching.
    if line.contains('{') {
        return find_brace_end(lines, start_idx);
    }
    // Single-expression arrow: look for the arrow, then the rest is one expression.
    // Could span multiple lines if there's no semicolon.
    for (i, line) in lines.iter().enumerate().skip(start_idx) {
        let l = line.trim();
        if i > start_idx && (l.is_empty() || l.ends_with(';') || l.ends_with(',')) {
            return (i + 1) as u32;
        }
        if i == start_idx && (l.ends_with(';') || l.ends_with(',')) {
            return (i + 1) as u32;
        }
    }
    (start_idx + 1) as u32
}

/// Very simple string stripping — replaces quoted content with spaces to avoid
/// counting braces inside strings. Handles single, double, and backtick quotes.
fn strip_strings(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match in_quote {
            Some(q) => {
                if ch == '\\' {
                    chars.next(); // skip escaped char
                    result.push(' ');
                    result.push(' ');
                } else if ch == q {
                    in_quote = None;
                    result.push(' ');
                } else {
                    result.push(' ');
                }
            }
            None => {
                if ch == '\'' || ch == '"' || ch == '`' {
                    in_quote = Some(ch);
                    result.push(' ');
                } else if ch == '/' && chars.peek() == Some(&'/') {
                    // Line comment: skip rest.
                    break;
                } else {
                    result.push(ch);
                }
            }
        }
    }
    result
}

/// Extract function calls from a body of lines.
fn extract_calls_in_body(
    body_lines: &[&str],
    base_line: u32,
    self_name: &str,
) -> Vec<CallSite> {
    let re_call = Regex::new(r"(\w+)\s*\(").unwrap();
    let re_method_call = Regex::new(r"(\w+)\.(\w+)\s*\(").unwrap();
    let re_require = Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();

    let block_keywords = ["if", "else", "for", "while", "switch"];

    let mut calls = Vec::new();
    let mut current_block: Option<u32> = None;
    let mut block_counter: u32 = 0;

    for (i, line) in body_lines.iter().enumerate() {
        let line_num = base_line + i as u32;
        let trimmed = line.trim();

        // Detect block boundaries.
        for kw in &block_keywords {
            if trimmed.starts_with(kw) && trimmed.contains('(') {
                block_counter += 1;
                current_block = Some(block_counter);
            }
        }

        // Extract method calls (obj.method()).
        for caps in re_method_call.captures_iter(trimmed) {
            let method = caps.get(2).unwrap().as_str();
            if is_call_target(method) && method != self_name {
                calls.push(CallSite {
                    callee_name: method.to_string(),
                    line: line_num,
                    block_id: current_block,
                });
            }
        }

        // Extract plain function calls.
        for caps in re_call.captures_iter(trimmed) {
            let name = caps.get(1).unwrap().as_str();
            if is_call_target(name) && name != self_name {
                // Avoid duplicating method calls already captured.
                let full_match_start = caps.get(0).unwrap().start();
                let is_method =
                    full_match_start > 0 && trimmed.as_bytes().get(full_match_start - 1) == Some(&b'.');
                if !is_method {
                    calls.push(CallSite {
                        callee_name: name.to_string(),
                        line: line_num,
                        block_id: current_block,
                    });
                }
            }
        }

        // Detect require() calls — these don't create edges to functions but we skip them.
        let _ = re_require;

        // Reset block at closing braces (simple heuristic).
        if trimmed == "}" {
            current_block = None;
        }
    }

    calls
}

/// Returns true if the name looks like a real function call (not a keyword or built-in).
fn is_call_target(name: &str) -> bool {
    !matches!(
        name,
        "if" | "else"
            | "for"
            | "while"
            | "switch"
            | "catch"
            | "return"
            | "new"
            | "typeof"
            | "delete"
            | "void"
            | "throw"
            | "yield"
            | "await"
            | "class"
            | "super"
            | "import"
            | "require"
            | "console"
            | "function"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn single_file_graph(source: &str) -> CallGraph {
        let mut sources = HashMap::new();
        sources.insert(PathBuf::from("test.js"), source.to_string());
        JsExtractor.extract(&sources)
    }

    #[test]
    fn function_declaration_and_call() {
        let src = r#"
function greet(name) {
    return "hello " + name;
}

function main() {
    greet("world");
}
"#;
        let g = single_file_graph(src);
        assert_eq!(g.node_count(), 2, "should find 2 functions");
        assert_eq!(g.edge_count(), 1, "main should call greet");
        let edge = &g.edges[0];
        let caller = g.node(edge.caller).unwrap();
        let callee = g.node(edge.callee).unwrap();
        assert_eq!(caller.name, "main");
        assert_eq!(callee.name, "greet");
    }

    #[test]
    fn arrow_function_detection() {
        let src = r#"
const add = (a, b) => {
    return a + b;
}

const multiply = (a, b) => a * b;

function compute() {
    add(1, 2);
    multiply(3, 4);
}
"#;
        let g = single_file_graph(src);
        // add, multiply, compute
        assert!(g.node_count() >= 3, "should find at least 3 functions, got {}", g.node_count());
        // compute calls add and multiply
        let compute_ids = g.fns_named("compute");
        assert_eq!(compute_ids.len(), 1);
        let edges_from_compute: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.caller == compute_ids[0])
            .collect();
        assert_eq!(
            edges_from_compute.len(),
            2,
            "compute should call add and multiply"
        );
    }

    #[test]
    fn class_method_detection() {
        let src = r#"
class Calculator {
    add(a, b) {
        return a + b;
    }

    multiply(a, b) {
        return a * b;
    }

    compute(x, y) {
        this.add(x, y);
        this.multiply(x, y);
    }
}
"#;
        let g = single_file_graph(src);
        assert!(g.node_count() >= 3, "should find 3 class methods, got {}", g.node_count());
        let names: Vec<_> = g.nodes.iter().map(|n| &n.name).collect();
        assert!(names.contains(&&"add".to_string()));
        assert!(names.contains(&&"multiply".to_string()));
        assert!(names.contains(&&"compute".to_string()));
    }

    #[test]
    fn express_route_entry_points() {
        let src = r#"
const express = require('express');
const app = express();

function handleGet(req, res) {
    res.send("ok");
}

function handlePost(req, res) {
    res.send("created");
}

app.get('/api/items', handleGet);
app.post('/api/items', handlePost);
"#;
        let g = single_file_graph(src);
        let get_fn = g.nodes.iter().find(|n| n.name == "handleGet").unwrap();
        assert_eq!(
            get_fn.entry_kind,
            Some(EntryPointKind::HttpHandler),
            "handleGet should be HttpHandler"
        );
        let post_fn = g.nodes.iter().find(|n| n.name == "handlePost").unwrap();
        assert_eq!(
            post_fn.entry_kind,
            Some(EntryPointKind::HttpHandler),
            "handlePost should be HttpHandler"
        );
    }

    #[test]
    fn jest_mocha_test_patterns() {
        let src = r#"
describe('math', () => {
    it('should add', () => {
        expect(add(1, 2)).toBe(3);
    });

    test('should multiply', () => {
        expect(multiply(2, 3)).toBe(6);
    });
});
"#;
        let g = single_file_graph(src);
        let test_entries: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.entry_kind == Some(EntryPointKind::Test))
            .collect();
        assert!(
            !test_entries.is_empty(),
            "should detect test entry points"
        );
        let test_names: Vec<_> = test_entries.iter().map(|n| &n.name).collect();
        assert!(test_names.contains(&&"describe".to_string()));
    }

    #[test]
    fn export_detection_public_api() {
        let src = r#"
export function processData(data) {
    return data.map(transform);
}

export default function main() {
    processData([1, 2, 3]);
}
"#;
        let g = single_file_graph(src);
        let process_fn = g.nodes.iter().find(|n| n.name == "processData").unwrap();
        assert_eq!(
            process_fn.entry_kind,
            Some(EntryPointKind::PublicApi),
            "exported function should be PublicApi"
        );
        let main_fn = g.nodes.iter().find(|n| n.name == "main").unwrap();
        assert_eq!(
            main_fn.entry_kind,
            Some(EntryPointKind::PublicApi),
            "export default function should be PublicApi"
        );
    }

    #[test]
    fn cross_file_call_resolution() {
        let mut sources = HashMap::new();
        sources.insert(
            PathBuf::from("utils.js"),
            r#"
function helper() {
    return 42;
}
"#
            .to_string(),
        );
        sources.insert(
            PathBuf::from("main.js"),
            r#"
function run() {
    helper();
}
"#
            .to_string(),
        );

        let g = JsExtractor.extract(&sources);
        assert_eq!(g.node_count(), 2, "should find 2 functions across files");
        assert_eq!(
            g.edge_count(),
            1,
            "run() should resolve call to helper() across files"
        );
        let edge = &g.edges[0];
        let caller = g.node(edge.caller).unwrap();
        let callee = g.node(edge.callee).unwrap();
        assert_eq!(caller.name, "run");
        assert_eq!(callee.name, "helper");
        assert_ne!(caller.file, callee.file, "should be in different files");
    }

    #[test]
    fn empty_source() {
        let g = single_file_graph("");
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn router_post_entry_point() {
        let src = r#"
const router = require('express').Router();

function createItem(req, res) {
    res.status(201).send({});
}

router.post('/items', createItem);
"#;
        let g = single_file_graph(src);
        let create_fn = g.nodes.iter().find(|n| n.name == "createItem").unwrap();
        assert_eq!(
            create_fn.entry_kind,
            Some(EntryPointKind::HttpHandler),
            "router.post handler should be HttpHandler"
        );
    }

    #[test]
    fn module_exports_public_api() {
        let src = r#"
function parse(input) {
    return JSON.parse(input);
}

module.exports = { parse };
"#;
        let g = single_file_graph(src);
        let parse_fn = g.nodes.iter().find(|n| n.name == "parse").unwrap();
        assert_eq!(
            parse_fn.entry_kind,
            Some(EntryPointKind::PublicApi),
            "module.exports function should be PublicApi"
        );
    }

    #[test]
    fn block_detection_assigns_block_ids() {
        let src = r#"
function process(x) {
    validate(x);
    if (x > 0) {
        handlePositive(x);
    } else {
        handleNegative(x);
    }
    finalize(x);
}

function validate(x) {}
function handlePositive(x) {}
function handleNegative(x) {}
function finalize(x) {}
"#;
        let g = single_file_graph(src);
        let process_edges: Vec<_> = g
            .edges
            .iter()
            .filter(|e| {
                g.node(e.caller)
                    .map(|n| n.name == "process")
                    .unwrap_or(false)
            })
            .collect();
        // Should have edges for validate, handlePositive, handleNegative, finalize
        assert!(
            process_edges.len() >= 3,
            "process should call at least 3 functions, got {}",
            process_edges.len()
        );
        // Some edges should have block IDs (those inside if/else)
        let with_blocks: Vec<_> = process_edges
            .iter()
            .filter(|e| e.call_site_block.is_some())
            .collect();
        assert!(
            !with_blocks.is_empty(),
            "calls inside if/else should have block IDs"
        );
    }

    #[test]
    fn async_function_detection() {
        let src = r#"
async function fetchData(url) {
    return fetch(url);
}

const processAsync = async (data) => {
    return data;
}
"#;
        let g = single_file_graph(src);
        let names: Vec<_> = g.nodes.iter().map(|n| &n.name).collect();
        assert!(names.contains(&&"fetchData".to_string()), "should detect async function");
        assert!(names.contains(&&"processAsync".to_string()), "should detect async arrow");
    }
}
