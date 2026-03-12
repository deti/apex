use apex_core::types::InputSeed;
use std::sync::Mutex;

/// Bidirectional seed exchange for fuzz <-> concolic feedback loop.
pub struct SeedExchange {
    fuzz_to_concolic: Mutex<Vec<InputSeed>>,
    concolic_to_fuzz: Mutex<Vec<InputSeed>>,
}

impl SeedExchange {
    pub fn new() -> Self {
        SeedExchange {
            fuzz_to_concolic: Mutex::new(Vec::new()),
            concolic_to_fuzz: Mutex::new(Vec::new()),
        }
    }

    pub fn deposit_for_concolic(&self, seed: InputSeed) {
        self.fuzz_to_concolic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seed);
    }

    pub fn deposit_for_fuzz(&self, seed: InputSeed) {
        self.concolic_to_fuzz
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seed);
    }

    pub fn take_for_concolic(&self) -> Vec<InputSeed> {
        std::mem::take(
            &mut *self
                .fuzz_to_concolic
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    pub fn take_for_fuzz(&self) -> Vec<InputSeed> {
        std::mem::take(
            &mut *self
                .concolic_to_fuzz
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    pub fn pending_for_concolic(&self) -> usize {
        self.fuzz_to_concolic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn pending_for_fuzz(&self) -> usize {
        self.concolic_to_fuzz
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

impl Default for SeedExchange {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;

    fn make_seed(data: &[u8]) -> InputSeed {
        InputSeed::new(data.to_vec(), SeedOrigin::Fuzzer)
    }

    #[test]
    fn new_is_empty() {
        let ex = SeedExchange::new();
        assert_eq!(ex.pending_for_concolic(), 0);
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    #[test]
    fn deposit_and_take_fuzz_to_concolic() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"hello"));
        assert_eq!(ex.pending_for_concolic(), 1);

        let seeds = ex.take_for_concolic();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].data.as_ref(), b"hello");
        assert_eq!(ex.pending_for_concolic(), 0);
    }

    #[test]
    fn deposit_and_take_concolic_to_fuzz() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(make_seed(b"world"));
        assert_eq!(ex.pending_for_fuzz(), 1);

        let seeds = ex.take_for_fuzz();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].data.as_ref(), b"world");
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    #[test]
    fn multiple_deposits_accumulate() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"a"));
        ex.deposit_for_concolic(make_seed(b"b"));
        ex.deposit_for_concolic(make_seed(b"c"));
        assert_eq!(ex.pending_for_concolic(), 3);

        let seeds = ex.take_for_concolic();
        assert_eq!(seeds.len(), 3);
    }

    #[test]
    fn pending_counts() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"x"));
        ex.deposit_for_fuzz(make_seed(b"y"));
        ex.deposit_for_fuzz(make_seed(b"z"));
        assert_eq!(ex.pending_for_concolic(), 1);
        assert_eq!(ex.pending_for_fuzz(), 2);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `take_for_concolic()` on an empty exchange returns an empty vec.
    #[test]
    fn take_for_concolic_when_empty_returns_empty() {
        let ex = SeedExchange::new();
        let seeds = ex.take_for_concolic();
        assert!(seeds.is_empty());
    }

    /// `take_for_fuzz()` on an empty exchange returns an empty vec.
    #[test]
    fn take_for_fuzz_when_empty_returns_empty() {
        let ex = SeedExchange::new();
        let seeds = ex.take_for_fuzz();
        assert!(seeds.is_empty());
    }

    /// After `take_for_concolic`, pending_for_concolic is 0.
    #[test]
    fn take_for_concolic_clears_pending() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"a"));
        ex.deposit_for_concolic(make_seed(b"b"));
        let taken = ex.take_for_concolic();
        assert_eq!(taken.len(), 2);
        assert_eq!(ex.pending_for_concolic(), 0);
    }

    /// After `take_for_fuzz`, pending_for_fuzz is 0.
    #[test]
    fn take_for_fuzz_clears_pending() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(make_seed(b"m"));
        ex.deposit_for_fuzz(make_seed(b"n"));
        ex.deposit_for_fuzz(make_seed(b"o"));
        let taken = ex.take_for_fuzz();
        assert_eq!(taken.len(), 3);
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    /// Two consecutive takes both return empty on the second call.
    #[test]
    fn double_take_second_is_empty() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"once"));
        let first = ex.take_for_concolic();
        assert_eq!(first.len(), 1);
        let second = ex.take_for_concolic();
        assert!(second.is_empty());
    }

    /// `Default` impl creates an empty exchange.
    #[test]
    fn default_creates_empty_exchange() {
        let ex = SeedExchange::default();
        assert_eq!(ex.pending_for_concolic(), 0);
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    /// fuzz→concolic and concolic→fuzz queues are independent.
    #[test]
    fn queues_are_independent() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"c1"));
        ex.deposit_for_concolic(make_seed(b"c2"));
        ex.deposit_for_fuzz(make_seed(b"f1"));

        // Taking concolic does not affect fuzz queue.
        let c = ex.take_for_concolic();
        assert_eq!(c.len(), 2);
        assert_eq!(ex.pending_for_fuzz(), 1);

        // Taking fuzz does not affect concolic queue (already empty).
        let f = ex.take_for_fuzz();
        assert_eq!(f.len(), 1);
        assert_eq!(ex.pending_for_concolic(), 0);
    }

    /// Data content is preserved through deposit/take round-trip.
    #[test]
    fn data_preserved_through_round_trip() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(make_seed(b"hello-world"));
        let seeds = ex.take_for_fuzz();
        assert_eq!(seeds[0].data.as_ref(), b"hello-world");
    }

    /// Many seeds deposited then taken all at once.
    #[test]
    fn many_seeds_round_trip() {
        let ex = SeedExchange::new();
        for i in 0u8..50 {
            ex.deposit_for_concolic(make_seed(&[i]));
        }
        assert_eq!(ex.pending_for_concolic(), 50);
        let all = ex.take_for_concolic();
        assert_eq!(all.len(), 50);
        assert_eq!(ex.pending_for_concolic(), 0);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `SeedExchange::default()` is equivalent to `SeedExchange::new()`.
    #[test]
    fn default_equiv_new() {
        let ex1 = SeedExchange::new();
        let ex2 = SeedExchange::default();
        assert_eq!(ex1.pending_for_concolic(), ex2.pending_for_concolic());
        assert_eq!(ex1.pending_for_fuzz(), ex2.pending_for_fuzz());
    }

    /// `deposit_for_concolic` with an empty seed (zero-byte data).
    #[test]
    fn deposit_for_concolic_empty_seed() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(&[]));
        assert_eq!(ex.pending_for_concolic(), 1);
        let seeds = ex.take_for_concolic();
        assert_eq!(seeds.len(), 1);
        assert!(seeds[0].data.is_empty());
    }

    /// `deposit_for_fuzz` with an empty seed.
    #[test]
    fn deposit_for_fuzz_empty_seed() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(make_seed(&[]));
        assert_eq!(ex.pending_for_fuzz(), 1);
        let seeds = ex.take_for_fuzz();
        assert_eq!(seeds.len(), 1);
        assert!(seeds[0].data.is_empty());
    }

    /// After `take_for_fuzz`, a subsequent `take_for_fuzz` returns empty.
    #[test]
    fn take_fuzz_twice_second_empty() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(make_seed(b"x"));
        ex.take_for_fuzz();
        let second = ex.take_for_fuzz();
        assert!(second.is_empty());
    }

    /// Interleaved deposits and takes maintain correct per-queue counts.
    #[test]
    fn interleaved_deposits_and_takes() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"c1"));
        ex.deposit_for_fuzz(make_seed(b"f1"));
        ex.deposit_for_concolic(make_seed(b"c2"));

        assert_eq!(ex.pending_for_concolic(), 2);
        assert_eq!(ex.pending_for_fuzz(), 1);

        let c = ex.take_for_concolic();
        assert_eq!(c.len(), 2);
        assert_eq!(ex.pending_for_concolic(), 0);
        assert_eq!(ex.pending_for_fuzz(), 1); // fuzz queue unchanged

        let f = ex.take_for_fuzz();
        assert_eq!(f.len(), 1);
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    /// Seed data is preserved correctly through the fuzz queue.
    #[test]
    fn seed_data_preserved_in_fuzz_queue() {
        let ex = SeedExchange::new();
        let data = b"fuzz-data-12345";
        ex.deposit_for_fuzz(make_seed(data));
        let seeds = ex.take_for_fuzz();
        assert_eq!(seeds[0].data.as_ref(), data);
    }
}
