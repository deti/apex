/// Pure-Rust mutation engine — no libafl dependency required.
///
/// Each function takes an input slice and an RNG, returns a mutated `Vec<u8>`.
/// `havoc` chains multiple random operators for deeper exploration.
use rand::Rng;

// ---------------------------------------------------------------------------
// Primitive operators
// ---------------------------------------------------------------------------

pub fn bit_flip(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    if data.is_empty() {
        return vec![0];
    }
    let mut out = data.to_vec();
    let pos = rng.random_range(0..out.len());
    let bit = rng.random_range(0..8u8);
    out[pos] ^= 1 << bit;
    out
}

pub fn byte_flip(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    if data.is_empty() {
        return vec![0xFF];
    }
    let mut out = data.to_vec();
    let pos = rng.random_range(0..out.len());
    out[pos] ^= 0xFF;
    out
}

pub fn byte_arith(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    if data.is_empty() {
        return vec![rng.random()];
    }
    let mut out = data.to_vec();
    let pos = rng.random_range(0..out.len());
    // Small arithmetic delta in range ±35 (AFL++ heuristic)
    let delta: i8 = rng.random_range(-35..=35);
    out[pos] = (out[pos] as i16 + delta as i16).clamp(0, 255) as u8;
    out
}

/// Replace a byte with an "interesting" value (boundaries, zeros, maxima).
pub fn interesting_byte(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    const INTERESTING: &[u8] = &[0, 1, 7, 8, 16, 32, 64, 127, 128, 192, 254, 255];
    if data.is_empty() {
        return vec![INTERESTING[rng.random_range(0..INTERESTING.len())]];
    }
    let mut out = data.to_vec();
    let pos = rng.random_range(0..out.len());
    out[pos] = INTERESTING[rng.random_range(0..INTERESTING.len())];
    out
}

pub fn insert_byte(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    let pos = if data.is_empty() {
        0
    } else {
        rng.random_range(0..=data.len())
    };
    let val: u8 = rng.random();
    let mut out = Vec::with_capacity(data.len() + 1);
    out.extend_from_slice(&data[..pos]);
    out.push(val);
    out.extend_from_slice(&data[pos..]);
    out
}

pub fn delete_byte(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    if data.len() <= 1 {
        return data.to_vec();
    }
    let pos = rng.random_range(0..data.len());
    let mut out = data.to_vec();
    out.remove(pos);
    out
}

pub fn duplicate_block(data: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    if data.len() < 2 {
        return data.to_vec();
    }
    let start = rng.random_range(0..data.len() - 1);
    let end = rng.random_range(start + 1..data.len());
    let insert_at = rng.random_range(0..=data.len());
    let block = data[start..end].to_vec();
    let mut out = Vec::with_capacity(data.len() + block.len());
    out.extend_from_slice(&data[..insert_at]);
    out.extend_from_slice(&block);
    out.extend_from_slice(&data[insert_at..]);
    out
}

/// Cross-over: concatenate the first part of `a` with the tail of `b`.
pub fn splice(a: &[u8], b: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let split_a = rng.random_range(0..a.len());
    let split_b = rng.random_range(0..b.len());
    let mut out = a[..split_a].to_vec();
    out.extend_from_slice(&b[split_b..]);
    out
}

// ---------------------------------------------------------------------------
// Havoc — chain N random operators
// ---------------------------------------------------------------------------

pub fn havoc(data: &[u8], rng: &mut impl Rng, ops: usize) -> Vec<u8> {
    let mut cur = data.to_vec();
    for _ in 0..ops {
        cur = match rng.random_range(0..7u8) {
            0 => bit_flip(&cur, rng),
            1 => byte_flip(&cur, rng),
            2 => byte_arith(&cur, rng),
            3 => interesting_byte(&cur, rng),
            4 => insert_byte(&cur, rng),
            5 => delete_byte(&cur, rng),
            _ => duplicate_block(&cur, rng),
        };
    }
    cur
}

// ---------------------------------------------------------------------------
// Trait impls — wrap free functions as `dyn Mutator` objects
// ---------------------------------------------------------------------------

