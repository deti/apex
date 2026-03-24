//! Tree-sitter unified probe instrumentation for Python, JavaScript, and Go.
//!
//! Instead of relying on per-language coverage tools (coverage.py, Istanbul,
//! JaCoCo), this module uses tree-sitter to insert coverage probes directly
//! into source code — giving a unified approach across all supported languages.
//!
//! # Probe strategy
//! - **Python**: inserts `__apex_probe(ID)` calls at function entries, branch
//!   points, and loop entries.
//! - **JavaScript**: inserts `__apexProbe(ID)` calls at the same sites.
//! - **Go**: inserts `apexProbe(ID)` calls (callers must also inject the tiny
//!   runtime import).
//!
//! # Runtime helpers
//! A tiny per-language probe runtime must be injected alongside the instrumented
//! source before execution:
//! - Python: `_apex_tracer.py` (see [`PYTHON_PROBE_RUNTIME`])
//! - JavaScript: `_apex_tracer.js` (see [`JS_PROBE_RUNTIME`])

use apex_core::{error::Result, types::Language};
use std::fmt;
use tree_sitter::Parser;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Probe kind — what kind of control-flow point a probe guards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeKind {
    FunctionEntry,
    BranchTrue,
    BranchFalse,
    LoopEntry,
}

impl fmt::Display for ProbeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeKind::FunctionEntry => write!(f, "FunctionEntry"),
            ProbeKind::BranchTrue => write!(f, "BranchTrue"),
            ProbeKind::BranchFalse => write!(f, "BranchFalse"),
            ProbeKind::LoopEntry => write!(f, "LoopEntry"),
        }
    }
}

/// Metadata for a single inserted probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeInfo {
    pub id: u32,
    pub line: u32,
    pub kind: ProbeKind,
}

/// Result of instrumenting a source file.
pub struct InstrumentedSource {
    pub source: String,
    pub probes: Vec<ProbeInfo>,
}

// ---------------------------------------------------------------------------
// Probe runtimes (injected alongside instrumented source)
// ---------------------------------------------------------------------------

/// Tiny Python probe runtime.  Inject this as `_apex_tracer.py` next to the
/// instrumented source before running it.
pub const PYTHON_PROBE_RUNTIME: &str = r#"# _apex_tracer.py — APEX probe runtime (auto-generated, do not edit)
import json, atexit, os as _os
_hits = set()
def __apex_probe(probe_id):
    _hits.add(probe_id)
def _apex_flush():
    out = _os.environ.get("APEX_PROBE_OUT", ".apex_probe_hits.json")
    with open(out, "w") as _f:
        json.dump(sorted(_hits), _f)
atexit.register(_apex_flush)
"#;

/// Tiny JavaScript probe runtime.  Inject this as `_apex_tracer.js` (or
/// `require`/`import` it) before the instrumented source runs.
pub const JS_PROBE_RUNTIME: &str = r#"// _apex_tracer.js — APEX probe runtime (auto-generated, do not edit)
const _apexHits = new Set();
global.__apexProbe = (id) => _apexHits.add(id);
process.on('exit', () => {
  const fs = require('fs');
  const out = process.env.APEX_PROBE_OUT || '.apex_probe_hits.json';
  fs.writeFileSync(out, JSON.stringify([..._apexHits].sort((a,b)=>a-b)));
});
"#;

/// Tiny Go probe runtime snippet.  Paste this into a `_apex_tracer.go` file
/// in the same package, replacing `PACKAGE` with the actual package name.
pub const GO_PROBE_RUNTIME: &str = r#"// _apex_tracer.go — APEX probe runtime (auto-generated, do not edit)
package PACKAGE

import (
	"encoding/json"
	"os"
	"runtime"
	"sort"
	"sync"
)

var (
	_apexMu   sync.Mutex
	_apexHits = map[uint32]struct{}{}
)

