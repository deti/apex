//! Architecture conformance checking via import graph analysis.
//!
//! Builds a module-level import graph from source files, classifies modules
//! into architectural layers, and detects dependency violations.

use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};

/// A directed edge in the import graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportEdge {
    /// The module that contains the import statement.
    pub from: String,
    /// The module being imported.
    pub to: String,
    /// Line number of the import statement.
    pub line: u32,
}

/// Module-level import/dependency graph.
#[derive(Debug, Clone, Default)]
pub struct ImportGraph {
    /// All modules found in the project.
    modules: HashSet<String>,
    /// Import edges: from -> list of edges.
    edges: HashMap<String, Vec<ImportEdge>>,
}

impl ImportGraph {
    pub fn new() -> Self {
        Default::default()
    }

    /// Build an import graph from source files.
    /// `sources` is a map of file path to file content.
    pub fn build(sources: &HashMap<String, String>) -> Self {
        let mut graph = ImportGraph::new();
        for (path, content) in sources {
            let module_name = Self::path_to_module(path);
            graph.modules.insert(module_name.clone());

            let imports = Self::extract_imports(content, path);
            for (imported, line) in imports {
                graph.add_edge(ImportEdge {
                    from: module_name.clone(),
                    to: imported,
                    line,
                });
            }
        }
        graph
    }

    /// Add an edge to the graph.
    pub fn add_edge(&mut self, edge: ImportEdge) {
        self.modules.insert(edge.from.clone());
        self.modules.insert(edge.to.clone());
        self.edges.entry(edge.from.clone()).or_default().push(edge);
    }

    /// Get all modules imported by `module`.
    pub fn imports_of(&self, module: &str) -> Vec<&ImportEdge> {
        self.edges
            .get(module)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get all modules that import `module`.
    pub fn dependents_of(&self, module: &str) -> Vec<&ImportEdge> {
        self.edges
            .values()
            .flat_map(|edges| edges.iter())
            .filter(|e| e.to == module)
            .collect()
    }

    /// BFS reachability: is `target` reachable from `source` via imports?
    pub fn is_reachable(&self, source: &str, target: &str) -> bool {
        if source == target {
            return true;
        }
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(source.to_string());
        visited.insert(source.to_string());

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = self.edges.get(&current) {
                for edge in edges {
                    if edge.to == target {
                        return true;
                    }
                    if visited.insert(edge.to.clone()) {
                        queue.push_back(edge.to.clone());
                    }
                }
            }
        }
        false
    }

    /// Get all modules in the graph.
    pub fn modules(&self) -> &HashSet<String> {
        &self.modules
    }

    /// Get all edges in the graph.
    pub fn all_edges(&self) -> Vec<&ImportEdge> {
        self.edges.values().flat_map(|v| v.iter()).collect()
    }

    /// Convert file path to module name.
    /// "src/utils/helpers.py" -> "utils.helpers"
    /// "src/main.rs" -> "main"
    fn path_to_module(path: &str) -> String {
        let p = path
            .trim_start_matches("src/")
            .trim_end_matches(".py")
            .trim_end_matches(".rs")
            .trim_end_matches(".js")
            .trim_end_matches(".ts");
        p.replace(['/', '\\'], ".")
    }

    /// Extract import statements from source code.
    /// Supports Python (`import x`, `from x import y`) and
    /// Rust (`use x::y`) style imports.
    fn extract_imports(content: &str, path: &str) -> Vec<(String, u32)> {
        let mut imports = Vec::new();
        let is_python = path.ends_with(".py");
        let is_rust = path.ends_with(".rs");

        for (line_idx, line) in content.lines().enumerate() {
            let line = line.trim();
            let line_num = (line_idx + 1) as u32;

            if is_python {
                // `import foo.bar` -> "foo.bar"
                if let Some(rest) = line.strip_prefix("import ") {
                    let module = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .split(" as ")
                        .next()
                        .unwrap_or("");
                    if !module.is_empty() {
                        imports.push((module.to_string(), line_num));
                    }
                }
                // `from foo.bar import baz` -> "foo.bar"
                if let Some(rest) = line.strip_prefix("from ") {
                    if let Some(module) = rest.split(" import ").next() {
                        let module = module.trim();
                        if !module.is_empty() {
                            imports.push((module.to_string(), line_num));
                        }
                    }
                }
            }

            if is_rust {
                // `use foo::bar::baz;` -> "foo.bar"
                if let Some(rest) = line.strip_prefix("use ") {
                    let path_str = rest
                        .trim_end_matches(';')
                        .split("::")
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(".");
                    if !path_str.is_empty() {
                        imports.push((path_str, line_num));
                    }
                }
            }
        }
        imports
    }
}

