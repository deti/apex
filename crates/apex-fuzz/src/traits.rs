//! Mutator trait for pluggable mutation operators.

use rand::RngCore;

/// A single mutation operator that transforms input bytes.
pub trait Mutator: Send + Sync {
    /// Apply this mutation to `input`, returning a new byte vector.
    fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8>;

    /// Human-readable name for logging and scheduling stats.
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct IdentityMutator;
    impl Mutator for IdentityMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            input.to_vec()
        }
        fn name(&self) -> &str {
            "identity"
        }
    }

    #[test]
    fn identity_mutator_preserves_input() {
        let m = IdentityMutator;
        let mut rng = rand::thread_rng();
        assert_eq!(m.mutate(b"hello", &mut rng), b"hello");
    }

    #[test]
    fn mutator_name() {
        let m = IdentityMutator;
        assert_eq!(m.name(), "identity");
    }

    #[test]
    fn mutator_empty_input() {
        let m = IdentityMutator;
        let mut rng = rand::thread_rng();
        assert_eq!(m.mutate(b"", &mut rng), b"");
    }

    #[test]
    fn mutator_is_object_safe() {
        let m: Box<dyn Mutator> = Box::new(IdentityMutator);
        let mut rng = rand::thread_rng();
        let _ = m.mutate(b"test", &mut rng);
    }
}
