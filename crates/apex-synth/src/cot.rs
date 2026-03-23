//! Chain-of-Thought prompt wrapper for LLM-guided test synthesis.
//!
//! Wraps the standard CoverUp prompt with reasoning instructions so the LLM
//! analyses path conditions before generating test code.

use crate::{CoverageGap, LlmMessage, LlmRole};

/// Build a chain-of-thought prompt for a coverage gap.
///
/// The system message instructs the LLM to reason step-by-step about what
/// inputs would exercise the uncovered lines before producing test code.
pub fn build_cot_prompt(gap: &CoverageGap) -> Vec<LlmMessage> {
    let system = LlmMessage {
        role: LlmRole::System,
        content: "You are an expert test developer. Before writing the test, \
                  think step-by-step about what inputs and conditions are needed \
                  to reach the uncovered lines. Then write the test. Respond with \
                  your reasoning followed by the test code in a code block."
            .to_string(),
    };

    let fn_hint = gap
        .function_name
        .as_deref()
        .map(|n| format!(" (function `{n}`)"))
        .unwrap_or_default();

    let lines_desc = if gap.uncovered_lines.is_empty() {
        format!("line {}", gap.target_line)
    } else {
        let parts: Vec<String> = gap.uncovered_lines.iter().map(|l| l.to_string()).collect();
        format!("lines {}", parts.join(", "))
    };

    let user = LlmMessage {
        role: LlmRole::User,
        content: format!(
            "File: {file}{fn_hint}\n\
             Uncovered: {lines_desc}\n\n\
             Source segment:\n```\n{segment}\n```\n\n\
             Think step-by-step about what inputs exercise {lines_desc}, \
             then write a test for {file}.",
            file = gap.file_path,
            fn_hint = fn_hint,
            lines_desc = lines_desc,
            segment = gap.source_segment,
        ),
    };

    vec![system, user]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoverageGap, LlmRole};

    fn make_gap() -> CoverageGap {
        CoverageGap {
            file_path: "app.py".into(),
            target_line: 10,
            function_name: Some("process".into()),
            source_segment: "if x > 0:\n    return x\n".into(),
            uncovered_lines: vec![11],
            cpg_context: None,
        }
    }

    #[test]
    fn cot_prompt_includes_reasoning_instruction() {
        let messages = build_cot_prompt(&make_gap());
        let system = &messages[0];
        assert_eq!(system.role, LlmRole::System);
        assert!(system.content.contains("step-by-step"));
    }

    #[test]
    fn cot_prompt_includes_source_segment() {
        let messages = build_cot_prompt(&make_gap());
        let user = &messages[1];
        assert_eq!(user.role, LlmRole::User);
        assert!(user.content.contains("if x > 0"));
    }

    #[test]
    fn cot_prompt_mentions_target_lines() {
        let messages = build_cot_prompt(&make_gap());
        let user = &messages[1];
        assert!(user.content.contains("11"));
    }
}