/// A forbidden dependency rule.
#[derive(Debug, Clone, Deserialize)]
pub struct ForbiddenDep {
    pub from: String,
    pub to: String,
}

/// Configuration for architecture conformance rules.
#[derive(Debug, Clone, Deserialize)]
pub struct ArchitectureConfig {
    /// Module prefix -> layer mapping.
    pub layers: HashMap<String, String>,
    /// Forbidden dependencies: from_layer -> to_layer.
    pub forbidden: Vec<ForbiddenDep>,
}

impl ArchitectureConfig {
    /// Parse from TOML string.
    pub fn from_toml(toml_str: &str) -> Result<Self, String> {
        toml::from_str(toml_str).map_err(|e| format!("failed to parse architecture config: {e}"))
    }
}

/// A dependency violation.
#[derive(Debug, Clone)]
pub struct ArchViolation {
    pub from_module: String,
    pub to_module: String,
    pub from_layer: String,
    pub to_layer: String,
    pub line: u32,
    pub message: String,
}

/// Classify a module name into an architecture layer based on config.
pub fn classify_module(module: &str, config: &ArchitectureConfig) -> String {
    let mut best_match = ("unknown".to_string(), 0usize);
    for (prefix, layer) in &config.layers {
        if module.starts_with(prefix) && prefix.len() > best_match.1 {
            best_match = (layer.clone(), prefix.len());
        }
    }
    best_match.0
}

