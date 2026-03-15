use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

pub struct JavaExtractor;

static RE_METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:public|private|protected)?\s*(?:static\s+)?(?:final\s+)?(?:synchronized\s+)?\w+(?:<[^>]+>)?\s+(\w+)\s*\(").unwrap()
});

static RE_KOTLIN_FUN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:public|private|protected|internal)?\s*(?:override\s+)?(?:suspend\s+)?fun\s+(\w+)\s*\(").unwrap()
});

static RE_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\w+)\s*\(").unwrap());
static RE_METHOD_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\.(\w+)\s*\(").unwrap());

static RE_TEST_ANNO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(Test|ParameterizedTest)").unwrap());
static RE_HTTP_ANNO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@(GetMapping|PostMapping|PutMapping|DeleteMapping|RequestMapping)").unwrap()
});
static RE_CONTROLLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(RestController|Controller)").unwrap());
static RE_MAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"public\s+static\s+void\s+main\s*\(").unwrap());

const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "switch", "case", "try", "catch", "finally",
    "throw", "return", "new", "class", "interface", "enum", "import", "package",
    "super", "this", "void", "null", "true", "false", "instanceof", "synchronized",
    "assert", "break", "continue", "default", "do",
];

impl CallGraphExtractor for JavaExtractor {
    fn language(&self) -> Language {
        Language::Java
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        let mut fn_index: HashMap<String, Vec<FnId>> = HashMap::new();
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();
            let is_controller = lines.iter().any(|l| RE_CONTROLLER.is_match(l));
            let mut pending_test = false;
            let mut pending_http = false;
            let mut current_fn: Option<(FnId, u32, u32)> = None; // (id, start_line, brace_depth_at_open)
            let mut brace_depth: i32 = 0;
            let mut block_id: u32 = 0;

            for (i, &line) in lines.iter().enumerate() {
                let line_num = (i + 1) as u32;
                let trimmed = line.trim();

                // Track annotations
                if RE_TEST_ANNO.is_match(trimmed) {
                    pending_test = true;
                }
                if RE_HTTP_ANNO.is_match(trimmed) {
                    pending_http = true;
                }

                // Skip comments
                if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
                    continue;
                }

                // Detect method (Java or Kotlin style)
                let method_name = RE_METHOD
                    .captures(trimmed)
                    .or_else(|| RE_KOTLIN_FUN.captures(trimmed))
                    .and_then(|caps| caps.get(1))
                    .map(|m| m.as_str().to_string());

                let found_method = method_name.is_some();
                if let Some(name) = method_name {
                    // Close previous function if open
                    if let Some((prev_id, _start, _)) = current_fn.take() {
                        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == prev_id) {
                            node.end_line = line_num.saturating_sub(1);
                        }
                    }

                    let entry_kind = if pending_test {
                        Some(EntryPointKind::Test)
                    } else if RE_MAIN.is_match(trimmed) {
                        Some(EntryPointKind::Main)
                    } else if pending_http {
                        Some(EntryPointKind::HttpHandler)
                    } else if is_controller && trimmed.contains("public") {
                        Some(EntryPointKind::PublicApi)
                    } else {
                        None
                    };

                    pending_test = false;
                    pending_http = false;

                    let id = FnId(next_id);
                    next_id += 1;
                    block_id = 0;

                    graph.nodes.push(FnNode {
                        id,
                        name: name.clone(),
                        file: path.clone(),
                        start_line: line_num,
                        end_line: line_num, // updated when closing brace found
                        entry_kind,
                    });

                    fn_index.entry(name).or_default().push(id);
                    current_fn = Some((id, line_num, brace_depth as u32));
                }

