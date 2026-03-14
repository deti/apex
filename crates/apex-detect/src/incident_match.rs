//! Incident Pattern Matching — matches errors against known incident patterns.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentFingerprint {
    pub id: String,
    pub error_pattern: String,
    pub service: Option<String>,
    pub root_cause: String,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentMatch {
    pub fingerprint: IncidentFingerprint,
    pub similarity: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentReport {
    pub matches: Vec<IncidentMatch>,
    pub query: String,
}

/// Match an error message against known incident fingerprints.
pub fn match_incidents(
    error_msg: &str,
    fingerprints: &[IncidentFingerprint],
) -> IncidentReport {
    let error_lower = error_msg.to_lowercase();
    let error_words: std::collections::HashSet<&str> =
        error_lower.split_whitespace().collect();

    let mut matches: Vec<IncidentMatch> = fingerprints
        .iter()
        .filter_map(|fp| {
            let pattern_lower = fp.error_pattern.to_lowercase();
            let pattern_words: std::collections::HashSet<&str> =
                pattern_lower.split_whitespace().collect();

            if error_words.is_empty() || pattern_words.is_empty() {
                return None;
            }
            let intersection = error_words.intersection(&pattern_words).count();
            let union = error_words.union(&pattern_words).count();
            let similarity = intersection as f64 / union as f64; // Jaccard

            if similarity > 0.2 {
                Some(IncidentMatch {
                    fingerprint: fp.clone(),
                    similarity,
                })
            } else {
                None
            }
        })
        .collect();

    matches.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    IncidentReport {
        matches,
        query: error_msg.to_string(),
    }
}

/// Parse incident history from JSON.
pub fn parse_incident_db(json: &str) -> Vec<IncidentFingerprint> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_db() -> Vec<IncidentFingerprint> {
        vec![
            IncidentFingerprint {
                id: "INC-1".into(),
                error_pattern: "ConnectionTimeout in PaymentService".into(),
                service: Some("payment".into()),
                root_cause: "Connection pool exhausted".into(),
                resolution: "Increase pool size".into(),
            },
            IncidentFingerprint {
                id: "INC-2".into(),
                error_pattern: "OutOfMemory in DataPipeline".into(),
                service: Some("data".into()),
                root_cause: "Unbounded batch size".into(),
                resolution: "Add batch size limit".into(),
            },
        ]
    }

    #[test]
    fn matches_similar_error() {
        let r = match_incidents(
            "ConnectionTimeout in PaymentService after 30s",
            &sample_db(),
        );
        assert!(!r.matches.is_empty());
        assert_eq!(r.matches[0].fingerprint.id, "INC-1");
    }

    #[test]
    fn no_match_for_unrelated_error() {
        let r = match_incidents("SyntaxError in test_utils.py", &sample_db());
        assert!(r.matches.is_empty());
    }

    #[test]
    fn empty_db_no_matches() {
        let r = match_incidents("some error", &[]);
        assert!(r.matches.is_empty());
    }

    #[test]
    fn parse_incident_json() {
        let json = r#"[{"id":"I1","error_pattern":"timeout","service":null,"root_cause":"slow","resolution":"fix"}]"#;
        let db = parse_incident_db(json);
        assert_eq!(db.len(), 1);
    }
}
