//! GALS (Globally Asynchronous, Locally Synchronous) ensemble synchronization.
//!
//! Provides a thread-safe buffer for exchanging seeds between concurrent
//! solver/fuzzer agents at configurable intervals.

use std::sync::Mutex;

use apex_core::types::InputSeed;

/// Synchronization primitive for exchanging seeds between ensemble agents.
///
/// Agents deposit interesting seeds into the shared buffer. At each sync
/// interval the buffer is drained and seeds are redistributed.
pub struct EnsembleSync {
    buffer: Mutex<Vec<InputSeed>>,
    interval: u64,
    last_sync: Mutex<u64>,
}

impl EnsembleSync {
    /// Create a new ensemble sync with the given sync interval (in iterations).
    pub fn new(interval: u64) -> Self {
        EnsembleSync {
            buffer: Mutex::new(Vec::new()),
            interval,
            last_sync: Mutex::new(0),
        }
    }

    /// Deposit a seed into the shared buffer.
    pub fn deposit(&self, seed: InputSeed) {
        self.buffer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seed);
    }

    /// Check whether a sync should happen at the given iteration.
    ///
    /// Returns `false` if interval is zero (sync disabled).
    pub fn should_sync(&self, iteration: u64) -> bool {
        if self.interval == 0 {
            return false;
        }
        let last = *self.last_sync.lock().unwrap_or_else(|e| e.into_inner());
        iteration >= last + self.interval
    }

    /// Drain the buffer and reset the sync timer. Returns all pending seeds.
    pub fn sync(&self, iteration: u64) -> Vec<InputSeed> {
        let mut last = self.last_sync.lock().unwrap_or_else(|e| e.into_inner());
        *last = iteration;
        let mut buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        buf.drain(..).collect()
    }

    /// Number of seeds waiting in the buffer.
    pub fn pending_count(&self) -> usize {
        self.buffer.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for EnsembleSync {
    fn default() -> Self {
        Self::new(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;

    fn make_seed() -> InputSeed {
        InputSeed::new(vec![0xAA, 0xBB], SeedOrigin::Fuzzer)
    }

    #[test]
    fn new_is_empty() {
        let sync = EnsembleSync::new(10);
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn deposit_increments_count() {
        let sync = EnsembleSync::new(10);
        sync.deposit(make_seed());
        assert_eq!(sync.pending_count(), 1);
        sync.deposit(make_seed());
        assert_eq!(sync.pending_count(), 2);
    }

    #[test]
    fn should_sync_at_interval() {
        let sync = EnsembleSync::new(5);
        // last_sync starts at 0, so need iteration >= 0 + 5
        assert!(!sync.should_sync(0));
        assert!(!sync.should_sync(4));
        assert!(sync.should_sync(5));
        assert!(sync.should_sync(10));
    }

    #[test]
    fn sync_drains_buffer() {
        let sync = EnsembleSync::new(5);
        sync.deposit(make_seed());
        sync.deposit(make_seed());
        let seeds = sync.sync(5);
        assert_eq!(seeds.len(), 2);
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn sync_resets_timer() {
        let sync = EnsembleSync::new(5);
        sync.sync(5);
        // After syncing at 5, next sync should be at 5 + 5 = 10
        assert!(!sync.should_sync(6));
        assert!(!sync.should_sync(9));
        assert!(sync.should_sync(10));
    }

    #[test]
    fn zero_interval_never_syncs() {
        let sync = EnsembleSync::new(0);
        assert!(!sync.should_sync(0));
        assert!(!sync.should_sync(100));
        assert!(!sync.should_sync(u64::MAX));
    }

    #[test]
    fn default_interval_is_20() {
        let sync = EnsembleSync::default();
        assert!(!sync.should_sync(19));
        assert!(sync.should_sync(20));
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `sync()` on an empty buffer returns an empty vec.
    #[test]
    fn sync_empty_buffer_returns_empty() {
        let sync = EnsembleSync::new(5);
        let seeds = sync.sync(0);
        assert!(seeds.is_empty());
    }

    /// `should_sync()` returns true at exact multiples of interval after reset.
    #[test]
    fn should_sync_at_multiples_after_reset() {
        let sync = EnsembleSync::new(10);
        sync.sync(10); // last_sync = 10
        assert!(!sync.should_sync(15)); // 15 < 10+10
        assert!(sync.should_sync(20));  // 20 >= 10+10
        assert!(sync.should_sync(25));  // 25 >= 20 (last_sync still 10)
    }

    /// Two sequential syncs correctly advance the timer each time.
    #[test]
    fn sequential_syncs_advance_timer() {
        let sync = EnsembleSync::new(5);
        sync.deposit(make_seed());
        sync.sync(5);  // last_sync = 5
        sync.deposit(make_seed());
        sync.sync(10); // last_sync = 10
        // After syncing at 10, next sync due at 15.
        assert!(!sync.should_sync(14));
        assert!(sync.should_sync(15));
    }

    /// `pending_count()` decrements to zero after sync.
    #[test]
    fn pending_count_zero_after_sync() {
        let sync = EnsembleSync::new(5);
        sync.deposit(make_seed());
        sync.deposit(make_seed());
        sync.deposit(make_seed());
        assert_eq!(sync.pending_count(), 3);
        sync.sync(5);
        assert_eq!(sync.pending_count(), 0);
    }

    /// Deposit multiple seeds and sync returns them all in order.
    #[test]
    fn sync_returns_all_deposited_seeds() {
        let sync = EnsembleSync::new(1);
        let s1 = InputSeed::new(vec![0x01], SeedOrigin::Fuzzer);
        let s2 = InputSeed::new(vec![0x02], SeedOrigin::Fuzzer);
        let s3 = InputSeed::new(vec![0x03], SeedOrigin::Fuzzer);
        sync.deposit(s1.clone());
        sync.deposit(s2.clone());
        sync.deposit(s3.clone());
        let seeds = sync.sync(1);
        assert_eq!(seeds.len(), 3);
        // Order preserved (Vec drain from front).
        assert_eq!(seeds[0].data.as_ref(), &[0x01_u8][..]);
        assert_eq!(seeds[1].data.as_ref(), &[0x02_u8][..]);
        assert_eq!(seeds[2].data.as_ref(), &[0x03_u8][..]);
    }

    /// `should_sync()` at iteration 0 with interval 0 returns false (zero-interval path).
    #[test]
    fn zero_interval_returns_false_regardless_of_iteration() {
        let sync = EnsembleSync::new(0);
        // All of these hit the `if self.interval == 0 { return false; }` branch.
        for iter in [0u64, 1, 10, 1000, u64::MAX / 2] {
            assert!(!sync.should_sync(iter));
        }
    }

    /// `should_sync()` when `iteration == last_sync + interval` (exact boundary).
    #[test]
    fn should_sync_exact_boundary() {
        let sync = EnsembleSync::new(7);
        // last_sync=0, interval=7 → should_sync(7) must be true (>=)
        assert!(sync.should_sync(7));
    }

    /// `should_sync()` one below exact boundary returns false.
    #[test]
    fn should_sync_one_below_boundary() {
        let sync = EnsembleSync::new(7);
        assert!(!sync.should_sync(6));
    }

    /// interval=1 means sync is due at every iteration.
    #[test]
    fn interval_one_syncs_every_iteration() {
        let sync = EnsembleSync::new(1);
        assert!(sync.should_sync(1));
        assert!(sync.should_sync(2));
        assert!(sync.should_sync(100));
    }

    /// Large interval — should_sync returns false until threshold.
    #[test]
    fn large_interval_sync() {
        let sync = EnsembleSync::new(u64::MAX / 2);
        assert!(!sync.should_sync(100));
        assert!(sync.should_sync(u64::MAX / 2));
    }
}
