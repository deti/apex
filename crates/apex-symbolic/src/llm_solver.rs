//! LLM-based constraint solver — uses a language model to solve constraints
//! when traditional SMT solvers fail or time out.
//! Based on the Cottontail paper.

use apex_core::types::InputSeed;
use apex_core::types::SeedOrigin;

/// Convert SMTLIB2 constraints to a natural language prompt for an LLM.
pub fn constraints_to_prompt(constraints: &[String], negate_last: bool) -> String {
    if constraints.is_empty() {
        return "There are no constraints to solve. Reply with an empty JSON object: {}. \
                Note: there are no constraints here."
            .to_string();
    }

    let mut prompt = String::new();
    prompt.push_str(
        "You are an SMT solver. Given the following SMTLIB2 constraints, \
         find integer values for all variables that satisfy ALL constraints.\n\n",
    );

    prompt.push_str("Constraints:\n");
    for (i, c) in constraints.iter().enumerate() {
        let is_last = i == constraints.len() - 1;
        if is_last && negate_last {
            prompt.push_str(&format!("  {}. (negate this) {}\n", i + 1, c));
        } else {
            prompt.push_str(&format!("  {}. {}\n", i + 1, c));
        }
    }

    if negate_last {
        prompt.push_str(
            "\nIMPORTANT: The last constraint must be NEGATED. Find values that satisfy \
             constraints 1..N-1 AND the negation of constraint N.\n",
        );
    }

    prompt.push_str(
        "\nReply with ONLY a JSON object mapping variable names to integer values. \
         Example: {\"x\": 42, \"y\": -5}\n\
         If unsatisfiable, reply with: UNSAT\n",
    );

    prompt
}

/// Parse an LLM response into an InputSeed.
///
/// Tries to extract a JSON object `{"var": value, ...}` from the response.
/// Handles responses with markdown code fences.
pub fn parse_llm_solution(response: &str) -> Option<InputSeed> {
    let trimmed = response.trim();

    // Try direct JSON parse first
    if let Some(seed) = try_parse_json_object(trimmed) {
        return Some(seed);
    }

    // Try extracting from markdown code fence
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        // Skip optional language tag (e.g., "json\n")
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            let json_str = content[..end].trim();
            return try_parse_json_object(json_str);
        }
    }

    None
}

fn try_parse_json_object(s: &str) -> Option<InputSeed> {
    let parsed: serde_json::Value = serde_json::from_str(s).ok()?;
    let obj = parsed.as_object()?;

    if obj.is_empty() {
        return None;
    }

    let json_bytes = serde_json::to_vec(&parsed).ok()?;
    Some(InputSeed::new(json_bytes, SeedOrigin::Symbolic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_to_prompt_simple() {
        let constraints = vec!["(> x 0)".to_string(), "(< x 100)".to_string()];
        let prompt = constraints_to_prompt(&constraints, false);
        assert!(prompt.contains("(> x 0)"));
        assert!(prompt.contains("(< x 100)"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn constraints_to_prompt_negate_last() {
        let constraints = vec!["(> x 0)".to_string(), "(< x 100)".to_string()];
        let prompt = constraints_to_prompt(&constraints, true);
        assert!(prompt.contains("negate"));
        assert!(prompt.contains("(< x 100)"));
    }

    #[test]
    fn constraints_to_prompt_empty() {
        let prompt = constraints_to_prompt(&[], false);
        assert!(prompt.contains("no constraints"));
    }

    #[test]
    fn parse_llm_solution_valid_json() {
        let response = r#"{"x": 42, "y": -5}"#;
        let seed = parse_llm_solution(response);
        assert!(seed.is_some());
        let data = String::from_utf8(seed.unwrap().data.to_vec()).unwrap();
        assert!(data.contains("42"));
    }

    #[test]
    fn parse_llm_solution_invalid() {
        let seed = parse_llm_solution("I cannot solve this");
        assert!(seed.is_none());
    }

    #[test]
    fn parse_llm_solution_json_in_markdown() {
        let response = "Here is the solution:\n```json\n{\"x\": 10}\n```\n";
        let seed = parse_llm_solution(response);
        assert!(seed.is_some());
    }

    #[test]
    fn parse_llm_solution_empty_object() {
        let seed = parse_llm_solution("{}");
        assert!(seed.is_none()); // empty object = no assignments
    }
}
