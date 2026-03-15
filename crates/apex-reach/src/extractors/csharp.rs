use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

pub struct CSharpExtractor;

/// Match: access_modifier? static? return_type name(
static RE_METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*(?:(?:public|private|protected|internal)\s+)?(?:static\s+)?(?:async\s+)?(?:override\s+)?(?:virtual\s+)?(?:\w+(?:<[^>]+>)?)\s+(\w+)\s*\(",
    )
    .unwrap()
});

static RE_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*\(").unwrap());

static RE_METHOD_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\.(\w+)\s*\(").unwrap());

const CSHARP_KEYWORDS: &[&str] = &[
    "if", "else", "for", "foreach", "while", "switch", "case", "default", "return",
    "break", "continue", "class", "struct", "interface", "enum", "namespace",
    "using", "new", "this", "base", "throw", "try", "catch", "finally", "lock",
    "typeof", "sizeof", "nameof", "await", "var", "void", "int", "string", "bool",
    "float", "double", "decimal", "object", "byte", "char", "long", "short",
    "where", "yield", "from", "select", "when",
];

impl CallGraphExtractor for CSharpExtractor {
    fn language(&self) -> Language {
        Language::CSharp
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        let mut fn_index: HashMap<String, Vec<FnId>> = HashMap::new();
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();

            let mut current_fn: Option<(FnId, i32)> = None;
            let mut brace_depth: i32 = 0;
            let mut block_id: u32 = 0;
            let mut prev_line_attrs: Vec<String> = Vec::new();

            for (i, &line) in lines.iter().enumerate() {
                let line_num = (i + 1) as u32;
                let trimmed = line.trim();

                // Skip comments
                if trimmed.starts_with("//") {
                    continue;
                }

                // Collect attributes from preceding lines
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    prev_line_attrs.push(trimmed.to_string());
                    continue;
                }

                // Detect method definition
                let func_match = if let Some(caps) = RE_METHOD.captures(trimmed) {
                    let name = caps[1].to_string();
                    // Filter out control flow that looks like method calls
                    if CSHARP_KEYWORDS.contains(&name.as_str()) {
                        None
                    } else {
                        Some(name)
                    }
                } else {
                    None
                };

                if let Some(name) = func_match {
                    // Close previous function
                    if let Some((prev_id, _)) = current_fn.take() {
                        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == prev_id) {
                            node.end_line = line_num.saturating_sub(1);
                        }
                    }

                    let has_test_attr = prev_line_attrs.iter().any(|a| {
                        a.contains("[Test]")
                            || a.contains("[Fact]")
                            || a.contains("[Theory]")
                            || a.contains("[TestMethod]")
                    });
                    let has_http_attr = prev_line_attrs.iter().any(|a| {
                        a.contains("[HttpGet")
                            || a.contains("[HttpPost")
                            || a.contains("[HttpPut")
                            || a.contains("[HttpDelete")
                            || a.contains("[Route")
                    });
                    let is_main = name == "Main"
                        && trimmed.contains("static")
                        && trimmed.contains("void");

                    let entry_kind = if has_test_attr {
                        Some(EntryPointKind::Test)
                    } else if is_main {
                        Some(EntryPointKind::Main)
                    } else if has_http_attr {
                        Some(EntryPointKind::HttpHandler)
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
                    prev_line_attrs.clear();
                } else if !trimmed.starts_with('[') {
                    prev_line_attrs.clear();
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
                        || trimmed.starts_with("foreach ")
                        || trimmed.starts_with("while ")
                        || trimmed.starts_with("switch ")
                    {
                        block_id += 1;
                    }
                    let block = if block_id > 0 { Some(block_id) } else { None };

                    for caps in RE_METHOD_CALL.captures_iter(trimmed) {
                        let method = caps[2].to_string();
                        if !CSHARP_KEYWORDS.contains(&method.as_str()) {
                            pending_edges.push((*fn_id, method, line_num, block));
                        }
                    }

                    for caps in RE_CALL.captures_iter(trimmed) {
                        let name = caps[1].to_string();
                        if !CSHARP_KEYWORDS.contains(&name.as_str()) {
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
        m.insert(PathBuf::from("Program.cs"), src.to_string());
        m
    }

    #[test]
    fn detects_methods_and_calls() {
        let src = "class Program {\n    static void Main(string[] args) {\n        Helper();\n    }\n\n    static void Helper() {\n    }\n}\n";
        let g = CSharpExtractor.extract(&single_file(src));
        assert_eq!(g.node_count(), 2);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn test_attribute_entry_point() {
        let src = "using NUnit.Framework;\n\nclass Tests {\n    [Test]\n    public void TestAdd() {\n    }\n}\n";
        let g = CSharpExtractor.extract(&single_file(src));
        let test_fn = g.nodes.iter().find(|n| n.name == "TestAdd").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn fact_attribute_entry_point() {
        let src = "using Xunit;\n\nclass Tests {\n    [Fact]\n    public void TestMultiply() {\n    }\n}\n";
        let g = CSharpExtractor.extract(&single_file(src));
        let test_fn = g.nodes.iter().find(|n| n.name == "TestMultiply").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn main_entry_point() {
        let src = "class Program {\n    static void Main(string[] args) {\n    }\n}\n";
        let g = CSharpExtractor.extract(&single_file(src));
        let main_fn = g.nodes.iter().find(|n| n.name == "Main").unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn http_handler_entry_point() {
        let src = "class Controller {\n    [HttpGet]\n    public IActionResult GetItems() {\n    }\n}\n";
        let g = CSharpExtractor.extract(&single_file(src));
        let handler = g.nodes.iter().find(|n| n.name == "GetItems").unwrap();
        assert_eq!(handler.entry_kind, Some(EntryPointKind::HttpHandler));
    }

    #[test]
    fn cross_file_resolution() {
        let mut sources = HashMap::new();
        sources.insert(
            PathBuf::from("A.cs"),
            "class A {\n    void Caller() {\n        Helper();\n    }\n}\n".to_string(),
        );
        sources.insert(
            PathBuf::from("B.cs"),
            "class B {\n    void Helper() {\n    }\n}\n".to_string(),
        );
        let g = CSharpExtractor.extract(&sources);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn empty_source() {
        let g = CSharpExtractor.extract(&HashMap::new());
        assert_eq!(g.node_count(), 0);
    }
}
