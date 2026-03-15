use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

pub struct SwiftExtractor;

static RE_FUNC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:(?:public|private|internal|open|fileprivate)\s+)?(?:static\s+)?(?:override\s+)?func\s+(\w+)\s*\(").unwrap());

static RE_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*\(").unwrap());

static RE_METHOD_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\.(\w+)\s*\(").unwrap());

static RE_TEST_FUNC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"func\s+(test\w+)\s*\(").unwrap());

const SWIFT_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "switch", "case", "default", "guard", "return",
    "break", "continue", "fallthrough", "func", "class", "struct", "enum",
    "protocol", "extension", "import", "var", "let", "typealias", "init", "deinit",
    "subscript", "where", "throw", "throws", "rethrows", "try", "catch", "defer",
    "repeat", "in", "as", "is", "self", "super", "print", "debugPrint", "fatalError",
    "precondition", "assert",
];

impl CallGraphExtractor for SwiftExtractor {
    fn language(&self) -> Language {
        Language::Swift
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        let mut fn_index: HashMap<String, Vec<FnId>> = HashMap::new();
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();
            let has_xctest = source.contains("XCTestCase");
            let _has_main_attr = source.contains("@main");
            let has_ui_delegate = source.contains("UIApplicationDelegate");

            let mut current_fn: Option<(FnId, i32)> = None;
            let mut brace_depth: i32 = 0;
            let mut block_id: u32 = 0;

            for (i, &line) in lines.iter().enumerate() {
                let line_num = (i + 1) as u32;
                let trimmed = line.trim();

                // Skip comments
                if trimmed.starts_with("//") {
                    continue;
                }

                // Detect function definition
                let func_match = RE_FUNC.captures(trimmed).map(|caps| caps[1].to_string());

                if let Some(name) = func_match {
                    // Close previous function
                    if let Some((prev_id, _)) = current_fn.take() {
                        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == prev_id) {
                            node.end_line = line_num.saturating_sub(1);
                        }
                    }

                    let entry_kind = if RE_TEST_FUNC.is_match(trimmed) && has_xctest {
                        Some(EntryPointKind::Test)
                    } else if name == "main"
                        || has_ui_delegate
                            && (name == "application"
                                || name == "applicationDidFinishLaunching")
                    {
                        Some(EntryPointKind::Main)
                    } else {
                        None
                    };

                    let id = FnId(next_id);
                    next_id += 1;
                    block_id = 0;

                    graph.nodes.push(FnNode {
                        id,
                        name: name.clone(),
                        file: path.clone(),
                        start_line: line_num,
                        end_line: line_num,
                        entry_kind,
                    });

                    fn_index.entry(name).or_default().push(id);
                    current_fn = Some((id, brace_depth));
                }

                // Count braces
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => {
                            brace_depth -= 1;
                            if let Some((fn_id, open_depth)) = &current_fn {
                                if brace_depth <= *open_depth {
                                    if let Some(node) =
                                        graph.nodes.iter_mut().find(|n| n.id == *fn_id)
                                    {
                                        node.end_line = line_num;
                                    }
                                    current_fn = None;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Extract calls inside current function
                if let Some((fn_id, _)) = &current_fn {
                    if trimmed.starts_with("if ")
                        || trimmed.starts_with("for ")
                        || trimmed.starts_with("while ")
                        || trimmed.starts_with("switch ")
                        || trimmed.starts_with("guard ")
                    {
                        block_id += 1;
                    }
                    let block = if block_id > 0 { Some(block_id) } else { None };

                    for caps in RE_METHOD_CALL.captures_iter(trimmed) {
                        let method = caps[2].to_string();
                        if !SWIFT_KEYWORDS.contains(&method.as_str()) {
                            pending_edges.push((*fn_id, method, line_num, block));
                        }
                    }

                    for caps in RE_CALL.captures_iter(trimmed) {
                        let name = caps[1].to_string();
                        if !SWIFT_KEYWORDS.contains(&name.as_str()) {
                            let m = caps.get(1).unwrap();
                            let is_method = m.start() > 0
                                && trimmed.as_bytes().get(m.start() - 1) == Some(&b'.');
                            if !is_method {
                                pending_edges.push((*fn_id, name, line_num, block));
                            }
                        }
                    }
                }
            }
        }

        // Resolve edges
        for (caller_id, callee_name, line, block) in pending_edges {
            if let Some(callee_ids) = fn_index.get(&callee_name) {
                for &callee_id in callee_ids {
                    if callee_id != caller_id {
                        graph.edges.push(CallEdge {
                            caller: caller_id,
                            callee: callee_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn single_file(src: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from("main.swift"), src.to_string());
        m
    }

    #[test]
    fn detects_functions_and_calls() {
        let src = "func main() {\n    helper()\n}\n\nfunc helper() {\n}\n";
        let g = SwiftExtractor.extract(&single_file(src));
        assert_eq!(g.node_count(), 2);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn test_entry_points_with_xctest() {
        let src = "import XCTest\n\nclass FooTests: XCTestCase {\n    func testAdd() {\n    }\n}\n";
        let g = SwiftExtractor.extract(&single_file(src));
        let test_fn = g.nodes.iter().find(|n| n.name == "testAdd").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn main_entry_point() {
        let src = "@main\nstruct App {\n    static func main() {\n    }\n}\n";
        let g = SwiftExtractor.extract(&single_file(src));
        let main_fn = g.nodes.iter().find(|n| n.name == "main").unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn ui_delegate_entry_point() {
        let src = "class AppDelegate: UIResponder, UIApplicationDelegate {\n    func application() {\n    }\n}\n";
        let g = SwiftExtractor.extract(&single_file(src));
        let app_fn = g.nodes.iter().find(|n| n.name == "application").unwrap();
        assert_eq!(app_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn cross_file_resolution() {
        let mut sources = HashMap::new();
        sources.insert(
            PathBuf::from("a.swift"),
            "func caller() {\n    helper()\n}\n".to_string(),
        );
        sources.insert(
            PathBuf::from("b.swift"),
            "func helper() {\n}\n".to_string(),
        );
        let g = SwiftExtractor.extract(&sources);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn empty_source() {
        let g = SwiftExtractor.extract(&HashMap::new());
        assert_eq!(g.node_count(), 0);
    }

    #[test]
    fn public_func_detected() {
        let src = "public func doSomething() {\n}\n";
        let g = SwiftExtractor.extract(&single_file(src));
        assert_eq!(g.node_count(), 1);
        assert_eq!(g.nodes[0].name, "doSomething");
    }
}
