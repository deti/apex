/// S2F difficulty categories for uncovered branches.
///
/// Based on S2F (FSE 2024): categorizing branches into 5 classes allows
/// routing each to the optimal synthesis strategy, reducing wasted LLM calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BranchDifficulty {
    Trivial,
    DataFlow,
    ExceptionHandler,
    Concurrency,
    Infeasible,
}

pub struct BranchClassifier;

impl BranchClassifier {
    pub fn classify_source(snippet: &str) -> BranchDifficulty {
        if snippet.contains("thread") || snippet.contains("Lock") || snippet.contains("async") {
            return BranchDifficulty::Concurrency;
        }
        if snippet.contains("except") || snippet.contains("raise") || snippet.contains("catch") {
            return BranchDifficulty::ExceptionHandler;
        }
        if snippet.contains('[') || snippet.contains('.') && snippet.contains('>') {
            return BranchDifficulty::DataFlow;
        }
        BranchDifficulty::Trivial
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_source_classified_hard() {
        let diff = BranchClassifier::classify_source("except ValueError:\n    pass");
        assert_eq!(diff, BranchDifficulty::ExceptionHandler);
    }

    #[test]
    fn thread_source_classified_concurrency() {
        let diff = BranchClassifier::classify_source("threading.Lock()");
        assert_eq!(diff, BranchDifficulty::Concurrency);
    }

    #[test]
    fn simple_condition_classified_trivial() {
        let diff = BranchClassifier::classify_source("if x == 1:");
        assert_eq!(diff, BranchDifficulty::Trivial);
    }

    #[test]
    fn data_flow_via_multiple_assignments() {
        let diff = BranchClassifier::classify_source("if result[0] > threshold:");
        assert_eq!(diff, BranchDifficulty::DataFlow);
    }

    #[test]
    fn all_variants_distinct() {
        use std::collections::HashSet;
        let variants = [BranchDifficulty::Trivial, BranchDifficulty::DataFlow,
                        BranchDifficulty::ExceptionHandler, BranchDifficulty::Concurrency,
                        BranchDifficulty::Infeasible];
        let set: HashSet<_> = variants.iter().collect();
        assert_eq!(set.len(), 5);
    }
}
