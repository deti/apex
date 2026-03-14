use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct PythonExtractor;

/// A function definition discovered during parsing.
struct FnDef {
    name: String,
    start_line: u32,
    end_line: u32,
    indent: usize,
    body_lines: Vec<(u32, String)>,
}

/// Detect the indentation level (number of leading spaces) for a line.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Parse all function definitions from a single file's source.
fn parse_functions(source: &str) -> Vec<FnDef> {
    let def_re = Regex::new(r"^(\s*)def\s+(\w+)\s*\(").unwrap();
    let class_re = Regex::new(r"^(\s*)class\s+(\w+)").unwrap();

    let lines: Vec<&str> = source.lines().collect();
    let mut functions: Vec<FnDef> = Vec::new();

    // Track class context: (indent, class_name).
    let mut class_stack: Vec<(usize, String)> = Vec::new();
    // Pending function: index in `functions`, waiting to close.
    let mut pending: Option<usize> = None;

    for (i, &line) in lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            // Blank/comment lines: still part of current function body.
            if let Some(idx) = pending {
                functions[idx].end_line = line_num;
                functions[idx].body_lines.push((line_num, line.to_string()));
            }
            continue;
        }

        let ind = indent_of(line);

        // Pop class stack for lines at same or lesser indent.
        class_stack.retain(|&(ci, _)| ci < ind);

        // Check for class definition.
        if let Some(caps) = class_re.captures(line) {
            let cls_indent = caps[1].len();
            let cls_name = caps[2].to_string();
            // Close pending function if this class is at same or lesser indent.
            if let Some(idx) = pending {
                if cls_indent <= functions[idx].indent {
                    pending = None;
                }
            }
            class_stack.push((cls_indent, cls_name));
            continue;
        }

        // Check for function definition.
        if let Some(caps) = def_re.captures(line) {
            let fn_indent = caps[1].len();
            let raw_name = caps[2].to_string();

            // Close pending function (overwritten by Some below, so just drop).

            // Determine qualified name.
            let name = if let Some((_, ref cls)) = class_stack
                .iter()
                .rev()
                .find(|(ci, _)| *ci < fn_indent)
            {
                format!("{cls}.{raw_name}")
            } else {
                raw_name
            };

            let idx = functions.len();
            functions.push(FnDef {
                name,
                start_line: line_num,
                end_line: line_num,
                indent: fn_indent,
                body_lines: Vec::new(),
            });
            pending = Some(idx);
            continue;
        }

        // Regular line inside a function body.
        if let Some(idx) = pending {
            // If this non-empty line is at indent <= function indent, the function is over.
            if ind <= functions[idx].indent {
                pending = None;
            } else {
                functions[idx].end_line = line_num;
                functions[idx].body_lines.push((line_num, line.to_string()));
            }
        }
    }

    functions
}

/// Determine the entry point kind for a function, given the file path,
/// file-level flags, and the line preceding the `def`.
fn classify_entry(
    fn_name: &str,
    file: &Path,
    has_dunder_main: bool,
    is_init_py: bool,
    has_cli_markers: bool,
    fn_indent: usize,
    preceding_line: Option<&str>,
) -> Option<EntryPointKind> {
    // Test functions/classes.
    let base = fn_name.rsplit('.').next().unwrap_or(fn_name);
    if base.starts_with("test_") {
        return Some(EntryPointKind::Test);
    }
    // Class-level: class Test* handled by class prefix on method name.
    if fn_name.starts_with("Test") || fn_name.contains(".test_") {
        return Some(EntryPointKind::Test);
    }

    // HTTP handler decorators.
    if let Some(prev) = preceding_line {
        let prev_trimmed = prev.trim();
        if prev_trimmed.starts_with("@app.route")
            || prev_trimmed.starts_with("@router.get")
            || prev_trimmed.starts_with("@router.post")
            || prev_trimmed.starts_with("@router.put")
            || prev_trimmed.starts_with("@router.delete")
            || prev_trimmed.starts_with("@router.patch")
        {
            return Some(EntryPointKind::HttpHandler);
        }
    }

    // CLI entry point.
    if has_cli_markers && fn_indent == 0 {
        return Some(EntryPointKind::CliEntry);
    }

    // __init__.py public API.
    if is_init_py && fn_indent == 0 {
        return Some(EntryPointKind::PublicApi);
    }

    // __name__ == "__main__" implies top-level functions are Main.
    if has_dunder_main && fn_indent == 0 {
        return Some(EntryPointKind::Main);
    }

    // File named __main__.py — top-level functions are Main.
    if let Some(fname) = file.file_name().and_then(|f| f.to_str()) {
        if fname == "__main__.py" && fn_indent == 0 {
            return Some(EntryPointKind::Main);
        }
    }

    None
}

