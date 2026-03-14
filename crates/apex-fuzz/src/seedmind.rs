//! SeedMind — LLM-guided seed generation targeting uncovered branches.
//! Based on the SeedMind paper.

use apex_core::types::BranchId;

/// Build a prompt asking an LLM to generate targeted seed inputs.
pub fn build_seed_prompt(uncovered: &[BranchId], target_format: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!(
        "You are a fuzzing seed generator. Generate test inputs in {target_format} format \
         that are likely to exercise specific code paths.\n\n"
    ));

    if uncovered.is_empty() {
        prompt.push_str(
            "There are no specific uncovered branches. Generate diverse, \
             boundary-testing inputs.\n",
        );
    } else {
        prompt.push_str("Target these uncovered branches:\n");
        for b in uncovered.iter().take(20) {
            prompt.push_str(&format!(
                "- file_id={}, line {}, direction {}\n",
                b.file_id, b.line, b.direction
            ));
        }
        if uncovered.len() > 20 {
            prompt.push_str(&format!("  ... and {} more\n", uncovered.len() - 20));
        }
    }

    prompt.push_str(&format!(
        "\nGenerate 5 diverse {target_format} inputs as a JSON array. \
         Include boundary values, empty inputs, large inputs, and edge cases.\n"
    ));

    prompt
}

/// Parse an LLM response into seed byte vectors.
pub fn parse_seed_response(response: &str) -> Vec<Vec<u8>> {
    let trimmed = response.trim();

    // Try direct parse
    if let Some(seeds) = try_parse_seeds(trimmed) {
        return seeds;
    }

    // Try extracting from markdown code fence
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            let json_str = content[..end].trim();
            if let Some(seeds) = try_parse_seeds(json_str) {
                return seeds;
            }
        }
    }

    vec![]
}

fn try_parse_seeds(s: &str) -> Option<Vec<Vec<u8>>> {
    let parsed: serde_json::Value = serde_json::from_str(s).ok()?;

    match parsed {
        serde_json::Value::Array(arr) => {
            let seeds: Vec<Vec<u8>> = arr
                .iter()
                .filter_map(|v| serde_json::to_vec(v).ok())
                .collect();
            if seeds.is_empty() {
                None
            } else {
                Some(seeds)
            }
        }
        serde_json::Value::Object(_) => {
            let bytes = serde_json::to_vec(&parsed).ok()?;
            Some(vec![bytes])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_seed_prompt_includes_format() {
        let uncovered = vec![BranchId::new(1, 10, 0, 0), BranchId::new(1, 20, 0, 1)];
        let prompt = build_seed_prompt(&uncovered, "JSON");
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("line 10"));
        assert!(prompt.contains("line 20"));
    }

    #[test]
    fn build_seed_prompt_empty_uncovered() {
        let prompt = build_seed_prompt(&[], "binary");
        assert!(prompt.contains("no specific"));
    }

    #[test]
    fn parse_seed_response_json_array() {
        let response = r#"[{"key": "value"}, {"key": "other"}]"#;
        let seeds = parse_seed_response(response);
        assert_eq!(seeds.len(), 2);
    }

    #[test]
    fn parse_seed_response_single_object() {
        let response = r#"{"input": 42}"#;
        let seeds = parse_seed_response(response);
        assert_eq!(seeds.len(), 1);
    }

    #[test]
    fn parse_seed_response_invalid() {
        let seeds = parse_seed_response("not json at all");
        assert!(seeds.is_empty());
    }

    #[test]
    fn parse_seed_response_with_markdown() {
        let response = "Here are seeds:\n```json\n[{\"x\": 1}]\n```\n";
        let seeds = parse_seed_response(response);
        assert_eq!(seeds.len(), 1);
    }
}
