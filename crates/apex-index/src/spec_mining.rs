//! Data transform specification mining.
//! Based on "Beyond Bools" paper — learns input/output relationships
//! beyond boolean pass/fail from test executions.

use serde::{Deserialize, Serialize};

/// A learned specification of a function's input/output behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformSpec {
    pub function_name: String,
    pub input_output_pairs: Vec<(String, String)>,
    pub inferred_properties: Vec<String>,
}

/// Infer high-level properties from input/output pairs.
///
/// Currently checks:
/// - `length_preserved`: input and output have the same string length.
/// - `idempotent`: output equals input for all pairs.
pub fn infer_properties(pairs: &[(String, String)]) -> Vec<String> {
    if pairs.is_empty() {
        return vec![];
    }

    let mut properties = Vec::new();

    // Check length preservation
    let all_length_preserved = pairs.iter().all(|(i, o)| i.len() == o.len());
    if all_length_preserved {
        properties.push("length_preserved".to_string());
    }

    // Check idempotency
    let all_idempotent = pairs.iter().all(|(i, o)| i == o);
    if all_idempotent {
        properties.push("idempotent".to_string());
    }

    properties
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_spec_creation() {
        let spec = TransformSpec {
            function_name: "sort".to_string(),
            input_output_pairs: vec![(r#"[3,1,2]"#.to_string(), r#"[1,2,3]"#.to_string())],
            inferred_properties: vec![],
        };
        assert_eq!(spec.function_name, "sort");
    }

    #[test]
    fn infer_length_preservation() {
        let pairs = vec![
            ("[1,2,3]".to_string(), "[3,2,1]".to_string()),
            ("[1]".to_string(), "[1]".to_string()),
            ("[5,4,3,2,1]".to_string(), "[1,2,3,4,5]".to_string()),
        ];
        let props = infer_properties(&pairs);
        assert!(props.contains(&"length_preserved".to_string()));
    }

    #[test]
    fn infer_no_properties_from_empty() {
        let props = infer_properties(&[]);
        assert!(props.is_empty());
    }

    #[test]
    fn infer_idempotent() {
        let pairs = vec![
            ("hello".to_string(), "hello".to_string()),
            ("world".to_string(), "world".to_string()),
        ];
        let props = infer_properties(&pairs);
        assert!(props.contains(&"idempotent".to_string()));
    }
}
