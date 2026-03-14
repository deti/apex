#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapKind { ExceptionHandler, BoundaryCondition, NullCheck, General }

pub struct GapClassifier;

impl GapClassifier {
    pub fn classify_source(snippet: &str) -> GapKind {
        if snippet.contains("except") || snippet.contains("catch") { return GapKind::ExceptionHandler; }
        if snippet.contains("None") || snippet.contains("null") || snippet.contains("nil") { return GapKind::NullCheck; }
        if snippet.contains('>') || snippet.contains('<') || snippet.contains(">=") || snippet.contains("<=") { return GapKind::BoundaryCondition; }
        GapKind::General
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_gap_classified_from_source() {
        let kind = GapClassifier::classify_source("try:\n    x = int(s)\nexcept ValueError:");
        assert_eq!(kind, GapKind::ExceptionHandler);
    }

    #[test]
    fn boundary_gap_classified_from_source() {
        let kind = GapClassifier::classify_source("if x > 100:");
        assert_eq!(kind, GapKind::BoundaryCondition);
    }

    #[test]
    fn unknown_gap_classified_as_general() {
        let kind = GapClassifier::classify_source("pass");
        assert_eq!(kind, GapKind::General);
    }
}
