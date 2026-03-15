use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub struct RubyExtractor;

static RE_DEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*def\s+(\w+)").unwrap());
static RE_CLASS_DEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*class\s+(\w+)").unwrap());
static RE_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\w+)\s*\(").unwrap());
static RE_DESCRIBE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(describe|context|it)\s").unwrap());
static RE_ROUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*(get|post|put|patch|delete)\s+['"\/]"#).unwrap());

const RUBY_KEYWORDS: &[&str] = &[
    "if", "else", "elsif", "unless", "while", "until", "for", "do", "begin",
    "rescue", "ensure", "end", "def", "class", "module", "return", "yield",
    "raise", "require", "include", "extend", "attr_reader", "attr_writer",
    "attr_accessor", "puts", "print", "p", "pp", "nil", "true", "false",
    "self", "super", "new", "lambda", "proc",
];

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_test_file(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("spec/") || s.contains("test/") || s.ends_with("_spec.rb") || s.ends_with("_test.rb")
}

impl CallGraphExtractor for RubyExtractor {
    fn language(&self) -> Language { Language::Ruby }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        let mut fn_index: HashMap<String, Vec<FnId>> = HashMap::new();
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();
            let is_sinatra = source.contains("Sinatra") || source.contains("sinatra");
            let is_rails_controller = source.contains("< ApplicationController") || source.contains("< ActionController");
            let has_optparse = source.contains("OptionParser");
            let is_spec = is_test_file(path);

            let mut current_fn: Option<(FnId, usize)> = None; // (id, indent)
            let mut block_id: u32 = 0;

            for (i, &line) in lines.iter().enumerate() {
                let line_num = (i + 1) as u32;
                let trimmed = line.trim();
                let indent = indent_of(line);

                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }

                // Close function if indent returns to or below function's indent
                if let Some((fn_id, fn_indent)) = &current_fn {
                    if indent <= *fn_indent && trimmed == "end" {
                        if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == *fn_id) {
                            n.end_line = line_num;
                        }
                        current_fn = None;
                    }
                }

                // RSpec/describe/it blocks
                if RE_DESCRIBE.is_match(trimmed) && is_spec {
                    let name = trimmed.split_whitespace().nth(1).unwrap_or("test").trim_matches(|c: char| !c.is_alphanumeric()).to_string();
                    let id = FnId(next_id);
                    next_id += 1;
                    graph.nodes.push(FnNode {
                        id, name: name.clone(), file: path.clone(),
                        start_line: line_num, end_line: line_num,
                        entry_kind: Some(EntryPointKind::Test),
                    });
                    fn_index.entry(name).or_default().push(id);
                    continue;
                }

                // Sinatra routes
                if is_sinatra && RE_ROUTE.is_match(trimmed) {
                    let method = trimmed.split_whitespace().next().unwrap_or("get");
                    let route_path = trimmed.split_whitespace().nth(1).unwrap_or("/").trim_matches(|c: char| c == '\'' || c == '"');
                    let name = format!("{}_{}", method, route_path.replace('/', "_").trim_matches('_'));
                    let id = FnId(next_id);
                    next_id += 1;
                    graph.nodes.push(FnNode {
                        id, name: name.clone(), file: path.clone(),
                        start_line: line_num, end_line: line_num,
                        entry_kind: Some(EntryPointKind::HttpHandler),
                    });
                    fn_index.entry(name).or_default().push(id);
                    continue;
                }

                // Method definition
                if let Some(caps) = RE_DEF.captures(trimmed) {
                    let name = caps[1].to_string();
                    let entry_kind = if name.starts_with("test_") {
                        Some(EntryPointKind::Test)
                    } else if is_rails_controller && !name.starts_with('_') {
                        Some(EntryPointKind::HttpHandler)
                    } else if has_optparse && name == "run" {
                        Some(EntryPointKind::CliEntry)
                    } else {
                        None
                    };

                    let id = FnId(next_id);
                    next_id += 1;
                    block_id = 0;
                    graph.nodes.push(FnNode {
                        id, name: name.clone(), file: path.clone(),
                        start_line: line_num, end_line: line_num,
                        entry_kind,
                    });
                    fn_index.entry(name).or_default().push(id);
                    current_fn = Some((id, indent));
                    continue;
                }

                // Extract calls inside functions
                if let Some((fn_id, _)) = &current_fn {
                    if trimmed.starts_with("if ") || trimmed.starts_with("unless ") || trimmed.starts_with("while ") || trimmed.starts_with("case ") {
                        block_id += 1;
                    }
                    let block = if block_id > 0 { Some(block_id) } else { None };

                    for caps in RE_CALL.captures_iter(trimmed) {
                        let name = caps[1].to_string();
                        if !RUBY_KEYWORDS.contains(&name.as_str()) {
                            pending_edges.push((*fn_id, name, line_num, block));
                        }
                    }
                }
            }
        }

        // Resolve edges
        for (caller_id, callee_name, line, block) in pending_edges {
            if let Some(ids) = fn_index.get(&callee_name) {
                for &callee_id in ids {
                    if callee_id != caller_id {
                        graph.edges.push(CallEdge {
                            caller: caller_id, callee: callee_id,
                            call_site_line: line, call_site_block: block,
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

    fn single_file(name: &str, src: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), src.to_string());
        m
    }

    #[test]
    fn detects_methods_and_calls() {
        let src = "def greet\n  puts('hi')\nend\n\ndef main\n  greet()\nend\n";
        let g = RubyExtractor.extract(&single_file("app.rb", src));
        assert_eq!(g.node_count(), 2);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn test_prefix_entry_point() {
        let src = "def test_addition\n  assert_equal 2, 1 + 1\nend\n";
        let g = RubyExtractor.extract(&single_file("test/test_math.rb", src));
        let test_fn = g.nodes.iter().find(|n| n.name == "test_addition").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn rspec_describe_entry_point() {
        let src = "describe 'Math' do\n  it 'adds' do\n    expect(1+1).to eq(2)\n  end\nend\n";
        let g = RubyExtractor.extract(&single_file("spec/math_spec.rb", src));
        assert!(!g.nodes.is_empty());
        assert!(g.nodes.iter().any(|n| n.entry_kind == Some(EntryPointKind::Test)));
    }

    #[test]
    fn sinatra_routes() {
        let src = "require 'sinatra'\n\nget '/hello' do\n  'world'\nend\n\npost '/data' do\n  'ok'\nend\n";
        let g = RubyExtractor.extract(&single_file("app.rb", src));
        assert!(g.nodes.iter().any(|n| n.entry_kind == Some(EntryPointKind::HttpHandler)));
    }

    #[test]
    fn rails_controller() {
        let src = "class UsersController < ApplicationController\n  def index\n    render json: users\n  end\n\n  def show\n    find_user\n  end\nend\n";
        let g = RubyExtractor.extract(&single_file("app/controllers/users_controller.rb", src));
        let index_fn = g.nodes.iter().find(|n| n.name == "index").unwrap();
        assert_eq!(index_fn.entry_kind, Some(EntryPointKind::HttpHandler));
    }

    #[test]
    fn cross_file_resolution() {
        let mut sources = HashMap::new();
        sources.insert(PathBuf::from("a.rb"), "def caller_fn\n  helper()\nend\n".to_string());
        sources.insert(PathBuf::from("b.rb"), "def helper\nend\n".to_string());
        let g = RubyExtractor.extract(&sources);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn empty_source() {
        let g = RubyExtractor.extract(&HashMap::new());
        assert_eq!(g.node_count(), 0);
    }
}