func apexProbe(id uint32) {
	_apexMu.Lock()
	_apexHits[id] = struct{}{}
	_apexMu.Unlock()
}

func init() {
	runtime.SetFinalizer(&_apexHits, func(_ *map[uint32]struct{}) {
		apexFlush()
	})
}

func apexFlush() {
	_apexMu.Lock()
	ids := make([]int, 0, len(_apexHits))
	for id := range _apexHits {
		ids = append(ids, int(id))
	}
	_apexMu.Unlock()
	sort.Ints(ids)
	out := os.Getenv("APEX_PROBE_OUT")
	if out == "" {
		out = ".apex_probe_hits.json"
	}
	data, _ := json.Marshal(ids)
	_ = os.WriteFile(out, data, 0o644)
}
"#;

// ---------------------------------------------------------------------------
// Instrumentor
// ---------------------------------------------------------------------------

/// Tree-sitter-based source instrumentor.
///
/// Parses source with tree-sitter and inserts lightweight probe calls at every
/// function entry, branch point (if/else bodies), and loop entry.
pub struct TreeSitterInstrumentor {
    language: Language,
}

impl TreeSitterInstrumentor {
    /// Create a new instrumentor for the given language.
    ///
    /// Returns `None` if the language is not yet supported by the tree-sitter
    /// backend (only Python, JavaScript, and Go are currently supported).
    pub fn new(language: Language) -> Option<Self> {
        match language {
            Language::Python | Language::JavaScript | Language::Go => {
                Some(TreeSitterInstrumentor { language })
            }
            _ => None,
        }
    }

    /// Insert coverage probes into `source`.
    ///
    /// Returns an [`InstrumentedSource`] containing the modified source text
    /// and the probe metadata so callers can map probe IDs back to locations.
    ///
    /// On parse failure tree-sitter recovers as best it can; we return the
    /// original source with zero probes rather than an error.
    pub fn instrument_source(&self, source: &str, filename: &str) -> Result<InstrumentedSource> {
        match self.language {
            Language::Python => instrument_python(source, filename),
            Language::JavaScript => instrument_javascript(source, filename),
            Language::Go => instrument_go(source, filename),
            _ => Ok(InstrumentedSource {
                source: source.to_owned(),
                probes: Vec::new(),
            }),
        }
    }

    /// The language this instrumentor was configured for.
    pub fn language(&self) -> Language {
        self.language
    }
}

// ---------------------------------------------------------------------------
// Probe-collection helpers shared across languages
// ---------------------------------------------------------------------------

/// A pending probe insertion.  We collect all of these before modifying the
/// source so that byte offsets remain valid while we walk the tree.
#[derive(Debug)]
struct PendingProbe {
    /// Byte offset in the original source where the probe call is inserted
    /// (immediately after the opening `{` / `:` of the block).
    insert_at: usize,
    /// Source line (1-based) of the probe point.
    line: u32,
    kind: ProbeKind,
}

/// Insert `pending` probes into `source` and return the modified string along
/// with the finalised `ProbeInfo` vec.
///
/// Probes are inserted from last-to-first so that earlier byte offsets remain
/// valid after each splice.
fn apply_probes(
    source: &str,
    mut pending: Vec<PendingProbe>,
    probe_call: fn(u32) -> String,
) -> InstrumentedSource {
    // Sort in reverse order of insertion point so splices don't shift earlier
    // offsets.
    pending.sort_by(|a, b| b.insert_at.cmp(&a.insert_at));

    let mut src = source.to_owned();
    let mut probes: Vec<ProbeInfo> = Vec::with_capacity(pending.len());

    for (idx, p) in pending.iter().enumerate() {
        let id = idx as u32;
        let call = probe_call(id);
        if p.insert_at <= src.len() {
            src.insert_str(p.insert_at, &call);
        }
        probes.push(ProbeInfo {
            id,
            line: p.line,
            kind: p.kind.clone(),
        });
    }

    // Reverse probes so IDs are in source order (ascending insert_at).
    probes.reverse();
    // Re-number IDs to match final order.
    for (i, p) in probes.iter_mut().enumerate() {
        p.id = i as u32;
    }

    InstrumentedSource {
        source: src,
        probes,
    }
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

fn instrument_python(source: &str, _filename: &str) -> Result<InstrumentedSource> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("tree-sitter Python language");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => {
            return Ok(InstrumentedSource {
                source: source.to_owned(),
                probes: Vec::new(),
            });
        }
    };

    let src_bytes = source.as_bytes();
    let mut pending: Vec<PendingProbe> = Vec::new();
    collect_python_probes(&tree.root_node(), src_bytes, &mut pending);

    fn py_probe(id: u32) -> String {
        format!("__apex_probe({id})\n")
    }

    Ok(apply_probes(source, pending, py_probe))
}