                // Count braces
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => {
                            brace_depth -= 1;
                            if let Some((fn_id, _, open_depth)) = &current_fn {
                                if brace_depth <= *open_depth as i32 {
                                    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == *fn_id) {
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
                if let Some((fn_id, _, _)) = &current_fn {
                    // Track blocks
                    if trimmed.starts_with("if ") || trimmed.starts_with("for ") || trimmed.starts_with("while ") || trimmed.starts_with("switch ") || trimmed.starts_with("try ") {
                        block_id += 1;
                    }

                    let block = if block_id > 0 { Some(block_id) } else { None };

                    // Method calls: obj.method()
                    for caps in RE_METHOD_CALL.captures_iter(trimmed) {
                        let method = caps[2].to_string();
                        if !KEYWORDS.contains(&method.as_str()) {
                            pending_edges.push((*fn_id, method, line_num, block));
                        }
                    }

                    // Simple calls: method()
                    for caps in RE_CALL.captures_iter(trimmed) {
                        let name = caps[1].to_string();
                        if !KEYWORDS.contains(&name.as_str()) {
                            let m = caps.get(1).unwrap();
                            let is_method = m.start() > 0
                                && trimmed.as_bytes().get(m.start() - 1) == Some(&b'.');
                            if !is_method {
                                pending_edges.push((*fn_id, name, line_num, block));
                            }
                        }
                    }
                }

                // Reset annotations if line is not an annotation
                if !trimmed.is_empty() && !trimmed.starts_with('@') && !found_method {
                    pending_test = false;
                    pending_http = false;
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
        m.insert(PathBuf::from("App.java"), src.to_string());
        m
    }

    #[test]
    fn detects_java_methods() {
        let src = "public class App {\n    public void hello() {\n        world();\n    }\n    void world() {\n    }\n}\n";
        let g = JavaExtractor.extract(&single_file(src));
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.fns_named("hello").len(), 1);
        assert_eq!(g.fns_named("world").len(), 1);
    }

    #[test]
    fn detects_call_edges() {
        let src = "class A {\n    void caller() {\n        callee();\n    }\n    void callee() {\n    }\n}\n";
        let g = JavaExtractor.extract(&single_file(src));
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn test_annotation_entry_point() {
        let src = "class T {\n    @Test\n    void testSomething() {\n    }\n}\n";
        let g = JavaExtractor.extract(&single_file(src));
        let test_fn = g.nodes.iter().find(|n| n.name == "testSomething").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn main_entry_point() {
        let src = "class App {\n    public static void main(String[] args) {\n    }\n}\n";
        let g = JavaExtractor.extract(&single_file(src));
        let main_fn = g.nodes.iter().find(|n| n.name == "main").unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn http_handler_entry_point() {
        let src = "@RestController\nclass Api {\n    @GetMapping\n    public String list() {\n        return \"ok\";\n    }\n}\n";
        let g = JavaExtractor.extract(&single_file(src));
        let list_fn = g.nodes.iter().find(|n| n.name == "list").unwrap();
        assert_eq!(list_fn.entry_kind, Some(EntryPointKind::HttpHandler));
    }

    #[test]
    fn controller_public_api() {
        let src = "@RestController\nclass Api {\n    public String getData() {\n        return \"data\";\n    }\n}\n";
        let g = JavaExtractor.extract(&single_file(src));
        let get_fn = g.nodes.iter().find(|n| n.name == "getData").unwrap();
        assert!(get_fn.entry_kind.is_some());
    }

    #[test]
    fn cross_file_resolution() {
        let mut sources = HashMap::new();
        sources.insert(PathBuf::from("A.java"), "class A {\n    void use() {\n        helper();\n    }\n}\n".to_string());
        sources.insert(PathBuf::from("B.java"), "class B {\n    void helper() {\n    }\n}\n".to_string());
        let g = JavaExtractor.extract(&sources);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn kotlin_fun_detection() {
        let mut sources = HashMap::new();
        sources.insert(PathBuf::from("App.kt"), "fun main() {\n    greet()\n}\n\nfun greet() {\n    println(\"hi\")\n}\n".to_string());
        let g = JavaExtractor.extract(&sources);
        assert_eq!(g.fns_named("main").len(), 1);
        assert_eq!(g.fns_named("greet").len(), 1);
    }

    #[test]
    fn empty_source() {
        let g = JavaExtractor.extract(&HashMap::new());
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }
}