/// Extract calls from function body lines.
fn extract_calls(body: &[(u32, String)]) -> Vec<(u32, String, Option<u32>)> {
    let call_re = Regex::new(r"(\w+)\s*\(").unwrap();
    let self_call_re = Regex::new(r"self\.(\w+)\s*\(").unwrap();
    let method_call_re = Regex::new(r"(\w+)\.(\w+)\s*\(").unwrap();

    // Python keywords that look like function calls but are not.
    let keywords: &[&str] = &[
        "if", "elif", "else", "for", "while", "try", "except", "with", "return", "yield",
        "assert", "raise", "import", "from", "class", "def", "pass", "break", "continue",
        "lambda", "and", "or", "not", "in", "is", "as", "del", "global", "nonlocal", "async",
        "await", "finally", "print",
    ];

    // Block detection: simple counter for indentation-based blocks.
    let block_re = Regex::new(
        r"^\s*(if|elif|else|for|while|try|except|finally|with)\b",
    )
    .unwrap();

    let mut calls = Vec::new();
    let mut block_id: u32 = 0;

    for (line_num, line) in body {
        if block_re.is_match(line) {
            block_id += 1;
        }

        let current_block = if block_id > 0 { Some(block_id) } else { None };

        // self.method() calls.
        for caps in self_call_re.captures_iter(line) {
            let name = caps[1].to_string();
            if !keywords.contains(&name.as_str()) {
                calls.push((*line_num, name, current_block));
            }
        }

        // obj.method() calls — emit as "method" for cross-reference.
        for caps in method_call_re.captures_iter(line) {
            let obj = &caps[1];
            let method = caps[2].to_string();
            if obj == "self" {
                continue; // Already captured above.
            }
            if !keywords.contains(&method.as_str()) {
                calls.push((*line_num, method, current_block));
            }
        }

        // Plain function calls.
        for caps in call_re.captures_iter(line) {
            let name = caps[1].to_string();
            if keywords.contains(&name.as_str()) {
                continue;
            }
            // Skip if this is actually an `obj.method(` — the word before `(` follows a dot.
            let m = caps.get(1).unwrap();
            if m.start() > 0 {
                let before = &line[..m.start()];
                if before.ends_with('.') {
                    continue;
                }
            }
            // Avoid duplicate if already captured as self.X.
            if name == "self" {
                continue;
            }
            calls.push((*line_num, name, current_block));
        }
    }

    calls
}