/// Check all import edges against architecture rules.
pub fn check_violations(graph: &ImportGraph, config: &ArchitectureConfig) -> Vec<ArchViolation> {
    let mut violations = Vec::new();
    for edge in graph.all_edges() {
        let from_layer = classify_module(&edge.from, config);
        let to_layer = classify_module(&edge.to, config);

        for dep in &config.forbidden {
            if from_layer == dep.from && to_layer == dep.to {
                violations.push(ArchViolation {
                    from_module: edge.from.clone(),
                    to_module: edge.to.clone(),
                    from_layer: from_layer.clone(),
                    to_layer: to_layer.clone(),
                    line: edge.line,
                    message: format!(
                        "{} ({}) must not depend on {} ({})",
                        edge.from, from_layer, edge.to, to_layer
                    ),
                });
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_graph_empty() {
        let graph = ImportGraph::new();
        assert!(graph.modules().is_empty());
        assert!(graph.all_edges().is_empty());
    }

    #[test]
    fn import_graph_add_edge() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "b".into(),
            line: 1,
        });
        assert!(graph.modules().contains("a"));
        assert!(graph.modules().contains("b"));
        assert_eq!(graph.all_edges().len(), 1);
    }

    #[test]
    fn imports_of_returns_edges() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "b".into(),
            line: 1,
        });
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "c".into(),
            line: 2,
        });
        let imports = graph.imports_of("a");
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn imports_of_unknown_module() {
        let graph = ImportGraph::new();
        assert!(graph.imports_of("nonexistent").is_empty());
    }

    #[test]
    fn dependents_of_returns_reverse() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "shared".into(),
            line: 1,
        });
        graph.add_edge(ImportEdge {
            from: "b".into(),
            to: "shared".into(),
            line: 2,
        });
        let deps = graph.dependents_of("shared");
        assert_eq!(deps.len(), 2);
    }

    #[test]
    fn is_reachable_direct() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "b".into(),
            line: 1,
        });
        assert!(graph.is_reachable("a", "b"));
    }

    #[test]
    fn is_reachable_transitive() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "b".into(),
            line: 1,
        });
        graph.add_edge(ImportEdge {
            from: "b".into(),
            to: "c".into(),
            line: 2,
        });
        assert!(graph.is_reachable("a", "c"));
    }

    #[test]
    fn is_reachable_not_connected() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "a".into(),
            to: "b".into(),
            line: 1,
        });
        graph.add_edge(ImportEdge {
            from: "c".into(),
            to: "d".into(),
            line: 2,
        });
        assert!(!graph.is_reachable("a", "c"));
        assert!(!graph.is_reachable("a", "d"));
    }

    #[test]
    fn is_reachable_self() {
        let graph = ImportGraph::new();
        assert!(graph.is_reachable("a", "a"));
    }

    #[test]
    fn build_from_python_sources() {
        let mut sources = HashMap::new();
        sources.insert(
            "src/app/views.py".to_string(),
            "import os\nfrom app.models import User\n".to_string(),
        );
        let graph = ImportGraph::build(&sources);
        let imports = graph.imports_of("app.views");
        assert_eq!(imports.len(), 2);
        let targets: Vec<&str> = imports.iter().map(|e| e.to.as_str()).collect();
        assert!(targets.contains(&"os"));
        assert!(targets.contains(&"app.models"));
    }

    #[test]
    fn build_from_rust_sources() {
        let mut sources = HashMap::new();
        sources.insert(
            "src/handler.rs".to_string(),
            "use std::collections;\nuse crate::model;\n".to_string(),
        );
        let graph = ImportGraph::build(&sources);
        let imports = graph.imports_of("handler");
        assert_eq!(imports.len(), 2);
        let targets: Vec<&str> = imports.iter().map(|e| e.to.as_str()).collect();
        assert!(targets.contains(&"std.collections"));
        assert!(targets.contains(&"crate.model"));
    }

    #[test]
    fn path_to_module_python() {
        assert_eq!(ImportGraph::path_to_module("src/utils/helpers.py"), "utils.helpers");
    }

    #[test]
    fn path_to_module_rust() {
        assert_eq!(ImportGraph::path_to_module("src/main.rs"), "main");
    }

    #[test]
    fn classify_module_known_layer() {
        let config = ArchitectureConfig {
            layers: HashMap::from([("api.".to_string(), "presentation".to_string())]),
            forbidden: vec![],
        };
        assert_eq!(classify_module("api.views", &config), "presentation");
    }

    #[test]
    fn classify_module_unknown() {
        let config = ArchitectureConfig {
            layers: HashMap::new(),
            forbidden: vec![],
        };
        assert_eq!(classify_module("random.stuff", &config), "unknown");
    }

    #[test]
    fn classify_module_longest_prefix() {
        let config = ArchitectureConfig {
            layers: HashMap::from([
                ("app.".to_string(), "application".to_string()),
                ("app.domain.".to_string(), "domain".to_string()),
            ]),
            forbidden: vec![],
        };
        assert_eq!(classify_module("app.domain.entities", &config), "domain");
        assert_eq!(classify_module("app.services", &config), "application");
    }

    #[test]
    fn check_violations_found() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "domain.entities".into(),
            to: "infra.database".into(),
            line: 5,
        });
        let config = ArchitectureConfig {
            layers: HashMap::from([
                ("domain.".to_string(), "domain".to_string()),
                ("infra.".to_string(), "infrastructure".to_string()),
            ]),
            forbidden: vec![ForbiddenDep {
                from: "domain".to_string(),
                to: "infrastructure".to_string(),
            }],
        };
        let violations = check_violations(&graph, &config);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].from_module, "domain.entities");
        assert_eq!(violations[0].to_module, "infra.database");
        assert_eq!(violations[0].line, 5);
    }

    #[test]
    fn check_violations_clean() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "app.services".into(),
            to: "domain.entities".into(),
            line: 3,
        });
        let config = ArchitectureConfig {
            layers: HashMap::from([
                ("app.".to_string(), "application".to_string()),
                ("domain.".to_string(), "domain".to_string()),
            ]),
            forbidden: vec![ForbiddenDep {
                from: "domain".to_string(),
                to: "infrastructure".to_string(),
            }],
        };
        let violations = check_violations(&graph, &config);
        assert!(violations.is_empty());
    }

    #[test]
    fn architecture_config_from_toml() {
        let toml_str = r#"
[layers]
"api." = "presentation"
"domain." = "domain"
"infra." = "infrastructure"

[[forbidden]]
from = "domain"
to = "infrastructure"

[[forbidden]]
from = "domain"
to = "presentation"
"#;
        let config = ArchitectureConfig::from_toml(toml_str).unwrap();
        assert_eq!(config.layers.len(), 3);
        assert_eq!(config.forbidden.len(), 2);
        assert_eq!(config.forbidden[0].from, "domain");
        assert_eq!(config.forbidden[0].to, "infrastructure");
    }

    #[test]
    fn build_graph_from_multiple_files() {
        let mut sources = HashMap::new();
        sources.insert(
            "src/api/views.py".to_string(),
            "from app.services import UserService\n".to_string(),
        );
        sources.insert(
            "src/app/services.py".to_string(),
            "from domain.models import User\n".to_string(),
        );
        sources.insert(
            "src/domain/models.py".to_string(),
            "import dataclasses\n".to_string(),
        );

        let graph = ImportGraph::build(&sources);

        // 3 source modules + their imports
        assert!(graph.modules().contains("api.views"));
        assert!(graph.modules().contains("app.services"));
        assert!(graph.modules().contains("domain.models"));

        // Verify edges
        assert_eq!(graph.imports_of("api.views").len(), 1);
        assert_eq!(graph.imports_of("api.views")[0].to, "app.services");
        assert_eq!(graph.imports_of("app.services")[0].to, "domain.models");
        assert_eq!(graph.imports_of("domain.models")[0].to, "dataclasses");

        // Transitive reachability
        assert!(graph.is_reachable("api.views", "domain.models"));
        assert!(!graph.is_reachable("domain.models", "api.views"));
    }
}
