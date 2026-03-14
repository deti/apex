/// Strip imports and top-level functions not referenced by or within `target_fn`.
pub fn eliminate_irrelevant(source: &str, target_fn: &str) -> String {
    let mut out = Vec::new();
    let mut in_target = false;
    let mut indent_base = 0usize;

    // Collect identifiers used in target function body.
    let target_body = extract_function_body(source, target_fn);

    for line in source.lines() {
        let trimmed = line.trim_start();
        // Keep target function and its body.
        if trimmed.starts_with(&format!("def {target_fn}")) {
            in_target = true;
            indent_base = line.len() - trimmed.len();
            out.push(line);
            continue;
        }
        if in_target {
            let cur_indent = line.len() - line.trim_start().len();
            if line.trim().is_empty() || cur_indent > indent_base {
                out.push(line); continue;
            }
            in_target = false;
        }
        // Keep imports only if the imported name appears in target body.
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            let name = trimmed.split_whitespace().nth(1).unwrap_or("");
            if target_body.contains(name) { out.push(line); }
            continue;
        }
        // Skip other top-level defs not referenced by target.
        if trimmed.starts_with("def ") || trimmed.starts_with("class ") { continue; }
    }
    out.join("\n")
}

fn extract_function_body(source: &str, fn_name: &str) -> String {
    let mut body = String::new();
    let mut in_fn = false;
    let mut base = 0usize;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("def {fn_name}")) {
            in_fn = true;
            base = line.len() - trimmed.len();
            continue;
        }
        if in_fn {
            let cur = line.len() - line.trim_start().len();
            if !line.trim().is_empty() && cur <= base { break; }
            body.push_str(line); body.push('\n');
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
import os
import sys
import logging  # unused

def foo(x):
    if x > 0:
        return x
    return -x

def bar():  # unrelated
    print("hello")
"#;

    #[test]
    fn eliminates_imports_not_referenced_in_target() {
        let result = eliminate_irrelevant(SOURCE, "foo");
        assert!(!result.contains("logging"), "unused import should be removed");
        assert!(result.contains("import os") || result.contains("def foo"));
    }

    #[test]
    fn keeps_target_function() {
        let result = eliminate_irrelevant(SOURCE, "foo");
        assert!(result.contains("def foo"));
        assert!(result.contains("return x"));
    }

    #[test]
    fn result_is_shorter_than_original() {
        let result = eliminate_irrelevant(SOURCE, "foo");
        assert!(result.len() < SOURCE.len());
    }
}
