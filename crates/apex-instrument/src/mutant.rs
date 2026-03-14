//! Source-level mutation injection for mutation testing.
//!
//! Uses line-based text replacement (no AST required) to inject mutations
//! into source code for each MutationKind.

use apex_coverage::mutation::{MutationKind, MutationOperator};
use regex::Regex;

/// Injects mutations into source code text.
pub struct MutantInjector;

impl MutantInjector {
    /// Apply a single mutation operator to source code.
    /// Returns the mutated source with the replacement applied.
    pub fn inject_mutation(source: &str, op: &MutationOperator) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let line_idx = (op.line as usize).saturating_sub(1);
        if line_idx < lines.len() {
            let mutated_line = lines[line_idx].replace(&op.original, &op.replacement);
            let mut result: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            result[line_idx] = mutated_line;
            return result.join("\n");
        }
        source.to_string()
    }

    /// Scan source code and generate all applicable mutation operators.
    /// Uses regex-based pattern matching (no full AST parse).
    pub fn generate_mutants(source: &str, file: &str) -> Vec<MutationOperator> {
        let mut ops = Vec::new();

        let re_assignment = Regex::new(r"[a-zA-Z_]\w*\s*=[^=]").unwrap();
        let re_keyword_line =
            Regex::new(r"^\s*(def |class |fn |if |for |while |let |const |var |import |from )")
                .unwrap();
        let re_conditional = Regex::new(r"^\s*if\s+").unwrap();
        let re_return = Regex::new(r"\breturn\s+\S").unwrap();
        let re_arith = Regex::new(r" [+\-*/] ").unwrap();
        let re_boundary = Regex::new(r"(<=|>=|<|>)").unwrap();
        let re_integer = Regex::new(r"\b(\d+)\b").unwrap();

        for (line_num, line) in source.lines().enumerate() {
            let line_1based = (line_num + 1) as u32;
            let trimmed = line.trim();

            // StatementDeletion: assignment lines (not keyword lines)
            if re_assignment.is_match(line) && !re_keyword_line.is_match(line) {
                ops.push(MutationOperator {
                    kind: MutationKind::StatementDeletion,
                    file: file.to_string(),
                    line: line_1based,
                    original: trimmed.to_string(),
                    replacement: "pass".to_string(),
                });
            }

            // ConditionalNegation: lines starting with `if`
            if re_conditional.is_match(line) {
                if let Some(cond_start) = line.find("if ") {
                    let after_if = &line[cond_start + 3..];
                    // Strip trailing colon for Python
                    let condition = after_if.trim_end_matches(':').trim();
                    ops.push(MutationOperator {
                        kind: MutationKind::ConditionalNegation,
                        file: file.to_string(),
                        line: line_1based,
                        original: format!("if {}", condition),
                        replacement: format!("if not ({})", condition),
                    });
                }
            }

            // ReturnValueChange: lines with `return <value>`
            if re_return.is_match(line) {
                if let Some(ret_start) = line.find("return ") {
                    let value = line[ret_start + 7..].trim().trim_end_matches(';');
                    ops.push(MutationOperator {
                        kind: MutationKind::ReturnValueChange,
                        file: file.to_string(),
                        line: line_1based,
                        original: format!("return {}", value),
                        replacement: "return None".to_string(),
                    });
                }
            }

            // ArithmeticReplace: swap operators
            for cap in re_arith.find_iter(line) {
                let op_char = cap.as_str().trim();
                let replacement_op = match op_char {
                    "+" => "-",
                    "-" => "+",
                    "*" => "/",
                    "/" => "*",
                    _ => continue,
                };
                ops.push(MutationOperator {
                    kind: MutationKind::ArithmeticReplace,
                    file: file.to_string(),
                    line: line_1based,
                    original: format!(" {} ", op_char),
                    replacement: format!(" {} ", replacement_op),
                });
            }

            // BoundaryShift: shift comparison operators
            // Process longer operators first to avoid partial matches
            for cap in re_boundary.find_iter(line) {
                let op_str = cap.as_str();
                let replacement_op = match op_str {
                    "<=" => "<",
                    ">=" => ">",
                    "<" => "<=",
                    ">" => ">=",
                    _ => continue,
                };
                ops.push(MutationOperator {
                    kind: MutationKind::BoundaryShift,
                    file: file.to_string(),
                    line: line_1based,
                    original: op_str.to_string(),
                    replacement: replacement_op.to_string(),
                });
            }

            // ConstantReplace: integer literals → 0
            for cap in re_integer.captures_iter(line) {
                let val = &cap[1];
                if val != "0" && val != "1" {
                    ops.push(MutationOperator {
                        kind: MutationKind::ConstantReplace,
                        file: file.to_string(),
                        line: line_1based,
                        original: val.to_string(),
                        replacement: "0".to_string(),
                    });
                }
            }
        }

        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_op(
        kind: MutationKind,
        line: u32,
        original: &str,
        replacement: &str,
    ) -> MutationOperator {
        MutationOperator {
            kind,
            file: "test.py".into(),
            line,
            original: original.into(),
            replacement: replacement.into(),
        }
    }

    #[test]
    fn inject_replaces_on_correct_line() {
        let source = "a = 1\nb = 2\nc = 3";
        let op = make_op(MutationKind::ArithmeticReplace, 2, "b = 2", "b = 0");
        let result = MutantInjector::inject_mutation(source, &op);
        assert_eq!(result, "a = 1\nb = 0\nc = 3");
    }

    #[test]
    fn inject_no_match_returns_original() {
        let source = "a = 1\nb = 2\nc = 3";
        let op = make_op(MutationKind::ArithmeticReplace, 2, "xyz", "abc");
        let result = MutantInjector::inject_mutation(source, &op);
        assert_eq!(result, source);
    }

    #[test]
    fn inject_out_of_bounds_returns_original() {
        let source = "a = 1\nb = 2";
        let op = make_op(MutationKind::ArithmeticReplace, 99, "a", "b");
        let result = MutantInjector::inject_mutation(source, &op);
        assert_eq!(result, source);
    }

    #[test]
    fn generate_finds_arithmetic() {
        let source = "x = a + b";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let arith: Vec<_> = ops
            .iter()
            .filter(|o| o.kind == MutationKind::ArithmeticReplace)
            .collect();
        assert_eq!(arith.len(), 1);
        assert_eq!(arith[0].original, " + ");
        assert_eq!(arith[0].replacement, " - ");
    }

    #[test]
    fn generate_finds_conditionals() {
        let source = "if x > 0:";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let conds: Vec<_> = ops
            .iter()
            .filter(|o| o.kind == MutationKind::ConditionalNegation)
            .collect();
        assert_eq!(conds.len(), 1);
        assert!(conds[0].replacement.contains("not"));
    }

    #[test]
    fn generate_finds_returns() {
        let source = "    return result";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let rets: Vec<_> = ops
            .iter()
            .filter(|o| o.kind == MutationKind::ReturnValueChange)
            .collect();
        assert_eq!(rets.len(), 1);
        assert_eq!(rets[0].replacement, "return None");
    }

    #[test]
    fn generate_finds_boundary() {
        let source = "if x <= y:";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let bounds: Vec<_> = ops
            .iter()
            .filter(|o| o.kind == MutationKind::BoundaryShift)
            .collect();
        assert!(!bounds.is_empty());
        assert_eq!(bounds[0].original, "<=");
        assert_eq!(bounds[0].replacement, "<");
    }

    #[test]
    fn generate_finds_constants() {
        let source = "timeout = 30";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let consts: Vec<_> = ops
            .iter()
            .filter(|o| o.kind == MutationKind::ConstantReplace)
            .collect();
        assert_eq!(consts.len(), 1);
        assert_eq!(consts[0].original, "30");
        assert_eq!(consts[0].replacement, "0");
    }

    #[test]
    fn generate_empty_source() {
        let ops = MutantInjector::generate_mutants("", "test.py");
        assert!(ops.is_empty());
    }

    #[test]
    fn generate_multiple_mutations_per_line() {
        let source = "if a + b < 10:";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let kinds: Vec<_> = ops.iter().map(|o| o.kind).collect();
        assert!(kinds.contains(&MutationKind::ArithmeticReplace));
        assert!(kinds.contains(&MutationKind::BoundaryShift));
        assert!(kinds.contains(&MutationKind::ConditionalNegation));
    }

    #[test]
    fn inject_statement_deletion() {
        let source = "x = 1\ny = compute()\nz = 3";
        let op = make_op(MutationKind::StatementDeletion, 2, "y = compute()", "pass");
        let result = MutantInjector::inject_mutation(source, &op);
        assert_eq!(result, "x = 1\npass\nz = 3");
    }

    #[test]
    fn generate_skips_def_lines() {
        let source = "def foo(x = 10):\n    pass";
        let ops = MutantInjector::generate_mutants(source, "test.py");
        let deletions: Vec<_> = ops
            .iter()
            .filter(|o| o.kind == MutationKind::StatementDeletion)
            .collect();
        assert!(deletions.is_empty());
    }
}