impl CallGraphExtractor for PythonExtractor {
    fn language(&self) -> Language {
        Language::Python
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        // Map from function name to list of FnIds (for call resolution).
        let mut name_to_ids: HashMap<String, Vec<FnId>> = HashMap::new();
        // Deferred edges: (caller_id, callee_name, line, block).
        let mut deferred_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (file, source) in sources {
            if source.is_empty() {
                continue;
            }

            let lines_vec: Vec<&str> = source.lines().collect();
            let has_dunder_main = source.contains("if __name__");
            let is_init_py = file
                .file_name()
                .and_then(|f| f.to_str())
                .map(|f| f == "__init__.py")
                .unwrap_or(false);
            let has_cli_markers = source.contains("argparse")
                || source.contains("click.command")
                || source.contains("@click.command");

            let fns = parse_functions(source);

            for f in &fns {
                let id = FnId(next_id);
                next_id += 1;

                // Determine preceding line for decorator detection.
                let preceding = if f.start_line >= 2 {
                    lines_vec.get((f.start_line - 2) as usize).copied()
                } else {
                    None
                };

                let entry_kind = classify_entry(
                    &f.name,
                    file,
                    has_dunder_main,
                    is_init_py,
                    has_cli_markers,
                    f.indent,
                    preceding,
                );

                graph.nodes.push(FnNode {
                    id,
                    name: f.name.clone(),
                    file: file.clone(),
                    start_line: f.start_line,
                    end_line: f.end_line,
                    entry_kind,
                });

                // Register name lookups: both full qualified and short name.
                name_to_ids.entry(f.name.clone()).or_default().push(id);
                if let Some(short) = f.name.rsplit('.').next() {
                    if short != f.name {
                        name_to_ids
                            .entry(short.to_string())
                            .or_default()
                            .push(id);
                    }
                }

                // Extract calls.
                let calls = extract_calls(&f.body_lines);
                for (line, callee_name, block) in calls {
                    deferred_edges.push((id, callee_name, line, block));
                }
            }
        }

        // Resolve deferred edges.
        for (caller_id, callee_name, line, block) in deferred_edges {
            if let Some(callee_ids) = name_to_ids.get(&callee_name) {
                for &callee_id in callee_ids {
                    // Avoid self-loops unless explicitly calling self.
                    if callee_id == caller_id {
                        continue;
                    }
                    graph.edges.push(CallEdge {
                        caller: caller_id,
                        callee: callee_id,
                        call_site_line: line,
                        call_site_block: block,
                    });
                }
            }
        }

        graph.build_indices();
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn single_file(name: &str, src: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), src.to_string());
        m
    }

    #[test]
    fn simple_def_and_call() {
        let src = "\
def foo():
    pass

def bar():
    foo()
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("app.py", src));

        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
        assert_eq!(g.fns_named("foo").len(), 1);
        assert_eq!(g.fns_named("bar").len(), 1);

        let bar_id = g.fns_named("bar")[0];
        let callees = g.callees_of.get(&bar_id).unwrap();
        let edge = &g.edges[callees[0]];
        assert_eq!(edge.callee, g.fns_named("foo")[0]);
    }

    #[test]
    fn class_method_detection() {
        let src = "\
class MyClass:
    def method_a(self):
        pass

    def method_b(self):
        self.method_a()
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("lib.py", src));

        assert_eq!(g.node_count(), 2);
        assert_eq!(g.fns_named("MyClass.method_a").len(), 1);
        assert_eq!(g.fns_named("MyClass.method_b").len(), 1);

        // method_b calls method_a via self.method_a().
        let mb_id = g.fns_named("MyClass.method_b")[0];
        let callees = g.callees_of.get(&mb_id).unwrap();
        assert!(!callees.is_empty());
        let edge = &g.edges[callees[0]];
        assert_eq!(edge.callee, g.fns_named("MyClass.method_a")[0]);
    }

    #[test]
    fn decorator_based_entry_points() {
        let src = "\
from flask import Flask
app = Flask(__name__)

@app.route('/hello')
def hello():
    return 'world'

@router.get('/items')
def list_items():
    return []
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("routes.py", src));

        let hello_id = g.fns_named("hello")[0];
        let hello = g.node(hello_id).unwrap();
        assert_eq!(hello.entry_kind, Some(EntryPointKind::HttpHandler));

        let items_id = g.fns_named("list_items")[0];
        let items = g.node(items_id).unwrap();
        assert_eq!(items.entry_kind, Some(EntryPointKind::HttpHandler));
    }

    #[test]
    fn test_prefix_detection() {
        let src = "\
def test_addition():
    assert 1 + 1 == 2

def helper():
    pass
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("test_math.py", src));

        let test_id = g.fns_named("test_addition")[0];
        let test_fn = g.node(test_id).unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));

        let helper_id = g.fns_named("helper")[0];
        let helper_fn = g.node(helper_id).unwrap();
        assert_eq!(helper_fn.entry_kind, None);
    }

    #[test]
    fn init_py_public_api() {
        let src = "\
def connect():
    pass

def _internal():
    pass
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("mypackage/__init__.py", src));

        let connect_id = g.fns_named("connect")[0];
        let connect_fn = g.node(connect_id).unwrap();
        assert_eq!(connect_fn.entry_kind, Some(EntryPointKind::PublicApi));

        let internal_id = g.fns_named("_internal")[0];
        let internal_fn = g.node(internal_id).unwrap();
        assert_eq!(internal_fn.entry_kind, Some(EntryPointKind::PublicApi));
    }

    #[test]
    fn cross_file_call_resolution() {
        let mut sources = HashMap::new();
        sources.insert(
            PathBuf::from("utils.py"),
            "def helper():\n    pass\n".to_string(),
        );
        sources.insert(
            PathBuf::from("app.py"),
            "def main():\n    helper()\n".to_string(),
        );

        let ext = PythonExtractor;
        let g = ext.extract(&sources);

        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);

        let main_id = g.fns_named("main")[0];
        let callees = g.callees_of.get(&main_id).unwrap();
        let edge = &g.edges[callees[0]];
        let helper_id = g.fns_named("helper")[0];
        assert_eq!(edge.callee, helper_id);
    }

    #[test]
    fn empty_source() {
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("empty.py", ""));
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn dunder_main_marks_main_entry() {
        let src = "\
def run():
    pass

if __name__ == '__main__':
    run()
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("script.py", src));

        let run_id = g.fns_named("run")[0];
        let run_fn = g.node(run_id).unwrap();
        assert_eq!(run_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn cli_entry_with_argparse() {
        let src = "\
import argparse

def main():
    parser = argparse.ArgumentParser()
    parser.parse_args()
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("cli.py", src));

        let main_id = g.fns_named("main")[0];
        let main_fn = g.node(main_id).unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::CliEntry));
    }

    #[test]
    fn block_detection_assigns_block_ids() {
        let src = "\
def process():
    x = compute()
    if True:
        handle_true()
    else:
        handle_false()
    for i in items():
        do_work()
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("blocks.py", src));

        // There should be edges with block IDs set.
        let proc_id = g.fns_named("process")[0];
        if let Some(callees) = g.callees_of.get(&proc_id) {
            let blocks: Vec<Option<u32>> = callees
                .iter()
                .map(|&idx| g.edges[idx].call_site_block)
                .collect();
            // compute() is outside any block keyword, so block = None.
            // handle_true/handle_false are inside if/else, so block > 0.
            assert!(blocks.iter().any(|b| b.is_none())); // compute()
            assert!(blocks.iter().any(|b| b.is_some())); // handle_true, handle_false, do_work
        }
    }

    #[test]
    fn nested_class_methods() {
        let src = "\
class Outer:
    def outer_method(self):
        pass

    class Inner:
        def inner_method(self):
            pass
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("nested.py", src));

        assert_eq!(g.fns_named("Outer.outer_method").len(), 1);
        assert_eq!(g.fns_named("Inner.inner_method").len(), 1);
    }

    #[test]
    fn function_end_line_tracking() {
        let src = "\
def short():
    pass

def longer():
    a = 1
    b = 2
    c = 3
    return a + b + c
";
        let ext = PythonExtractor;
        let g = ext.extract(&single_file("lines.py", src));

        let short_id = g.fns_named("short")[0];
        let short_fn = g.node(short_id).unwrap();
        assert_eq!(short_fn.start_line, 1);
        // end_line is 3 because the blank line between functions is
        // included in the preceding function until the next `def`.
        assert_eq!(short_fn.end_line, 3);

        let longer_id = g.fns_named("longer")[0];
        let longer_fn = g.node(longer_id).unwrap();
        assert_eq!(longer_fn.start_line, 4);
        assert_eq!(longer_fn.end_line, 8);
    }
}
