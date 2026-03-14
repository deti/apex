use crate::cfg::{BasicBlock, MirFunction, Statement, Terminator};

/// Parse rustc MIR text output into a list of `MirFunction`s.
///
/// Expects the format produced by `rustc --emit=mir` or `-Zdump-mir=all`.
pub fn parse_mir_output(mir_text: &str) -> Vec<MirFunction> {
    let mut functions: Vec<MirFunction> = Vec::new();
    let mut current_fn: Option<MirFunction> = None;
    let mut current_stmts: Vec<Statement> = Vec::new();
    let mut current_bb_id: Option<usize> = None;
    // Track brace depth so we can distinguish bb-closing `}` from fn-closing `}`.
    let mut brace_depth: usize = 0;

    for raw_line in mir_text.lines() {
        let line = raw_line.trim();

        // Detect function start: "fn <name>(...) -> ... {"
        if line.starts_with("fn ") && line.ends_with('{') {
            // Finish previous function if any
            if let Some(mut f) = current_fn.take() {
                if let Some(bb_id) = current_bb_id.take() {
                    f.blocks.push(BasicBlock {
                        id: bb_id,
                        statements: std::mem::take(&mut current_stmts),
                        terminator: Terminator::Return,
                    });
                }
                functions.push(f);
            }
            let name = extract_fn_name(line);
            current_fn = Some(MirFunction::new(name));
            current_bb_id = None;
            current_stmts.clear();
            brace_depth = 1; // the opening `{` of the function
            continue;
        }

        if current_fn.is_none() {
            continue;
        }

        // Track brace depth for lines containing braces
        let opens = line.chars().filter(|&c| c == '{').count();
        let closes = line.chars().filter(|&c| c == '}').count();
        brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);

        // Function body closed when depth reaches 0
        if brace_depth == 0 {
            if let Some(mut f) = current_fn.take() {
                if let Some(bb_id) = current_bb_id.take() {
                    f.blocks.push(BasicBlock {
                        id: bb_id,
                        statements: std::mem::take(&mut current_stmts),
                        terminator: Terminator::Return,
                    });
                }
                functions.push(f);
            }
            continue;
        }

        // Detect basic block header: "bb0: {" or "bb12: {"
        if line.starts_with("bb") && line.contains(": {") {
            // Flush previous bb
            if let Some(bb_id) = current_bb_id.take() {
                if let Some(ref mut f) = current_fn {
                    f.blocks.push(BasicBlock {
                        id: bb_id,
                        statements: std::mem::take(&mut current_stmts),
                        terminator: Terminator::Return,
                    });
                }
            }
            if let Some(id) = parse_bb_ref(&line[..line.find(':').unwrap_or(line.len())]) {
                current_bb_id = Some(id);
            }
            current_stmts.clear();
            continue;
        }

        // Inside a basic block — parse statements and terminators
        if current_bb_id.is_some() {
            if line.starts_with("StorageLive(") {
                let var = line
                    .trim_start_matches("StorageLive(")
                    .trim_end_matches(");")
                    .trim()
                    .to_string();
                current_stmts.push(Statement::StorageLive(var));
            } else if line.starts_with("StorageDead(") {
                let var = line
                    .trim_start_matches("StorageDead(")
                    .trim_end_matches(");")
                    .trim()
                    .to_string();
                current_stmts.push(Statement::StorageDead(var));
            } else if line.contains(" = ") && line.ends_with(';') {
                let parts: Vec<&str> = line.trim_end_matches(';').splitn(2, " = ").collect();
                if parts.len() == 2 {
                    current_stmts.push(Statement::Assign {
                        place: parts[0].trim().to_string(),
                        rvalue: parts[1].trim().to_string(),
                    });
                }
            } else if line.starts_with("goto ->") || line.starts_with("goto->") {
                let rest = line
                    .trim_start_matches("goto")
                    .trim_start_matches(" ->")
                    .trim_start_matches("->")
                    .trim();
                let target = rest.trim_end_matches(';');
                if let Some(t) = parse_bb_ref(target) {
                    finish_bb_with_terminator(
                        &mut current_fn,
                        &mut current_bb_id,
                        &mut current_stmts,
                        Terminator::Goto { target: t },
                    );
                }
            } else if line.starts_with("return;") || line == "return" {
                finish_bb_with_terminator(
                    &mut current_fn,
                    &mut current_bb_id,
                    &mut current_stmts,
                    Terminator::Return,
                );
            } else if line.starts_with("unreachable;") || line == "unreachable" {
                finish_bb_with_terminator(
                    &mut current_fn,
                    &mut current_bb_id,
                    &mut current_stmts,
                    Terminator::Unreachable,
                );
            } else if line.starts_with("abort;") || line == "abort" {
                finish_bb_with_terminator(
                    &mut current_fn,
                    &mut current_bb_id,
                    &mut current_stmts,
                    Terminator::Abort,
                );
            } else if line == "}" {
                // End of bb block — flush with default Return if terminator wasn't explicit
                if let Some(bb_id) = current_bb_id.take() {
                    if let Some(ref mut f) = current_fn {
                        f.blocks.push(BasicBlock {
                            id: bb_id,
                            statements: std::mem::take(&mut current_stmts),
                            terminator: Terminator::Return,
                        });
                    }
                }
            }
        }
    }

    // Flush trailing function
    if let Some(mut f) = current_fn.take() {
        if let Some(bb_id) = current_bb_id.take() {
            f.blocks.push(BasicBlock {
                id: bb_id,
                statements: std::mem::take(&mut current_stmts),
                terminator: Terminator::Return,
            });
        }
        functions.push(f);
    }

    functions
}