fn collect_python_probes(node: &tree_sitter::Node, src: &[u8], pending: &mut Vec<PendingProbe>) {
    let kind = node.kind();

    match kind {
        // function_definition: probe at first byte after the ':'
        "function_definition" => {
            if let Some(body) = node.child_by_field_name("body") {
                let insert_at = probe_after_colon_python(&body, src);
                let line = node.start_position().row as u32 + 1;
                pending.push(PendingProbe {
                    insert_at,
                    line,
                    kind: ProbeKind::FunctionEntry,
                });
            }
        }
        // if_statement: probe at start of consequence (true branch)
        "if_statement" => {
            let line = node.start_position().row as u32 + 1;
            // consequence = the "if" body block
            if let Some(consequence) = node.child_by_field_name("consequence") {
                let insert_at = probe_after_colon_python(&consequence, src);
                pending.push(PendingProbe {
                    insert_at,
                    line,
                    kind: ProbeKind::BranchTrue,
                });
            }
            // alternative = elif / else block
            if let Some(alt) = node.child_by_field_name("alternative") {
                let alt_line = alt.start_position().row as u32 + 1;
                // elif_clause has a "consequence" child; else_clause has a "body"
                let body_node = alt
                    .child_by_field_name("consequence")
                    .or_else(|| alt.child_by_field_name("body"))
                    .unwrap_or(alt);
                let insert_at = probe_after_colon_python(&body_node, src);
                pending.push(PendingProbe {
                    insert_at,
                    line: alt_line,
                    kind: ProbeKind::BranchFalse,
                });
            }
        }
        // for_statement / while_statement: probe at loop body
        "for_statement" | "while_statement" => {
            let line = node.start_position().row as u32 + 1;
            if let Some(body) = node.child_by_field_name("body") {
                let insert_at = probe_after_colon_python(&body, src);
                pending.push(PendingProbe {
                    insert_at,
                    line,
                    kind: ProbeKind::LoopEntry,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_probes(&child, src, pending);
    }
}

/// Return the byte offset immediately after the newline that follows the `:` at
/// the end of a Python compound-statement header.  This is where we want to
/// insert a probe so it runs as the first statement of the block.
///
/// If the block is a `block` node (indented suite), we insert after the first
/// newline inside it.  Otherwise we fall back to the node's start byte.
fn probe_after_colon_python(node: &tree_sitter::Node, src: &[u8]) -> usize {
    let start = node.start_byte();
    // Find the first '\n' at or after `start` and insert just after it.
    let slice = &src[start..];
    if let Some(nl) = slice.iter().position(|&b| b == b'\n') {
        start + nl + 1
    } else {
        start
    }
}

// ---------------------------------------------------------------------------
// JavaScript
// ---------------------------------------------------------------------------

fn instrument_javascript(source: &str, _filename: &str) -> Result<InstrumentedSource> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("tree-sitter JavaScript language");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => {
            return Ok(InstrumentedSource {
                source: source.to_owned(),
                probes: Vec::new(),
            });
        }
    };

    let src_bytes = source.as_bytes();
    let mut pending: Vec<PendingProbe> = Vec::new();
    collect_js_probes(&tree.root_node(), src_bytes, &mut pending);

    fn js_probe(id: u32) -> String {
        format!("__apexProbe({id});")
    }

    Ok(apply_probes(source, pending, js_probe))
}

fn collect_js_probes(node: &tree_sitter::Node, src: &[u8], pending: &mut Vec<PendingProbe>) {
    let kind = node.kind();

    match kind {
        // function_declaration, arrow_function, function_expression, method_definition
        "function_declaration"
        | "function_expression"
        | "arrow_function"
        | "generator_function_declaration"
        | "generator_function"
        | "method_definition" => {
            let line = node.start_position().row as u32 + 1;
            // body is either a statement_block or an expression (arrow fn)
            if let Some(body) = node.child_by_field_name("body") {
                if let Some(insert_at) = probe_inside_block_js(&body, src) {
                    pending.push(PendingProbe {
                        insert_at,
                        line,
                        kind: ProbeKind::FunctionEntry,
                    });
                }
            }
        }
        // if_statement: consequence = true branch, alternative = false
        "if_statement" => {
            let line = node.start_position().row as u32 + 1;
            if let Some(consequence) = node.child_by_field_name("consequence") {
                if let Some(insert_at) = probe_inside_block_js(&consequence, src) {
                    pending.push(PendingProbe {
                        insert_at,
                        line,
                        kind: ProbeKind::BranchTrue,
                    });
                }
            }
            if let Some(alternative) = node.child_by_field_name("alternative") {
                let alt_line = alternative.start_position().row as u32 + 1;
                // else body — use the inner block if it's an else_clause
                let insert_at = if alternative.kind() == "else_clause" {
                    alternative
                        .child_by_field_name("body")
                        .and_then(|b| probe_inside_block_js(&b, src))
                        .or_else(|| probe_inside_block_js(&alternative, src))
                } else {
                    probe_inside_block_js(&alternative, src)
                };
                if let Some(insert_at) = insert_at {
                    pending.push(PendingProbe {
                        insert_at,
                        line: alt_line,
                        kind: ProbeKind::BranchFalse,
                    });
                }
            }
        }
        // for_statement, while_statement, for_in_statement, do_statement
        "for_statement" | "while_statement" | "for_in_statement" | "do_statement" => {
            let line = node.start_position().row as u32 + 1;
            if let Some(body) = node.child_by_field_name("body") {
                if let Some(insert_at) = probe_inside_block_js(&body, src) {
                    pending.push(PendingProbe {
                        insert_at,
                        line,
                        kind: ProbeKind::LoopEntry,
                    });
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_js_probes(&child, src, pending);
    }
}

/// Return the byte offset just after the opening `{` of a `statement_block`
/// node (i.e., right where we want to insert the probe call).
/// Returns `None` for arrow functions with expression bodies.
fn probe_inside_block_js(node: &tree_sitter::Node, src: &[u8]) -> Option<usize> {
    if node.kind() == "statement_block" {
        // First child is `{`; insert just after it.
        let open_brace_end = node.start_byte() + 1;
        let _ = src; // suppress unused warning
        Some(open_brace_end)
    } else {
        // Expression body (arrow fn) — insert before the expression start.
        Some(node.start_byte())
    }
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

fn instrument_go(source: &str, _filename: &str) -> Result<InstrumentedSource> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .expect("tree-sitter Go language");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => {
            return Ok(InstrumentedSource {
                source: source.to_owned(),
                probes: Vec::new(),
            });
        }
    };

    let src_bytes = source.as_bytes();
    let mut pending: Vec<PendingProbe> = Vec::new();
    collect_go_probes(&tree.root_node(), src_bytes, &mut pending);

    fn go_probe(id: u32) -> String {
        format!("apexProbe({id});")
    }

    Ok(apply_probes(source, pending, go_probe))
}

fn collect_go_probes(node: &tree_sitter::Node, src: &[u8], pending: &mut Vec<PendingProbe>) {
    let kind = node.kind();

    match kind {
        "function_declaration" | "method_declaration" | "func_literal" => {
            let line = node.start_position().row as u32 + 1;
            if let Some(body) = node.child_by_field_name("body") {
                if let Some(insert_at) = probe_inside_block_go(&body, src) {
                    pending.push(PendingProbe {
                        insert_at,
                        line,
                        kind: ProbeKind::FunctionEntry,
                    });
                }
            }
        }
        "if_statement" => {
            let line = node.start_position().row as u32 + 1;
            // consequence field holds the block
            if let Some(consequence) = node.child_by_field_name("consequence") {
                if let Some(insert_at) = probe_inside_block_go(&consequence, src) {
                    pending.push(PendingProbe {
                        insert_at,
                        line,
                        kind: ProbeKind::BranchTrue,
                    });
                }
            }
            if let Some(alternative) = node.child_by_field_name("alternative") {
                let alt_line = alternative.start_position().row as u32 + 1;
                // else body block — try "body" field first, then the alt node itself
                let insert_at = alternative
                    .child_by_field_name("body")
                    .and_then(|b| probe_inside_block_go(&b, src))
                    .or_else(|| probe_inside_block_go(&alternative, src));
                if let Some(insert_at) = insert_at {
                    pending.push(PendingProbe {
                        insert_at,
                        line: alt_line,
                        kind: ProbeKind::BranchFalse,
                    });
                }
            }
        }
        "for_statement" => {
            let line = node.start_position().row as u32 + 1;
            if let Some(body) = node.child_by_field_name("body") {
                if let Some(insert_at) = probe_inside_block_go(&body, src) {
                    pending.push(PendingProbe {
                        insert_at,
                        line,
                        kind: ProbeKind::LoopEntry,
                    });
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_go_probes(&child, src, pending);
    }
}

/// Return byte offset just after the opening `{` of a Go block node.
fn probe_inside_block_go(node: &tree_sitter::Node, _src: &[u8]) -> Option<usize> {
    if node.kind() == "block" {
        Some(node.start_byte() + 1)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // TreeSitterInstrumentor::new
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_supported_languages() {
        assert!(TreeSitterInstrumentor::new(Language::Python).is_some());
        assert!(TreeSitterInstrumentor::new(Language::JavaScript).is_some());
        assert!(TreeSitterInstrumentor::new(Language::Go).is_some());
    }

    #[test]
    fn test_new_unsupported_language() {
        assert!(TreeSitterInstrumentor::new(Language::Java).is_none());
        assert!(TreeSitterInstrumentor::new(Language::Rust).is_none());
    }

    #[test]
    fn test_language_accessor() {
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        assert_eq!(inst.language(), Language::Python);
    }

    // -----------------------------------------------------------------------
    // Python instrumentation
    // -----------------------------------------------------------------------

    #[test]
    fn test_python_function_entry_probe() {
        let src = "def foo():\n    return 1\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "foo.py").unwrap();

        assert!(!result.probes.is_empty(), "expected at least one probe");
        let fn_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::FunctionEntry)
            .collect();
        assert!(!fn_probes.is_empty(), "expected a FunctionEntry probe");
        assert!(
            result.source.contains("__apex_probe("),
            "instrumented source must contain probe calls"
        );
    }

    #[test]
    fn test_python_if_else_probes() {
        let src = "def check(x):\n    if x > 0:\n        return 1\n    else:\n        return -1\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "check.py").unwrap();

        let true_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::BranchTrue)
            .collect();
        let false_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::BranchFalse)
            .collect();

        assert!(!true_probes.is_empty(), "expected BranchTrue probe");
        assert!(!false_probes.is_empty(), "expected BranchFalse probe");
        assert!(result.source.contains("__apex_probe("));
    }

    #[test]
    fn test_python_loop_probe() {
        let src = "def loop():\n    for i in range(10):\n        pass\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "loop.py").unwrap();

        let loop_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::LoopEntry)
            .collect();
        assert!(!loop_probes.is_empty(), "expected LoopEntry probe");
    }

    #[test]
    fn test_python_no_branches_no_branch_probes() {
        let src = "x = 1\ny = 2\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "plain.py").unwrap();

        let branch_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::BranchTrue || p.kind == ProbeKind::BranchFalse)
            .collect();
        assert!(
            branch_probes.is_empty(),
            "plain assignments should have no branch probes"
        );
    }

    #[test]
    fn test_python_empty_source_no_probes() {
        let src = "";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "empty.py").unwrap();
        assert!(result.probes.is_empty());
        assert_eq!(result.source, "");
    }

    #[test]
    fn test_python_probe_ids_sequential() {
        let src = "def a():\n    pass\ndef b():\n    pass\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "two_fns.py").unwrap();
        for (i, p) in result.probes.iter().enumerate() {
            assert_eq!(p.id, i as u32, "probe IDs must be sequential");
        }
    }

    #[test]
    fn test_python_probe_line_numbers() {
        let src = "def foo():\n    return 0\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "foo.py").unwrap();
        for p in &result.probes {
            assert!(p.line >= 1, "line numbers must be 1-based");
        }
    }

    #[test]
    fn test_python_malformed_source_graceful() {
        // tree-sitter recovers from syntax errors; we should get a result not a panic
        let src = "def foo(\n    !!!syntax error\n";
        let inst = TreeSitterInstrumentor::new(Language::Python).unwrap();
        let result = inst.instrument_source(src, "bad.py");
        assert!(result.is_ok(), "malformed source must not return Err");
    }

    // -----------------------------------------------------------------------
    // JavaScript instrumentation
    // -----------------------------------------------------------------------

    #[test]
    fn test_js_function_entry_probe() {
        let src = "function greet(name) {\n  return 'hello ' + name;\n}\n";
        let inst = TreeSitterInstrumentor::new(Language::JavaScript).unwrap();
        let result = inst.instrument_source(src, "greet.js").unwrap();

        let fn_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::FunctionEntry)
            .collect();
        assert!(
            !fn_probes.is_empty(),
            "expected FunctionEntry probe for JS function"
        );
        assert!(result.source.contains("__apexProbe("));
    }

    #[test]
    fn test_js_if_else_probes() {
        let src =
            "function f(x) {\n  if (x > 0) {\n    return 1;\n  } else {\n    return -1;\n  }\n}\n";
        let inst = TreeSitterInstrumentor::new(Language::JavaScript).unwrap();
        let result = inst.instrument_source(src, "f.js").unwrap();

        let true_count = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::BranchTrue)
            .count();
        let false_count = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::BranchFalse)
            .count();
        assert!(true_count >= 1, "expected BranchTrue probe");
        assert!(false_count >= 1, "expected BranchFalse probe");
    }

    #[test]
    fn test_js_loop_probe() {
        let src =
            "function count() {\n  for (let i = 0; i < 10; i++) {\n    console.log(i);\n  }\n}\n";
        let inst = TreeSitterInstrumentor::new(Language::JavaScript).unwrap();
        let result = inst.instrument_source(src, "count.js").unwrap();

        let loop_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::LoopEntry)
            .collect();
        assert!(!loop_probes.is_empty(), "expected LoopEntry probe");
    }

    #[test]
    fn test_js_empty_source_no_probes() {
        let src = "";
        let inst = TreeSitterInstrumentor::new(Language::JavaScript).unwrap();
        let result = inst.instrument_source(src, "empty.js").unwrap();
        assert!(result.probes.is_empty());
        assert_eq!(result.source, "");
    }

    #[test]
    fn test_js_probe_ids_sequential() {
        let src = "function a() {}\nfunction b() {}\n";
        let inst = TreeSitterInstrumentor::new(Language::JavaScript).unwrap();
        let result = inst.instrument_source(src, "two.js").unwrap();
        for (i, p) in result.probes.iter().enumerate() {
            assert_eq!(p.id, i as u32);
        }
    }

    #[test]
    fn test_js_malformed_source_graceful() {
        let src = "function f( { return !!!; }";
        let inst = TreeSitterInstrumentor::new(Language::JavaScript).unwrap();
        let result = inst.instrument_source(src, "bad.js");
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Go instrumentation
    // -----------------------------------------------------------------------

    #[test]
    fn test_go_function_entry_probe() {
        let src = "package main\n\nfunc hello() {\n\treturn\n}\n";
        let inst = TreeSitterInstrumentor::new(Language::Go).unwrap();
        let result = inst.instrument_source(src, "hello.go").unwrap();

        let fn_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::FunctionEntry)
            .collect();
        assert!(!fn_probes.is_empty(), "expected FunctionEntry probe");
        assert!(result.source.contains("apexProbe("));
    }

    #[test]
    fn test_go_if_else_probes() {
        let src = "package main\n\nfunc check(x int) int {\n\tif x > 0 {\n\t\treturn 1\n\t} else {\n\t\treturn -1\n\t}\n}\n";
        let inst = TreeSitterInstrumentor::new(Language::Go).unwrap();
        let result = inst.instrument_source(src, "check.go").unwrap();

        let true_count = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::BranchTrue)
            .count();
        assert!(true_count >= 1, "expected BranchTrue probe");
    }

    #[test]
    fn test_go_loop_probe() {
        let src = "package main\n\nfunc loop() {\n\tfor i := 0; i < 10; i++ {\n\t\t_ = i\n\t}\n}\n";
        let inst = TreeSitterInstrumentor::new(Language::Go).unwrap();
        let result = inst.instrument_source(src, "loop.go").unwrap();

        let loop_probes: Vec<_> = result
            .probes
            .iter()
            .filter(|p| p.kind == ProbeKind::LoopEntry)
            .collect();
        assert!(!loop_probes.is_empty(), "expected LoopEntry probe");
    }

    #[test]
    fn test_go_empty_source_no_probes() {
        let src = "";
        let inst = TreeSitterInstrumentor::new(Language::Go).unwrap();
        let result = inst.instrument_source(src, "empty.go").unwrap();
        assert!(result.probes.is_empty());
    }

    #[test]
    fn test_go_malformed_source_graceful() {
        let src = "package main\nfunc f( { !!!";
        let inst = TreeSitterInstrumentor::new(Language::Go).unwrap();
        let result = inst.instrument_source(src, "bad.go");
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Probe runtime constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_python_probe_runtime_has_probe_fn() {
        assert!(PYTHON_PROBE_RUNTIME.contains("__apex_probe"));
        assert!(PYTHON_PROBE_RUNTIME.contains("_hits"));
    }

    #[test]
    fn test_js_probe_runtime_has_probe_fn() {
        assert!(JS_PROBE_RUNTIME.contains("__apexProbe"));
        assert!(JS_PROBE_RUNTIME.contains("_apexHits"));
    }

    #[test]
    fn test_go_probe_runtime_has_probe_fn() {
        assert!(GO_PROBE_RUNTIME.contains("apexProbe"));
        assert!(GO_PROBE_RUNTIME.contains("_apexHits"));
    }

    // -----------------------------------------------------------------------
    // ProbeKind Display
    // -----------------------------------------------------------------------

    #[test]
    fn test_probe_kind_display() {
        assert_eq!(ProbeKind::FunctionEntry.to_string(), "FunctionEntry");
        assert_eq!(ProbeKind::BranchTrue.to_string(), "BranchTrue");
        assert_eq!(ProbeKind::BranchFalse.to_string(), "BranchFalse");
        assert_eq!(ProbeKind::LoopEntry.to_string(), "LoopEntry");
    }
}
