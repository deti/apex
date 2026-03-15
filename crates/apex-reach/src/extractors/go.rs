use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

pub struct GoExtractor;

static RE_FUNC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^func\s+(\w+)\s*\(").unwrap());

static RE_METHOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^func\s+\(\w+\s+\*?(\w+)\)\s+(\w+)\s*\(").unwrap());

static RE_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*\(").unwrap());

static RE_METHOD_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\.(\w+)\s*\(").unwrap());

static RE_TEST_FUNC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^func\s+(Test\w+|Benchmark\w+)\s*\(").unwrap());

const GO_KEYWORDS: &[&str] = &[
    "if", "else", "for", "switch", "select", "case", "default", "go", "defer",
    "return", "break", "continue", "fallthrough", "range", "func", "type",
    "struct", "interface", "map", "chan", "var", "const", "package", "import",
    "make", "new", "append", "len", "cap", "close", "delete", "copy", "panic",
    "recover", "print", "println",
];

impl CallGraphExtractor for GoExtractor {
    fn language(&self) -> Language {
        Language::Go
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        let mut fn_index: HashMap<String, Vec<FnId>> = HashMap::new();
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();
            let has_cobra = source.contains("cobra.Command") || source.contains("cobra.RootCmd");
            let has_flag = source.contains("flag.Parse()");
            let has_http_handler = source.contains("http.HandleFunc")
                || source.contains("mux.HandleFunc")
                || source.contains(".GET(")
                || source.contains(".POST(");

            let mut current_fn: Option<(FnId, i32)> = None; // (id, brace_depth_at_open)
            let mut brace_depth: i32 = 0;
            let mut block_id: u32 = 0;

            for (i, &line) in lines.iter().enumerate() {
                let line_num = (i + 1) as u32;
                let trimmed = line.trim();

                if trimmed.starts_with("//") {
                    continue;
                }

                // Detect function/method definition
                let func_match = if let Some(caps) = RE_METHOD.captures(trimmed) {
                    let type_name = caps[1].to_string();
                    let method_name = caps[2].to_string();
                    Some(format!("{}.{}", type_name, method_name))
                } else {
                    RE_FUNC.captures(trimmed).map(|caps| caps[1].to_string())
                };

                if let Some(name) = func_match {
                    // Close previous function
                    if let Some((prev_id, _)) = current_fn.take() {
                        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == prev_id) {
                            node.end_line = line_num.saturating_sub(1);
                        }
                    }

                    let short_name = name.rsplit('.').next().unwrap_or(&name);
                    let is_exported = short_name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);

                    let entry_kind = if RE_TEST_FUNC.is_match(trimmed) {
                        Some(EntryPointKind::Test)
                    } else if short_name == "main" {
                        if has_cobra || has_flag {
                            Some(EntryPointKind::CliEntry)
                        } else {
                            Some(EntryPointKind::Main)
                        }
                    } else if has_http_handler && is_exported {
                        // Check if this function name appears in a HandleFunc/router registration
                        let name_in_handler = lines.iter().any(|l| {
                            (l.contains("HandleFunc") || l.contains(".GET(") || l.contains(".POST("))
                                && l.contains(short_name)
                        });
                        if name_in_handler {
                            Some(EntryPointKind::HttpHandler)
                        } else if is_exported {
                            Some(EntryPointKind::PublicApi)
                        } else {
                            None
                        }
                    } else if is_exported {
                        Some(EntryPointKind::PublicApi)
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

                    fn_index.entry(name.clone()).or_default().push(id);
                    // Also register short name for method resolution
                    let short = name.rsplit('.').next().unwrap_or(&name).to_string();
                    if short != name {
                        fn_index.entry(short).or_default().push(id);
                    }

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
                if let Some((fn_id, _)) = &current_fn {
                    if trimmed.starts_with("if ") || trimmed.starts_with("for ") || trimmed.starts_with("switch ") || trimmed.starts_with("select ") {
                        block_id += 1;
                    }
                    let block = if block_id > 0 { Some(block_id) } else { None };

                    for caps in RE_METHOD_CALL.captures_iter(trimmed) {
                        let method = caps[2].to_string();
                        if !GO_KEYWORDS.contains(&method.as_str()) {
                            pending_edges.push((*fn_id, method, line_num, block));
                        }
                    }

                    for caps in RE_CALL.captures_iter(trimmed) {
                        let name = caps[1].to_string();
                        if !GO_KEYWORDS.contains(&name.as_str()) {
                            let m = caps.get(1).unwrap();
                            let is_method = m.start() > 0 && trimmed.as_bytes().get(m.start() - 1) == Some(&b'.');
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
        m.insert(PathBuf::from("main.go"), src.to_string());
        m
    }

    #[test]
    fn detects_functions_and_calls() {
        let src = "package main\n\nfunc main() {\n\thelper()\n}\n\nfunc helper() {\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        assert_eq!(g.node_count(), 2);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn detects_methods() {
        let src = "package main\n\ntype Server struct{}\n\nfunc (s *Server) Start() {\n\ts.listen()\n}\n\nfunc (s *Server) listen() {\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        assert_eq!(g.fns_named("Server.Start").len(), 1);
        assert_eq!(g.fns_named("Server.listen").len(), 1);
    }

    #[test]
    fn test_entry_points() {
        let src = "package main\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n}\n\nfunc BenchmarkAdd(b *testing.B) {\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        let test_fn = g.nodes.iter().find(|n| n.name == "TestAdd").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
        let bench_fn = g.nodes.iter().find(|n| n.name == "BenchmarkAdd").unwrap();
        assert_eq!(bench_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn main_entry_point() {
        let src = "package main\n\nfunc main() {\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        let main_fn = g.nodes.iter().find(|n| n.name == "main").unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn exported_functions_are_public_api() {
        let src = "package lib\n\nfunc Exported() {\n}\n\nfunc unexported() {\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        let exported = g.nodes.iter().find(|n| n.name == "Exported").unwrap();
        assert_eq!(exported.entry_kind, Some(EntryPointKind::PublicApi));
        let unexported = g.nodes.iter().find(|n| n.name == "unexported").unwrap();
        assert_eq!(unexported.entry_kind, None);
    }

    #[test]
    fn http_handler_detection() {
        let src = "package main\n\nimport \"net/http\"\n\nfunc Handler(w http.ResponseWriter, r *http.Request) {\n}\n\nfunc main() {\n\thttp.HandleFunc(\"/\", Handler)\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        let handler = g.nodes.iter().find(|n| n.name == "Handler").unwrap();
        assert_eq!(handler.entry_kind, Some(EntryPointKind::HttpHandler));
    }

    #[test]
    fn cross_file_resolution() {
        let mut sources = HashMap::new();
        sources.insert(PathBuf::from("a.go"), "package main\n\nfunc caller() {\n\thelper()\n}\n".to_string());
        sources.insert(PathBuf::from("b.go"), "package main\n\nfunc helper() {\n}\n".to_string());
        let g = GoExtractor.extract(&sources);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn empty_source() {
        let g = GoExtractor.extract(&HashMap::new());
        assert_eq!(g.node_count(), 0);
    }

    #[test]
    fn cli_entry_with_cobra() {
        let src = "package main\n\nimport \"github.com/spf13/cobra\"\n\nvar rootCmd = &cobra.Command{}\n\nfunc main() {\n\trootCmd.Execute()\n}\n";
        let g = GoExtractor.extract(&single_file(src));
        let main_fn = g.nodes.iter().find(|n| n.name == "main").unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::CliEntry));
    }
}