use crate::traits::Mutator;
use rand::RngCore;

/// Wrapper to use `&mut dyn RngCore` where `impl Rng` is expected.
struct RngCoreWrapper<'a>(&'a mut dyn RngCore);

impl rand::RngCore for RngCoreWrapper<'_> {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest)
    }
}

macro_rules! mutator_struct {
    ($name:ident, $func:ident, $label:expr) => {
        pub struct $name;
        impl Mutator for $name {
            fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
                let mut wrapper = RngCoreWrapper(rng);
                $func(input, &mut wrapper)
            }
            fn name(&self) -> &str {
                $label
            }
        }
    };
}

mutator_struct!(BitFlipMutator, bit_flip, "bit_flip");
mutator_struct!(ByteFlipMutator, byte_flip, "byte_flip");
mutator_struct!(ByteArithMutator, byte_arith, "byte_arith");
mutator_struct!(InterestingByteMutator, interesting_byte, "interesting_byte");
mutator_struct!(InsertByteMutator, insert_byte, "insert_byte");
mutator_struct!(DeleteByteMutator, delete_byte, "delete_byte");
mutator_struct!(DuplicateBlockMutator, duplicate_block, "duplicate_block");

/// All 7 built-in mutators as trait objects.
pub fn builtin_mutators() -> Vec<Box<dyn Mutator>> {
    vec![
        Box::new(BitFlipMutator),
        Box::new(ByteFlipMutator),
        Box::new(ByteArithMutator),
        Box::new(InterestingByteMutator),
        Box::new(InsertByteMutator),
        Box::new(DeleteByteMutator),
        Box::new(DuplicateBlockMutator),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    #[test]
    fn bit_flip_changes_one_bit() {
        let input = vec![0u8; 8];
        let out = bit_flip(&input, &mut rng());
        let changed: u32 = input
            .iter()
            .zip(out.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum();
        assert_eq!(changed, 1);
    }

    #[test]
    fn byte_flip_flips_all_bits() {
        let input = vec![0b10101010u8; 4];
        let out = byte_flip(&input, &mut rng());
        assert!(out.iter().zip(input.iter()).any(|(a, b)| a ^ b == 0xFF));
    }

    #[test]
    fn havoc_empty_input() {
        let out = havoc(&[], &mut rng(), 10);
        // Should not panic, length ≥ 0
        let _ = out;
    }

    #[test]
    fn splice_combines_both() {
        let a = b"AAAAAAA".to_vec();
        let b = b"BBBBBBB".to_vec();
        let out = splice(&a, &b, &mut rng());
        assert!(!out.is_empty());
    }

    #[test]
    fn byte_arith_stays_in_bounds() {
        let input = vec![0u8, 255u8];
        for seed in 0..100u64 {
            let mut r = StdRng::seed_from_u64(seed);
            let out = byte_arith(&input, &mut r);
            assert_eq!(out.len(), 2);
            // All values must be valid u8 (0..=255) — no overflow/underflow
        }
    }

    #[test]
    fn interesting_byte_uses_boundary() {
        let input = vec![100u8; 4];
        let out = interesting_byte(&input, &mut rng());
        let interesting = [0, 1, 7, 8, 16, 32, 64, 127, 128, 192, 254, 255];
        // At least one byte should be from the interesting set
        assert!(out.iter().any(|b| interesting.contains(b)));
    }

    #[test]
    fn insert_byte_grows_by_one() {
        let input = vec![1, 2, 3];
        let out = insert_byte(&input, &mut rng());
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn delete_byte_shrinks_by_one() {
        let input = vec![1, 2, 3, 4];
        let out = delete_byte(&input, &mut rng());
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn delete_byte_single_element_unchanged() {
        let input = vec![42];
        let out = delete_byte(&input, &mut rng());
        assert_eq!(out, vec![42]);
    }

    #[test]
    fn duplicate_block_grows() {
        let input = vec![1, 2, 3, 4, 5];
        let out = duplicate_block(&input, &mut rng());
        assert!(out.len() > input.len());
    }

    #[test]
    fn duplicate_block_tiny_input_unchanged() {
        let input = vec![1];
        let out = duplicate_block(&input, &mut rng());
        assert_eq!(out, vec![1]);
    }

    #[test]
    fn splice_empty_a_returns_b() {
        let out = splice(&[], b"hello", &mut rng());
        assert_eq!(out, b"hello");
    }

    #[test]
    fn splice_empty_b_returns_a() {
        let out = splice(b"hello", &[], &mut rng());
        assert_eq!(out, b"hello");
    }

    #[test]
    fn bit_flip_empty_returns_zero() {
        let out = bit_flip(&[], &mut rng());
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn byte_flip_empty_returns_ff() {
        let out = byte_flip(&[], &mut rng());
        assert_eq!(out, vec![0xFF]);
    }

    #[test]
    fn havoc_deterministic_with_seed() {
        let input = b"test input data";
        let a = havoc(input, &mut StdRng::seed_from_u64(99), 5);
        let b = havoc(input, &mut StdRng::seed_from_u64(99), 5);
        assert_eq!(a, b);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_bit_flip_preserves_length(data in proptest::collection::vec(any::<u8>(), 1..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = bit_flip(&data, &mut rng);
            prop_assert_eq!(out.len(), data.len());
        }

        #[test]
        fn prop_byte_flip_preserves_length(data in proptest::collection::vec(any::<u8>(), 1..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = byte_flip(&data, &mut rng);
            prop_assert_eq!(out.len(), data.len());
        }

        #[test]
        fn prop_byte_arith_preserves_length(data in proptest::collection::vec(any::<u8>(), 1..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = byte_arith(&data, &mut rng);
            prop_assert_eq!(out.len(), data.len());
        }

        #[test]
        fn prop_interesting_byte_preserves_length(data in proptest::collection::vec(any::<u8>(), 1..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = interesting_byte(&data, &mut rng);
            prop_assert_eq!(out.len(), data.len());
        }

        #[test]
        fn prop_insert_byte_grows_by_one(data in proptest::collection::vec(any::<u8>(), 0..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = insert_byte(&data, &mut rng);
            prop_assert_eq!(out.len(), data.len() + 1);
        }

        #[test]
        fn prop_delete_byte_shrinks_by_one(data in proptest::collection::vec(any::<u8>(), 2..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = delete_byte(&data, &mut rng);
            prop_assert_eq!(out.len(), data.len() - 1);
        }

        #[test]
        fn prop_havoc_never_panics(
            data in proptest::collection::vec(any::<u8>(), 0..64),
            ops in 0..20usize,
            seed in any::<u64>()
        ) {
            let mut rng = StdRng::seed_from_u64(seed);
            let _ = havoc(&data, &mut rng, ops);
        }

        #[test]
        fn prop_splice_bounded(
            a in proptest::collection::vec(any::<u8>(), 0..128),
            b in proptest::collection::vec(any::<u8>(), 0..128),
            seed in any::<u64>()
        ) {
            let mut rng = StdRng::seed_from_u64(seed);
            let out = splice(&a, &b, &mut rng);
            // splice output can't be longer than a + b combined
            prop_assert!(out.len() <= a.len() + b.len());
        }

        #[test]
        fn prop_duplicate_block_non_empty(
            data in proptest::collection::vec(any::<u8>(), 2..128),
            seed in any::<u64>()
        ) {
            let mut rng = StdRng::seed_from_u64(seed);
            let out = duplicate_block(&data, &mut rng);
            prop_assert!(out.len() >= data.len());
        }

        #[test]
        fn prop_bit_flip_changes_at_most_one_bit(data in proptest::collection::vec(any::<u8>(), 1..256)) {
            let mut rng = StdRng::seed_from_u64(42);
            let out = bit_flip(&data, &mut rng);
            let total_changed_bits: u32 = data.iter()
                .zip(out.iter())
                .map(|(a, b)| (a ^ b).count_ones())
                .sum();
            prop_assert_eq!(total_changed_bits, 1);
        }
    }

    #[test]
    fn byte_arith_empty_returns_random() {
        let out = byte_arith(&[], &mut rng());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn interesting_byte_empty_returns_interesting() {
        let out = interesting_byte(&[], &mut rng());
        assert_eq!(out.len(), 1);
        let interesting = [0, 1, 7, 8, 16, 32, 64, 127, 128, 192, 254, 255];
        assert!(interesting.contains(&out[0]));
    }

    #[test]
    fn insert_byte_empty_input() {
        let out = insert_byte(&[], &mut rng());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn duplicate_block_empty_input() {
        let out = duplicate_block(&[], &mut rng());
        assert!(out.is_empty());
    }

    #[test]
    fn delete_byte_empty_input() {
        let out = delete_byte(&[], &mut rng());
        assert!(out.is_empty());
    }

    #[test]
    fn splice_both_empty() {
        let out = splice(&[], &[], &mut rng());
        assert!(out.is_empty());
    }

    #[test]
    fn havoc_zero_ops() {
        let input = b"unchanged";
        let out = havoc(input, &mut rng(), 0);
        assert_eq!(out, input);
    }

    #[test]
    fn havoc_single_op() {
        let input = b"test";
        let out = havoc(input, &mut rng(), 1);
        // Should produce something (may or may not differ)
        let _ = out;
    }

    #[test]
    fn builtin_mutator_bit_flip_via_trait() {
        let m = BitFlipMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert_eq!(out.len(), 4);
        assert_eq!(m.name(), "bit_flip");
    }

    #[test]
    fn builtin_mutator_byte_flip_via_trait() {
        let m = ByteFlipMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert_eq!(out.len(), 4);
        assert_eq!(m.name(), "byte_flip");
    }

    #[test]
    fn builtin_mutator_byte_arith_via_trait() {
        let m = ByteArithMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert_eq!(out.len(), 4);
        assert_eq!(m.name(), "byte_arith");
    }

    #[test]
    fn builtin_mutator_interesting_byte_via_trait() {
        let m = InterestingByteMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert_eq!(out.len(), 4);
        assert_eq!(m.name(), "interesting_byte");
    }

    #[test]
    fn builtin_mutator_insert_byte_via_trait() {
        let m = InsertByteMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert_eq!(out.len(), 5);
        assert_eq!(m.name(), "insert_byte");
    }

    #[test]
    fn builtin_mutator_delete_byte_via_trait() {
        let m = DeleteByteMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert_eq!(out.len(), 3);
        assert_eq!(m.name(), "delete_byte");
    }

    #[test]
    fn builtin_mutator_duplicate_block_via_trait() {
        let m = DuplicateBlockMutator;
        let mut r = rand::rng();
        let out = m.mutate(b"test", &mut r);
        assert!(out.len() >= 4);
        assert_eq!(m.name(), "duplicate_block");
    }

    // Exercise RngCoreWrapper::fill_bytes and try_fill_bytes directly.
    // These are the two branches reported uncovered (lines 147-152).
    #[test]
    fn rng_core_wrapper_fill_bytes() {
        use rand::RngCore;
        let mut inner = StdRng::seed_from_u64(1);
        let mut wrapper = RngCoreWrapper(&mut inner);
        let mut dest = [0u8; 16];
        wrapper.fill_bytes(&mut dest);
        // After fill_bytes, dest should no longer be all-zero (probabilistically true)
        assert_ne!(
            dest, [0u8; 16],
            "fill_bytes should have written random data"
        );
    }

    // try_fill_bytes was removed from RngCore in rand 0.9 (moved to TryRngCore)

    #[test]
    fn all_builtin_mutators_implement_trait() {
        use crate::traits::Mutator;
        let mutators: Vec<Box<dyn Mutator>> = builtin_mutators();
        assert_eq!(mutators.len(), 7);
        let names: Vec<&str> = mutators.iter().map(|m| m.name()).collect();
        assert!(names.contains(&"bit_flip"));
        assert!(names.contains(&"byte_flip"));
        assert!(names.contains(&"byte_arith"));
        assert!(names.contains(&"interesting_byte"));
        assert!(names.contains(&"insert_byte"));
        assert!(names.contains(&"delete_byte"));
        assert!(names.contains(&"duplicate_block"));
    }
}
