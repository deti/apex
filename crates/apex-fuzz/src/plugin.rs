//! Custom mutator plugin registry.

use crate::traits::Mutator;

/// A registry that holds pluggable [`Mutator`] implementations.
pub struct MutatorRegistry {
    mutators: Vec<Box<dyn Mutator>>,
}

impl MutatorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        MutatorRegistry {
            mutators: Vec::new(),
        }
    }

    /// Register a new mutator.
    pub fn register(&mut self, mutator: Box<dyn Mutator>) {
        self.mutators.push(mutator);
    }

    /// Return a slice of all registered mutators.
    pub fn mutators(&self) -> &[Box<dyn Mutator>] {
        &self.mutators
    }

    /// Number of registered mutators.
    pub fn len(&self) -> usize {
        self.mutators.len()
    }

    /// Returns `true` if no mutators are registered.
    pub fn is_empty(&self) -> bool {
        self.mutators.is_empty()
    }
}

impl Default for MutatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    struct UpperMutator;
    impl Mutator for UpperMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            input.iter().map(|b| b.to_ascii_uppercase()).collect()
        }
        fn name(&self) -> &str {
            "upper"
        }
    }

    struct ReverseMutator;
    impl Mutator for ReverseMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            input.iter().rev().copied().collect()
        }
        fn name(&self) -> &str {
            "reverse"
        }
    }

    #[test]
    fn registry_new_is_empty() {
        let reg = MutatorRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_and_retrieve() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(UpperMutator));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        assert_eq!(reg.mutators()[0].name(), "upper");
    }

    #[test]
    fn multiple_mutators() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(UpperMutator));
        reg.register(Box::new(ReverseMutator));
        assert_eq!(reg.len(), 2);
        let names: Vec<&str> = reg.mutators().iter().map(|m| m.name()).collect();
        assert_eq!(names, vec!["upper", "reverse"]);
    }

    #[test]
    fn registry_default_is_empty() {
        let reg = MutatorRegistry::default();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registered_mutator_works() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(UpperMutator));
        let mut rng = rand::thread_rng();
        let result = reg.mutators()[0].mutate(b"hello", &mut rng);
        assert_eq!(result, b"HELLO");
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn registry_len_matches_register_calls() {
        let mut reg = MutatorRegistry::new();
        for i in 0..5 {
            let _ = i; // suppress warning
            reg.register(Box::new(UpperMutator));
        }
        assert_eq!(reg.len(), 5);
        assert!(!reg.is_empty());
    }

    #[test]
    fn registry_default_and_new_are_equivalent() {
        let a = MutatorRegistry::new();
        let b = MutatorRegistry::default();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.is_empty(), b.is_empty());
    }

    #[test]
    fn registry_mutators_returns_all_registered() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(UpperMutator));
        reg.register(Box::new(ReverseMutator));
        let names: Vec<&str> = reg.mutators().iter().map(|m| m.name()).collect();
        assert!(names.contains(&"upper"));
        assert!(names.contains(&"reverse"));
    }

    #[test]
    fn registry_reverse_mutator_works() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(ReverseMutator));
        let mut rng = rand::thread_rng();
        let result = reg.mutators()[0].mutate(b"abcd", &mut rng);
        assert_eq!(result, b"dcba");
    }

    #[test]
    fn registry_empty_mutators_slice_is_empty() {
        let reg = MutatorRegistry::new();
        assert!(reg.mutators().is_empty());
    }
}