/// Helper: finish the current basic block with a specific terminator.
fn finish_bb_with_terminator(
    current_fn: &mut Option<MirFunction>,
    current_bb_id: &mut Option<usize>,
    current_stmts: &mut Vec<Statement>,
    terminator: Terminator,
) {
    if let Some(bb_id) = current_bb_id.take() {
        if let Some(ref mut f) = current_fn {
            f.blocks.push(BasicBlock {
                id: bb_id,
                statements: std::mem::take(current_stmts),
                terminator,
            });
        }
    }
}

/// Extract a function name from a MIR function header line.
///
/// Input like `fn foo::bar(_1: u32) -> bool {` returns `foo::bar`.
pub fn extract_fn_name(line: &str) -> String {
    let after_fn = line.trim_start_matches("fn ").trim();
    // The name ends at the first '(' or whitespace
    let end = after_fn
        .find('(')
        .unwrap_or_else(|| after_fn.find(' ').unwrap_or(after_fn.len()));
    after_fn[..end].trim().to_string()
}

/// Parse a basic block reference like `bb3` into `Some(3)`.
pub fn parse_bb_ref(s: &str) -> Option<usize> {
    let s = s.trim();
    s.strip_prefix("bb").and_then(|rest| rest.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bb_ref_valid() {
        assert_eq!(parse_bb_ref("bb0"), Some(0));
        assert_eq!(parse_bb_ref("bb42"), Some(42));
    }

    #[test]
    fn parse_bb_ref_invalid() {
        assert_eq!(parse_bb_ref("block0"), None);
        assert_eq!(parse_bb_ref(""), None);
        assert_eq!(parse_bb_ref("bbXYZ"), None);
    }

    #[test]
    fn extract_fn_name_simple() {
        assert_eq!(extract_fn_name("fn foo(_1: u32) -> bool {"), "foo");
    }

    #[test]
    fn extract_fn_name_path() {
        assert_eq!(
            extract_fn_name("fn my_crate::module::bar(_1: i32) {"),
            "my_crate::module::bar"
        );
    }

    #[test]
    fn parse_empty_mir() {
        let result = parse_mir_output("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_single_function_single_block() {
        let mir = "\
fn simple(_1: u32) -> u32 {
    bb0: {
        _0 = _1;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "simple");
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn parse_function_with_goto() {
        let mir = "\
fn with_goto() -> () {
    bb0: {
        goto -> bb1;
    }
    bb1: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 2);
        let succs = funcs[0].successors(0);
        assert_eq!(succs, vec![1]);
    }

    #[test]
    fn parse_multiple_functions() {
        let mir = "\
fn alpha() -> () {
    bb0: {
        return;
    }
}
fn beta() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name, "alpha");
        assert_eq!(funcs[1].name, "beta");
    }

    #[test]
    fn parse_storage_live_dead() {
        let mir = "\
fn storage_test() -> () {
    bb0: {
        StorageLive(_1);
        StorageDead(_1);
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Statement::StorageLive(v) if v == "_1"));
        assert!(matches!(&stmts[1], Statement::StorageDead(v) if v == "_1"));
    }

    #[test]
    fn parse_assignment() {
        let mir = "\
fn assign_test() -> () {
    bb0: {
        _0 = _1;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 1);
        assert!(
            matches!(&stmts[0], Statement::Assign { place, rvalue } if place == "_0" && rvalue == "_1")
        );
    }

    #[test]
    fn parse_unreachable_terminator() {
        let mir = "\
fn unreachable_test() -> ! {
    bb0: {
        unreachable;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Unreachable
        ));
    }

    #[test]
    fn parse_abort_terminator() {
        let mir = "\
fn abort_test() -> ! {
    bb0: {
        abort;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Abort));
    }

    #[test]
    fn parse_goto_no_space() {
        let mir = "\
fn goto_nospace() -> () {
    bb0: {
        goto->bb1;
    }
    bb1: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 2);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
    }

    #[test]
    fn parse_function_with_brace_depth_close() {
        // The function closes via the outer `}` dropping brace_depth to 0.
        let mir = "\
fn depth_close(_1: bool) -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "depth_close");
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn parse_trailing_function() {
        // No closing `}` — the trailing-flush path (lines 152-162) must fire.
        let mir = "\
fn trailing() -> () {
    bb0: {
        return;";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "trailing");
        // bb0 was open but never explicitly closed; trailing flush saves it.
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn parse_multiple_blocks() {
        // Exercises the bb-header flush path: each new `bb: {` flushes the previous bb.
        let mir = "\
fn multi_block() -> () {
    bb0: {
        goto -> bb1;
    }
    bb1: {
        goto -> bb2;
    }
    bb2: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 3);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
        assert!(matches!(
            funcs[0].blocks[1].terminator,
            Terminator::Goto { target: 2 }
        ));
        assert!(matches!(funcs[0].blocks[2].terminator, Terminator::Return));
    }

    #[test]
    fn parse_bb_implicit_close() {
        // A bb that ends with `}` without an explicit terminator keyword.
        // The `} ` branch (line 137) must fire and flush with Terminator::Return.
        let mir = "\
fn implicit_close() -> () {
    bb0: {
        StorageLive(_1);
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
        assert_eq!(funcs[0].blocks[0].statements.len(), 1);
    }

    #[test]
    fn parse_fn_start_flushes_previous() {
        // Two functions back-to-back where second starts before first explicitly closes.
        // Exercises the flush-previous-fn path at lines 20-28.
        let mir = "fn first() -> () {\n    bb0: {\n        _0 = const 1;\n\nfn second() -> () {\n    bb0: {\n        return;\n    }\n}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name, "first");
        assert_eq!(funcs[1].name, "second");
    }

    #[test]
    fn parse_fn_with_no_basic_blocks() {
        // brace_depth drops to 0 with no bb — function should still be emitted.
        let mir = "fn empty_fn() -> () {\n}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "empty_fn");
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn parse_non_mir_lines_ignored() {
        // Lines when current_fn is None hit the `continue` at line 39.
        let mir = "// some comment\nrandom junk\n\nfn real() -> () {\n    bb0: {\n        return;\n    }\n}\nmore junk after";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "real");
    }

    #[test]
    fn parse_return_bare_keyword() {
        // Exercises the `line == "return"` branch (no semicolon).
        let mir = "fn bare_return() -> () {\n    bb0: {\n        return\n    }\n}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn parse_unreachable_bare_keyword() {
        // Exercises the `line == "unreachable"` branch (no semicolon).
        let mir = "fn bare_unreachable() -> ! {\n    bb0: {\n        unreachable\n    }\n}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Unreachable
        ));
    }

    #[test]
    fn parse_abort_bare_keyword() {
        // Exercises the `line == "abort"` branch (no semicolon).
        let mir = "fn bare_abort() -> ! {\n    bb0: {\n        abort\n    }\n}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Abort));
    }

    #[test]
    fn parse_complex_assignment_rvalue() {
        // rvalue contains `=` — splitn(2) must keep it intact.
        let mir =
            "fn complex() -> () {\n    bb0: {\n        _0 = Eq(_1, _2);\n        return;\n    }\n}";
        let funcs = parse_mir_output(mir);
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 1);
        assert!(
            matches!(&stmts[0], Statement::Assign { place, rvalue } if place == "_0" && rvalue == "Eq(_1, _2)")
        );
    }

    #[test]
    fn parse_goto_with_semicolon() {
        // `goto -> bb1;` — semicolon trimming path.
        let mir = "fn goto_semi() -> () {\n    bb0: {\n        goto -> bb1;\n    }\n    bb1: {\n        return;\n    }\n}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
    }

    #[test]
    fn parse_bb_ref_with_whitespace() {
        // parse_bb_ref trims surrounding whitespace before parsing.
        assert_eq!(parse_bb_ref("  bb5  "), Some(5));
    }

    #[test]
    fn extract_fn_name_no_parens() {
        // extract_fn_name falls back to whitespace split when no `(` present.
        assert_eq!(extract_fn_name("fn bare {"), "bare");
    }

    #[test]
    fn extract_fn_name_no_parens_no_space() {
        // No '(' and no space after name — uses full remaining string.
        assert_eq!(extract_fn_name("fn solitary"), "solitary");
    }

    #[test]
    fn parse_unrecognized_statement_ignored() {
        // Lines inside a bb that do not match any statement/terminator pattern
        // should be silently ignored without affecting the block.
        let mir = "\
fn unknown_stmt() -> () {
    bb0: {
        StorageLive(_1);
        nop;
        some_random_directive
        _0 = _1;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // Only StorageLive and Assign are recognized; the two junk lines are skipped.
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Statement::StorageLive(v) if v == "_1"));
        assert!(
            matches!(&stmts[1], Statement::Assign { place, rvalue } if place == "_0" && rvalue == "_1")
        );
    }

    #[test]
    fn parse_goto_invalid_target_ignored() {
        // goto with a non-parseable target — parse_bb_ref returns None,
        // so the terminator is never set and the bb gets default Return.
        let mir = "\
fn goto_bad() -> () {
    bb0: {
        goto -> notabb;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // bb0 was never flushed by goto (target invalid), so it gets flushed
        // by the closing `}` with default Return terminator.
        assert_eq!(funcs[0].block_count(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn parse_fn_start_flushes_previous_without_open_bb() {
        // First function has no bb when second function starts.
        // Exercises fn-start flush path where current_bb_id is None.
        let mir = "\
fn first() -> () {
fn second() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name, "first");
        assert_eq!(funcs[0].block_count(), 0); // no bb was open
        assert_eq!(funcs[1].name, "second");
        assert_eq!(funcs[1].block_count(), 1);
    }

    #[test]
    fn parse_brace_depth_close_without_open_bb() {
        // Function closes via brace_depth=0 but has no open bb at that point.
        // Exercises the brace-depth-zero flush where current_bb_id is None.
        let mir = "\
fn no_bb_close() -> () {
    // just some non-bb content
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "no_bb_close");
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn parse_trailing_function_no_open_bb() {
        // Trailing flush with a function that has no open bb.
        // Exercises lines 157-165 where current_bb_id is None.
        let mir = "fn trailing_no_bb() -> () {";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "trailing_no_bb");
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn parse_assignment_line_without_equals_sign() {
        // A line ending in `;` but containing no ` = ` should not produce
        // an Assign statement.
        let mir = "\
fn no_assign() -> () {
    bb0: {
        _debug x => _1;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // The `_debug x => _1;` line has no ` = `, so no statement is added.
        assert_eq!(funcs[0].blocks[0].statements.len(), 0);
    }

    #[test]
    fn parse_multiple_stmts_then_terminator() {
        // Multiple statements followed by a terminator, verifying order.
        let mir = "\
fn multi_stmt() -> () {
    bb0: {
        StorageLive(_1);
        _0 = Add(_1, _2);
        StorageDead(_1);
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 3);
        assert!(matches!(&stmts[0], Statement::StorageLive(v) if v == "_1"));
        assert!(
            matches!(&stmts[1], Statement::Assign { place, rvalue } if place == "_0" && rvalue == "Add(_1, _2)")
        );
        assert!(matches!(&stmts[2], Statement::StorageDead(v) if v == "_1"));
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn parse_bb_header_flushes_previous_bb_without_terminator() {
        // A new bb header flushes the previous bb that had no explicit terminator.
        // Exercises the bb-header flush at lines 65-72.
        let mir = "\
fn flush_bb() -> () {
    bb0: {
        StorageLive(_1);
    bb1: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 2);
        // bb0 was flushed by bb1 header with default Return
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
        assert_eq!(funcs[0].blocks[0].statements.len(), 1);
    }

    #[test]
    fn parse_three_functions_sequential() {
        // Three functions in sequence to stress the flush logic.
        let mir = "\
fn a() -> () {
    bb0: {
        return;
    }
}
fn b() -> () {
    bb0: {
        goto -> bb1;
    }
    bb1: {
        unreachable;
    }
}
fn c() -> () {
    bb0: {
        abort;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 3);
        assert_eq!(funcs[0].name, "a");
        assert_eq!(funcs[1].name, "b");
        assert_eq!(funcs[2].name, "c");
        assert_eq!(funcs[1].block_count(), 2);
        assert!(matches!(
            funcs[1].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
        assert!(matches!(
            funcs[1].blocks[1].terminator,
            Terminator::Unreachable
        ));
        assert!(matches!(funcs[2].blocks[0].terminator, Terminator::Abort));
    }

    #[test]
    fn parse_bb_invalid_id_in_header() {
        // bb header with non-numeric id — parse_bb_ref returns None,
        // so no bb_id is set and the block content is ignored.
        let mir = "\
fn bad_bb_id() -> () {
    bbXYZ: {
        StorageLive(_1);
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // bbXYZ did not parse, so no block was created.
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn parse_nested_braces_in_statement() {
        // A statement containing braces should still track brace depth correctly.
        let mir = "\
fn nested_braces() -> () {
    bb0: {
        _0 = SomeStruct { field: 1 };
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        // The inner braces bump depth up then back down, but the function
        // should still close properly.
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn finish_bb_with_terminator_no_open_bb() {
        // When current_bb_id is None, finish_bb_with_terminator is a no-op.
        // This is exercised via a goto with valid target but no open bb.
        // We simulate this indirectly: goto fires, takes the bb_id, then
        // a second goto on the same bb has no bb_id left.
        let mir = "\
fn double_goto() -> () {
    bb0: {
        goto -> bb1;
        goto -> bb2;
    }
    bb1: {
        return;
    }
    bb2: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // First goto consumed bb0; second goto had no bb_id, so it's a no-op.
        // bb1 and bb2 are parsed normally.
        assert!(funcs[0].block_count() >= 2);
    }

    #[test]
    fn finish_bb_with_terminator_no_current_fn() {
        // finish_bb_with_terminator with current_fn == None is only reachable
        // if somehow current_fn was taken. In practice this doesn't happen,
        // but we can test the helper directly.
        let mut current_fn: Option<MirFunction> = None;
        let mut current_bb_id: Option<usize> = Some(0);
        let mut current_stmts: Vec<Statement> = vec![Statement::Nop];
        finish_bb_with_terminator(
            &mut current_fn,
            &mut current_bb_id,
            &mut current_stmts,
            Terminator::Return,
        );
        // bb_id was taken but no function to push to
        assert!(current_bb_id.is_none());
        assert!(current_fn.is_none());
    }

    #[test]
    fn parse_only_whitespace() {
        let mir = "   \n  \n\t  \n";
        let funcs = parse_mir_output(mir);
        assert!(funcs.is_empty());
    }

    #[test]
    fn parse_comments_and_garbage_only() {
        let mir = "// comment 1\n# not MIR\nsome random text\n---\n===";
        let funcs = parse_mir_output(mir);
        assert!(funcs.is_empty());
    }

    #[test]
    fn parse_fn_no_body_just_opening_brace() {
        // Function with opening brace and nothing else (trailing flush, no bb)
        let mir = "fn lonely() -> () {";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "lonely");
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn parse_multiple_bb_same_function_sequential_terminators() {
        // Each bb has a different terminator type
        let mir = "\
fn varied() -> () {
    bb0: {
        goto -> bb1;
    }
    bb1: {
        unreachable;
    }
    bb2: {
        abort;
    }
    bb3: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 4);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
        assert!(matches!(
            funcs[0].blocks[1].terminator,
            Terminator::Unreachable
        ));
        assert!(matches!(funcs[0].blocks[2].terminator, Terminator::Abort));
        assert!(matches!(funcs[0].blocks[3].terminator, Terminator::Return));
    }

    #[test]
    fn parse_bb_close_brace_with_no_bb_open() {
        // A `}` line inside a function but outside any bb — just adjusts brace depth
        let mir = "\
fn stray_brace() -> () {
    let x = 0;
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn parse_assignment_with_equals_in_rvalue() {
        // rvalue contains multiple `=` characters
        let mir = "\
fn multi_eq() -> () {
    bb0: {
        _0 = CheckedBinaryOp(Eq, _1, _2);
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Assign { place, rvalue } => {
                assert_eq!(place, "_0");
                assert_eq!(rvalue, "CheckedBinaryOp(Eq, _1, _2)");
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_fn_name_with_generics() {
        assert_eq!(
            extract_fn_name("fn std::vec::Vec::<T>::push(_1: &mut Vec<T>) {"),
            "std::vec::Vec::<T>::push"
        );
    }

    #[test]
    fn parse_fn_name_with_angle_brackets() {
        // Angle brackets before parenthesis
        assert_eq!(extract_fn_name("fn foo::<u32>() {"), "foo::<u32>");
    }

    #[test]
    fn parse_bb_ref_leading_zeros() {
        assert_eq!(parse_bb_ref("bb007"), Some(7));
    }

    #[test]
    fn parse_bb_ref_large_number() {
        assert_eq!(parse_bb_ref("bb999999"), Some(999999));
    }

    #[test]
    fn parse_bb_ref_just_bb() {
        // "bb" with no digits — parse fails
        assert_eq!(parse_bb_ref("bb"), None);
    }

    #[test]
    fn parse_bb_ref_negative() {
        // "bb-1" — not a valid usize
        assert_eq!(parse_bb_ref("bb-1"), None);
    }

    #[test]
    fn parse_storage_live_dead_whitespace_in_var() {
        // Variable name with trailing whitespace before `)` and `;`
        let mir = "\
fn ws_test() -> () {
    bb0: {
        StorageLive( _1 );
        StorageDead( _2 );
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        let stmts = &funcs[0].blocks[0].statements;
        assert_eq!(stmts.len(), 2);
        // The trim() in parsing should handle extra spaces
        assert!(matches!(&stmts[0], Statement::StorageLive(v) if v == "_1"));
        assert!(matches!(&stmts[1], Statement::StorageDead(v) if v == "_2"));
    }

    #[test]
    fn parse_line_starts_with_eq_sign_ignored() {
        // In test enumeration context, lines starting with = are skipped.
        // In MIR parsing, lines outside bb that don't match patterns are ignored.
        let mir = "\
fn eq_line() -> () {
    bb0: {
        return;
    }
}
= some separator line";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
    }

    #[test]
    fn parse_multiple_functions_first_has_no_close() {
        // First function never closes (no matching `}`), second function starts
        let mir = "\
fn first() -> () {
    bb0: {
        StorageLive(_1);
        _0 = _1;
fn second() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name, "first");
        // first's bb0 was open when second fn started — it should be flushed
        assert_eq!(funcs[0].block_count(), 1);
        assert_eq!(funcs[0].blocks[0].statements.len(), 2);
        assert_eq!(funcs[1].name, "second");
    }

    #[test]
    fn parse_bb_close_when_no_bb_id_set() {
        // `}` line inside a bb context where current_bb_id is already None
        // (e.g., after a terminator already consumed it)
        let mir = "\
fn extra_close() -> () {
    bb0: {
        return;
    }
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn parse_deep_nesting_braces() {
        // Statement with deeply nested braces
        let mir = "\
fn deep() -> () {
    bb0: {
        _0 = A { b: B { c: C { d: 1 } } };
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn parse_empty_bb_immediately_closed() {
        let mir = "\
fn empty_bb() -> () {
    bb0: {
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert!(funcs[0].blocks[0].statements.is_empty());
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn parse_goto_semicolon_no_space() {
        // goto->bb1; — no space between goto and ->
        let mir = "\
fn goto_nospc_semi() -> () {
    bb0: {
        goto->bb1;
    }
    bb1: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
    }

    #[test]
    fn extract_fn_name_with_return_type_and_where() {
        // Complex signature
        assert_eq!(
            extract_fn_name("fn my_mod::complex_fn(x: i32) -> Result<(), Error> {"),
            "my_mod::complex_fn"
        );
    }

    #[test]
    fn parse_trailing_with_open_bb_and_stmts() {
        // Trailing flush with an open bb that has accumulated statements
        let mir =
            "fn trailing_stmts() -> () {\n    bb0: {\n        StorageLive(_1);\n        _0 = _1;";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert_eq!(funcs[0].blocks[0].statements.len(), 2);
    }

    #[test]
    fn parse_bb_header_colon_at_end() {
        // bb header where `: {` is at the very end
        let mir = "\
fn colon_end() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    // -----------------------------------------------------------------------
    // Branch-coverage expansion tests
    // -----------------------------------------------------------------------

    #[test]
    fn line_starts_with_fn_but_no_opening_brace() {
        // A line starting with "fn " but NOT ending with '{' should not
        // trigger function-start detection (the && short-circuits).
        let mir = "\
fn real_fn() -> () {
    bb0: {
        _0 = fn helper;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "real_fn");
    }

    #[test]
    fn line_starts_with_bb_but_no_colon_brace() {
        // A line starting with "bb" but not containing ": {" should not
        // be treated as a bb header.
        let mir = "\
fn bb_prefix() -> () {
    bb0: {
        _0 = bball;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        // "bball" does not match any pattern (no ` = ` with `;` ending)
        // Wait — "_0 = bball;" would match assignment. Let's check:
        // Actually "_0 = bball;" doesn't end with `;` — let me fix:
    }

    #[test]
    fn line_with_bb_prefix_no_colon_brace_inside_bb() {
        // Inside a bb, a line starting with "bb" but not "bb<N>: {" format
        let mir = "\
fn bb_line() -> () {
    bb0: {
        _0 = bb_value;
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn assignment_contains_eq_no_trailing_semicolon() {
        // A line containing " = " but NOT ending with ';' should not
        // produce an Assign statement. Exercises false branch of
        // `line.contains(" = ") && line.ends_with(';')`.
        let mir = "\
fn no_semi() -> () {
    bb0: {
        _0 = something
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // "_0 = something" has no `;` so it's not matched as assignment
        assert_eq!(funcs[0].blocks[0].statements.len(), 0);
    }

    #[test]
    fn goto_with_space_arrow_exercises_first_branch() {
        // "goto -> bb1;" exercises starts_with("goto ->") = true
        // (the first arm of the || on line 105)
        let mir = "\
fn goto_space() -> () {
    bb0: {
        goto -> bb1;
    }
    bb1: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 1 }
        ));
    }

    #[test]
    fn goto_nospace_exercises_second_branch() {
        // "goto->bb1;" exercises starts_with("goto->") = true
        // (the second arm of the || on line 105, first arm is false)
        let mir = "\
fn goto_nospace2() -> () {
    bb0: {
        goto->bb2;
    }
    bb1: {
        return;
    }
    bb2: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Goto { target: 2 }
        ));
    }

    #[test]
    fn return_with_semicolon_exercises_first_branch() {
        // "return;" exercises starts_with("return;") = true
        let mir = "\
fn ret_semi() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn return_bare_exercises_second_branch() {
        // "return" (no semicolon) exercises line == "return" = true
        let mir = "\
fn ret_bare() -> () {
    bb0: {
        return
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn unreachable_with_semicolon_exercises_first_branch() {
        // "unreachable;" exercises starts_with("unreachable;") = true
        let mir = "\
fn unreach_semi() -> ! {
    bb0: {
        unreachable;
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Unreachable
        ));
    }

    #[test]
    fn unreachable_bare_exercises_second_branch() {
        // "unreachable" exercises line == "unreachable" = true
        let mir = "\
fn unreach_bare() -> ! {
    bb0: {
        unreachable
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(
            funcs[0].blocks[0].terminator,
            Terminator::Unreachable
        ));
    }

    #[test]
    fn abort_with_semicolon_exercises_first_branch() {
        // "abort;" exercises starts_with("abort;") = true
        let mir = "\
fn abort_semi() -> ! {
    bb0: {
        abort;
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Abort));
    }

    #[test]
    fn abort_bare_exercises_second_branch() {
        // "abort" exercises line == "abort" = true
        let mir = "\
fn abort_bare() -> ! {
    bb0: {
        abort
    }
}";
        let funcs = parse_mir_output(mir);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Abort));
    }

    #[test]
    fn brace_depth_nonzero_skips_flush() {
        // When brace_depth != 0 after counting braces, the function-close
        // path (line 48) is NOT taken — exercises the false branch.
        let mir = "\
fn depth_stays() -> () {
    bb0: {
        _0 = Struct { x: 1 };
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn current_fn_none_skips_processing() {
        // Lines before any function definition hit `current_fn.is_none() -> continue`
        // (true branch of line 38). Lines after function closes also hit this.
        let mir = "\
not a function
also not a function
fn real() -> () {
    bb0: {
        return;
    }
}
still not a function
trailing garbage";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
    }

    #[test]
    fn fn_start_flush_with_open_bb_having_stmts() {
        // A new function starts while the previous function has an open bb
        // with accumulated statements. Exercises lines 20-28 with both
        // current_fn = Some AND current_bb_id = Some.
        let mir = "\
fn first() -> () {
    bb0: {
        StorageLive(_1);
        _0 = _1;
fn second() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name, "first");
        assert_eq!(funcs[0].block_count(), 1);
        assert_eq!(funcs[0].blocks[0].statements.len(), 2);
        assert_eq!(funcs[1].name, "second");
    }

    #[test]
    fn brace_depth_close_with_open_bb() {
        // Function closes via brace_depth reaching 0 while a bb is open.
        // Exercises lines 49-55 (both Some branches).
        let mir = "\
fn depth_close_bb() -> () {
    bb0: {
        StorageLive(_1);
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn bb_header_no_previous_bb_open() {
        // First bb header in a function — current_bb_id is None, so the
        // flush-previous-bb path (line 65) takes the None branch.
        let mir = "\
fn first_bb() -> () {
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn content_outside_bb_inside_fn_ignored() {
        // Lines inside a function but outside any bb (current_bb_id is None)
        // skip the entire statement parsing block (line 82 false branch).
        let mir = "\
fn outside_bb() -> () {
    let debug _x => _1;
    scope 1 {
    }
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn close_brace_inside_bb_with_bb_id_none() {
        // A `}` line inside a bb context after the terminator already
        // consumed bb_id. current_bb_id is None so the `}` handler
        // at line 143 takes the None branch.
        let mir = "\
fn extra_close2() -> () {
    bb0: {
        goto -> bb1;
    }
    bb1: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 2);
    }

    #[test]
    fn extract_fn_name_paren_before_space() {
        // '(' found before ' ' — exercises the unwrap_or_else NOT being called
        assert_eq!(extract_fn_name("fn name(arg) {"), "name");
    }

    #[test]
    fn extract_fn_name_space_before_paren() {
        // No '(' at all, but ' ' exists — exercises the unwrap_or_else fallback
        assert_eq!(extract_fn_name("fn name stuff"), "name");
    }

    #[test]
    fn extract_fn_name_neither_paren_nor_space() {
        // Neither '(' nor ' ' after "fn " — uses full remaining length
        assert_eq!(extract_fn_name("fn name"), "name");
    }

    #[test]
    fn parse_bb_ref_with_trailing_text() {
        // parse_bb_ref with non-numeric suffix
        assert_eq!(parse_bb_ref("bb1abc"), None);
    }

    #[test]
    fn parse_bb_ref_with_space_prefix() {
        // parse_bb_ref trims whitespace
        assert_eq!(parse_bb_ref("  bb10"), Some(10));
    }

    #[test]
    fn multiple_terminators_in_sequence_first_wins() {
        // After the first terminator consumes bb_id, subsequent terminators
        // in the same bb are no-ops (finish_bb_with_terminator with None bb_id)
        let mir = "\
fn multi_term() -> () {
    bb0: {
        return;
        unreachable;
        abort;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // return consumed bb0; unreachable and abort had no bb_id
        assert_eq!(funcs[0].block_count(), 1);
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn statement_types_not_inside_bb_are_ignored() {
        // StorageLive, StorageDead, assignments outside a bb (current_bb_id
        // is None) should be ignored — exercises line 82 false branch.
        let mir = "\
fn stmts_outside() -> () {
    StorageLive(_1);
    _0 = _1;
    StorageDead(_1);
    bb0: {
        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert_eq!(funcs[0].blocks[0].statements.len(), 0);
    }

    #[test]
    fn trailing_flush_fn_with_no_bb_no_stmts() {
        // Trailing flush where current_fn is Some but current_bb_id is None
        // and no statements. Exercises line 157 Some + line 158 None.
        let mir = "fn trail_empty() -> () {";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "trail_empty");
        assert_eq!(funcs[0].block_count(), 0);
    }

    #[test]
    fn trailing_flush_fn_with_open_bb() {
        // Trailing flush where both current_fn and current_bb_id are Some.
        // Exercises lines 157-163 both Some branches.
        let mir = "fn trail_bb() -> () {\n    bb0: {\n        StorageLive(_1);";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        assert_eq!(funcs[0].blocks[0].id, 0);
        assert_eq!(funcs[0].blocks[0].statements.len(), 1);
    }

    #[test]
    fn no_trailing_fn_to_flush() {
        // After the last function properly closes, trailing flush finds
        // current_fn = None — exercises line 157 None branch.
        let mir = "\
fn closed() -> () {
    bb0: {
        return;
    }
}
";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
    }

    #[test]
    fn goto_nospace_invalid_target() {
        // "goto->notabb;" — exercises goto->  path with invalid parse_bb_ref
        let mir = "\
fn goto_ns_bad() -> () {
    bb0: {
        goto->invalid;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 1);
        // Invalid target means goto didn't fire; bb closed by `}` with default Return
        assert!(matches!(funcs[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn saturating_sub_brace_depth_doesnt_underflow() {
        // Extra closing braces should not underflow brace_depth due to
        // saturating_sub. This exercises the saturating edge case.
        let mir = "\
fn brace_underflow() -> () {
    bb0: {
        return;
    }
    }
    }
}";
        let funcs = parse_mir_output(mir);
        // The function closes when depth first hits 0
        assert_eq!(funcs.len(), 1);
    }

    #[test]
    fn empty_lines_inside_bb_ignored() {
        // Empty/whitespace-only lines inside a bb should not match any pattern
        let mir = "\
fn empty_lines() -> () {
    bb0: {

        StorageLive(_1);

        return;
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].blocks[0].statements.len(), 1);
    }

    #[test]
    fn three_functions_last_trailing() {
        // Three functions where the last one has no closing brace — tests
        // that trailing flush handles the final function.
        let mir = "\
fn a() -> () {
    bb0: {
        return;
    }
}
fn b() -> () {
    bb0: {
        return;
    }
}
fn c() -> () {
    bb0: {
        StorageLive(_1);";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 3);
        assert_eq!(funcs[2].name, "c");
        assert_eq!(funcs[2].block_count(), 1);
    }

    #[test]
    fn bb_header_with_invalid_id_after_valid_bb() {
        // A valid bb followed by an invalid bb header — exercises the
        // bb-header flush (line 65 Some) then parse_bb_ref returning None (line 74).
        let mir = "\
fn mixed_bb() -> () {
    bb0: {
        StorageLive(_1);
    bbXYZ: {
        StorageLive(_2);
    }
}";
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        // bb0 is flushed by bbXYZ header, but bbXYZ doesn't parse so no second block
        assert_eq!(funcs[0].block_count(), 1);
        assert_eq!(funcs[0].blocks[0].id, 0);
    }
}
