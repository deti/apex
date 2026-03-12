//! Pure Rust SanCov runtime callbacks.
//!
//! When a Rust binary is compiled with `-C passes=sancov-module
//! -C llvm-args=-sanitizer-coverage-trace-pc-guard`, the compiler inserts
//! calls to `__sanitizer_cov_trace_pc_guard` at each edge. These callbacks
//! record edge coverage into a shared bitmap.

use std::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};

/// Maximum number of edges tracked. Matches the SHM bitmap size.
pub const MAX_EDGES: usize = 65536;

/// Edge hit counters. Each edge gets one byte (saturating).
#[allow(clippy::declare_interior_mutable_const)]
static COUNTERS: [AtomicU8; MAX_EDGES] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; MAX_EDGES]
};

/// Total number of guards (edges) registered.
static NUM_GUARDS: AtomicU32 = AtomicU32::new(0);

/// Called by the instrumented binary once at startup for each module.
///
/// # Safety
/// Called by compiler-inserted code. `start` and `stop` point to a
/// contiguous array of u32 guard slots.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_pc_guard_init(start: *mut u32, stop: *mut u32) {
    if start == stop || start.is_null() {
        return;
    }
    let count = (stop as usize - start as usize) / std::mem::size_of::<u32>();
    let base = NUM_GUARDS.fetch_add(count as u32, Ordering::SeqCst);
    for i in 0..count {
        let guard = start.add(i);
        let id = base + i as u32;
        if (id as usize) < MAX_EDGES {
            *guard = id;
        }
    }
}

/// Called at each instrumented edge. Increments the counter for this guard.
///
/// # Safety
/// Called by compiler-inserted code. `guard` points to a valid u32.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_pc_guard(guard: *mut u32) {
    let idx = *guard as usize;
    if idx < MAX_EDGES {
        let _ = COUNTERS[idx].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            if v < 255 {
                Some(v + 1)
            } else {
                None
            }
        });
    }
}

/// Read the current coverage bitmap. Returns a copy of all counters.
pub fn read_bitmap() -> [u8; MAX_EDGES] {
    let mut bitmap = [0u8; MAX_EDGES];
    for (i, counter) in COUNTERS.iter().enumerate() {
        bitmap[i] = counter.load(Ordering::Relaxed);
    }
    bitmap
}

/// Reset all counters to zero (between executions).
pub fn reset_bitmap() {
    for counter in COUNTERS.iter() {
        counter.store(0, Ordering::Relaxed);
    }
}

/// Number of registered guards (edges).
pub fn num_guards() -> u32 {
    NUM_GUARDS.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// CMP comparison log (RedQueen / input-to-state)
// ---------------------------------------------------------------------------

/// Maximum CMP entries per execution (ring buffer).
pub const MAX_CMP_ENTRIES: usize = 4096;

/// A single comparison observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmpLogEntry {
    pub arg1: Vec<u8>,
    pub arg2: Vec<u8>,
    pub size: u8,
}

/// Raw storage for CMP log entries. Each entry is stored as:
/// [size(1B)][arg1(8B little-endian padded)][arg2(8B little-endian padded)] = 17 bytes
const CMP_ENTRY_SIZE: usize = 17;
#[allow(clippy::declare_interior_mutable_const)]
static CMP_BUFFER: [AtomicU8; MAX_CMP_ENTRIES * CMP_ENTRY_SIZE] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; MAX_CMP_ENTRIES * CMP_ENTRY_SIZE]
};
static CMP_COUNT: AtomicUsize = AtomicUsize::new(0);

fn record_cmp(arg1: u64, arg2: u64, size: u8) {
    let idx = CMP_COUNT.fetch_add(1, Ordering::Relaxed) % MAX_CMP_ENTRIES;
    let base = idx * CMP_ENTRY_SIZE;
    CMP_BUFFER[base].store(size, Ordering::Relaxed);
    let a1 = arg1.to_le_bytes();
    let a2 = arg2.to_le_bytes();
    for i in 0..8 {
        CMP_BUFFER[base + 1 + i].store(a1[i], Ordering::Relaxed);
        CMP_BUFFER[base + 9 + i].store(a2[i], Ordering::Relaxed);
    }
}

/// # Safety
/// Called by compiler-inserted comparison instrumentation.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_cmp1(arg1: u8, arg2: u8) {
    record_cmp(arg1 as u64, arg2 as u64, 1);
}

/// # Safety
/// Called by compiler-inserted comparison instrumentation.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_cmp2(arg1: u16, arg2: u16) {
    record_cmp(arg1 as u64, arg2 as u64, 2);
}

/// # Safety
/// Called by compiler-inserted comparison instrumentation.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_cmp4(arg1: u32, arg2: u32) {
    record_cmp(arg1 as u64, arg2 as u64, 4);
}

/// # Safety
/// Called by compiler-inserted comparison instrumentation.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_cmp8(arg1: u64, arg2: u64) {
    record_cmp(arg1, arg2, 8);
}

/// Read the current CMP log. Returns entries recorded since last reset.
pub fn read_cmp_log() -> Vec<CmpLogEntry> {
    let count = CMP_COUNT.load(Ordering::Relaxed).min(MAX_CMP_ENTRIES);
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * CMP_ENTRY_SIZE;
        let size = CMP_BUFFER[base].load(Ordering::Relaxed);
        if size == 0 {
            continue;
        }
        let mut a1 = [0u8; 8];
        let mut a2 = [0u8; 8];
        for j in 0..8 {
            a1[j] = CMP_BUFFER[base + 1 + j].load(Ordering::Relaxed);
            a2[j] = CMP_BUFFER[base + 9 + j].load(Ordering::Relaxed);
        }
        entries.push(CmpLogEntry {
            arg1: a1[..size as usize].to_vec(),
            arg2: a2[..size as usize].to_vec(),
            size,
        });
    }
    entries
}

/// Reset the CMP log (between executions).
pub fn reset_cmp_log() {
    CMP_COUNT.store(0, Ordering::Relaxed);
    for cell in CMP_BUFFER.iter() {
        cell.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// Reset all global state so tests don't interfere regardless of order.
    fn reset_all() {
        reset_bitmap();
        reset_cmp_log();
        NUM_GUARDS.store(0, Ordering::SeqCst);
    }

    #[test]
    #[serial]
    fn initial_bitmap_is_zero() {
        reset_all();
        let bitmap = read_bitmap();
        assert!(bitmap.iter().all(|&b| b == 0));
    }

    #[test]
    #[serial]
    fn reset_clears_bitmap() {
        reset_all();
        COUNTERS[0].store(42, Ordering::Relaxed);
        reset_bitmap();
        assert_eq!(COUNTERS[0].load(Ordering::Relaxed), 0);
    }

    #[test]
    #[serial]
    fn trace_pc_guard_increments() {
        reset_all();
        let mut guard: u32 = 5;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        assert_eq!(COUNTERS[5].load(Ordering::Relaxed), 2);
    }

    #[test]
    #[serial]
    fn counter_saturates_at_255() {
        reset_all();
        let mut guard: u32 = 10;
        COUNTERS[10].store(254, Ordering::Relaxed);
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        assert_eq!(COUNTERS[10].load(Ordering::Relaxed), 255);
    }

    #[test]
    #[serial]
    fn out_of_bounds_guard_ignored() {
        reset_all();
        let mut guard: u32 = MAX_EDGES as u32 + 100;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
    }

    #[test]
    #[serial]
    fn read_bitmap_reflects_counters() {
        reset_all();
        COUNTERS[42].store(7, Ordering::Relaxed);
        let bitmap = read_bitmap();
        assert_eq!(bitmap[42], 7);
    }

    // CMP log tests

    #[test]
    #[serial]
    fn cmp_log_initially_empty() {
        reset_all();
        let log = read_cmp_log();
        assert!(log.is_empty());
    }

    #[test]
    #[serial]
    fn trace_cmp4_records_entry() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp4(0x41414141, 0x42424242);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 4);
        assert_eq!(log[0].arg1, 0x41414141u32.to_le_bytes());
        assert_eq!(log[0].arg2, 0x42424242u32.to_le_bytes());
    }

    #[test]
    #[serial]
    fn trace_cmp1_records_single_byte() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp1(0xAA, 0xBB);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 1);
        assert_eq!(log[0].arg1, vec![0xAA]);
        assert_eq!(log[0].arg2, vec![0xBB]);
    }

    #[test]
    #[serial]
    fn trace_cmp8_records_eight_bytes() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp8(0x0102030405060708, 0x1112131415161718);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 8);
    }

    #[test]
    #[serial]
    fn cmp_log_ring_buffer_wraps() {
        reset_all();
        for i in 0..MAX_CMP_ENTRIES + 10 {
            unsafe {
                __sanitizer_cov_trace_cmp4(i as u32, 0);
            }
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), MAX_CMP_ENTRIES);
    }

    #[test]
    #[serial]
    fn reset_cmp_log_clears() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp4(1, 2);
        }
        assert!(!read_cmp_log().is_empty());
        reset_cmp_log();
        assert!(read_cmp_log().is_empty());
    }

    // guard_init tests

    #[test]
    #[serial]
    fn guard_init_with_null_start() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(
                std::ptr::null_mut(),
                std::ptr::null_mut::<u32>().add(4),
            );
        }
    }

    #[test]
    #[serial]
    fn guard_init_with_equal_pointers() {
        reset_all();
        let mut slot: u32 = 0xDEAD;
        let ptr = &mut slot as *mut u32;
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(ptr, ptr);
        }
        assert_eq!(slot, 0xDEAD);
    }

    #[test]
    #[serial]
    fn guard_init_assigns_ids() {
        reset_all();
        let mut guards = [0u32; 4];
        let start = guards.as_mut_ptr();
        let stop = unsafe { start.add(guards.len()) };
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(start, stop);
        }
        // NUM_GUARDS was reset to 0, so IDs start at 0.
        for (i, &g) in guards.iter().enumerate() {
            assert_eq!(g, i as u32);
        }
    }

    #[test]
    #[serial]
    fn guard_init_and_trace_integration() {
        reset_all();
        let mut guards = [0u32; 2];
        let start = guards.as_mut_ptr();
        let stop = unsafe { start.add(guards.len()) };
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(start, stop);
        }
        let id = guards[0] as usize;
        assert!(id < MAX_EDGES, "guard id must be within bitmap bounds");
        let before = COUNTERS[id].load(Ordering::Relaxed);
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guards[0] as *mut u32);
        }
        assert_eq!(COUNTERS[id].load(Ordering::Relaxed), before + 1);
    }

    #[test]
    #[serial]
    fn trace_cmp2_records_entry() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp2(0x1234, 0x5678);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 2);
        assert_eq!(log[0].arg1, 0x1234u16.to_le_bytes().to_vec());
        assert_eq!(log[0].arg2, 0x5678u16.to_le_bytes().to_vec());
    }

    // --- num_guards ---

    #[test]
    #[serial]
    fn num_guards_returns_current_count() {
        reset_all();
        assert_eq!(num_guards(), 0);
    }

    #[test]
    #[serial]
    fn guard_init_increments_num_guards() {
        reset_all();
        let mut guards = [0u32; 3];
        let start = guards.as_mut_ptr();
        let stop = unsafe { start.add(guards.len()) };
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(start, stop);
        }
        assert_eq!(NUM_GUARDS.load(Ordering::SeqCst), 3);
    }

    // --- guard_init clamps IDs that exceed MAX_EDGES ---

    #[test]
    #[serial]
    fn guard_init_does_not_write_when_id_exceeds_max_edges() {
        reset_all();
        // Force NUM_GUARDS to a value just below MAX_EDGES so the last few
        // guards in the init call will overflow the bitmap.
        let overflow_base = (MAX_EDGES - 1) as u32;
        NUM_GUARDS.store(overflow_base, Ordering::SeqCst);

        let mut guards = [0xDEAD_BEEFu32; 3];
        let start = guards.as_mut_ptr();
        let stop = unsafe { start.add(guards.len()) };
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(start, stop);
        }
        // guards[0] is at id == overflow_base (MAX_EDGES-1) — valid, must be written.
        assert_eq!(guards[0], overflow_base);
        // guards[1] and guards[2] have id >= MAX_EDGES — must NOT be written.
        assert_eq!(guards[1], 0xDEAD_BEEF);
        assert_eq!(guards[2], 0xDEAD_BEEF);
    }

    // --- trace_pc_guard boundary: guard == 0 ---

    #[test]
    #[serial]
    fn trace_pc_guard_at_index_zero() {
        reset_all();
        let mut guard: u32 = 0;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        assert_eq!(COUNTERS[0].load(Ordering::Relaxed), 1);
    }

    // --- trace_pc_guard at the last valid index ---

    #[test]
    #[serial]
    fn trace_pc_guard_at_max_edges_minus_one() {
        reset_all();
        let mut guard: u32 = (MAX_EDGES - 1) as u32;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        assert_eq!(COUNTERS[MAX_EDGES - 1].load(Ordering::Relaxed), 1);
    }

    // --- trace_pc_guard exactly at MAX_EDGES (boundary, must be ignored) ---

    #[test]
    #[serial]
    fn trace_pc_guard_at_max_edges_is_ignored() {
        reset_all();
        let mut guard: u32 = MAX_EDGES as u32;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        let bitmap = read_bitmap();
        assert!(bitmap.iter().all(|&b| b == 0));
    }

    // --- reset_bitmap zeroes ALL counters ---

    #[test]
    #[serial]
    fn reset_bitmap_zeroes_multiple_counters() {
        reset_all();
        COUNTERS[0].store(1, Ordering::Relaxed);
        COUNTERS[100].store(100, Ordering::Relaxed);
        COUNTERS[MAX_EDGES - 1].store(255, Ordering::Relaxed);
        reset_bitmap();
        assert_eq!(COUNTERS[0].load(Ordering::Relaxed), 0);
        assert_eq!(COUNTERS[100].load(Ordering::Relaxed), 0);
        assert_eq!(COUNTERS[MAX_EDGES - 1].load(Ordering::Relaxed), 0);
    }

    // --- read_cmp_log skips entries with size == 0 ---

    #[test]
    #[serial]
    fn read_cmp_log_skips_zero_size_entries() {
        reset_all();

        // Manually write a size-0 entry at slot 0 and a real entry at slot 1.
        CMP_BUFFER[0].store(0, Ordering::Relaxed); // size = 0  → must be skipped
        let base1 = CMP_ENTRY_SIZE;
        CMP_BUFFER[base1].store(2, Ordering::Relaxed);
        for i in 0..8 {
            CMP_BUFFER[base1 + 1 + i].store(i as u8, Ordering::Relaxed);
            CMP_BUFFER[base1 + 9 + i].store((i + 10) as u8, Ordering::Relaxed);
        }
        CMP_COUNT.store(2, Ordering::Relaxed);

        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 2);
    }

    // --- CMP log: multiple entries, count capped at MAX_CMP_ENTRIES ---

    #[test]
    #[serial]
    fn read_cmp_log_count_capped_at_max() {
        reset_all();
        for i in 0..(MAX_CMP_ENTRIES + 5) {
            unsafe {
                __sanitizer_cov_trace_cmp4(i as u32, i as u32 + 1);
            }
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), MAX_CMP_ENTRIES);
    }

    // --- CmpLogEntry derives (Clone, Debug, PartialEq, Eq) ---

    #[test]
    #[serial]
    fn cmp_log_entry_clone_and_eq() {
        let entry = CmpLogEntry {
            arg1: vec![1, 2],
            arg2: vec![3, 4],
            size: 2,
        };
        let cloned = entry.clone();
        assert_eq!(entry, cloned);
        let _ = format!("{:?}", entry);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// Counter already at 255 — `fetch_update` callback returns `None` (saturated).
    /// The counter should remain at 255.
    #[test]
    #[serial]
    fn counter_already_saturated_stays_at_255() {
        reset_all();
        let mut guard: u32 = 3;
        COUNTERS[3].store(255, Ordering::Relaxed);
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        assert_eq!(COUNTERS[3].load(Ordering::Relaxed), 255);
    }

    /// `read_bitmap()` reads a zero-length slice correctly (all counters are reset).
    #[test]
    #[serial]
    fn read_bitmap_all_zeros_after_reset() {
        reset_all();
        let bitmap = read_bitmap();
        assert!(bitmap.iter().all(|&b| b == 0));
        assert_eq!(bitmap.len(), MAX_EDGES);
    }

    /// `reset_bitmap()` called when counters are already zero does nothing harmful.
    #[test]
    #[serial]
    fn reset_bitmap_idempotent() {
        reset_all();
        reset_bitmap();
        reset_bitmap();
        let bitmap = read_bitmap();
        assert!(bitmap.iter().all(|&b| b == 0));
    }

    /// `num_guards()` after `reset_all()` returns 0.
    #[test]
    #[serial]
    fn num_guards_zero_after_reset() {
        reset_all();
        assert_eq!(num_guards(), 0);
    }

    /// `reset_cmp_log()` called when already empty stays empty.
    #[test]
    #[serial]
    fn reset_cmp_log_idempotent() {
        reset_all();
        reset_cmp_log();
        reset_cmp_log();
        assert!(read_cmp_log().is_empty());
    }

    /// CMP log with exactly MAX_CMP_ENTRIES entries — `count.min(MAX_CMP_ENTRIES)` returns
    /// `MAX_CMP_ENTRIES` when CMP_COUNT == MAX_CMP_ENTRIES.
    #[test]
    #[serial]
    fn read_cmp_log_exactly_max_entries() {
        reset_all();
        for i in 0..MAX_CMP_ENTRIES {
            unsafe {
                __sanitizer_cov_trace_cmp4(i as u32, i as u32 + 1);
            }
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), MAX_CMP_ENTRIES);
    }

    /// `trace_cmp8` produces size=8 entries with correct byte widths.
    #[test]
    #[serial]
    fn trace_cmp8_size_and_byte_widths() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp8(0xABCD_EF01_2345_6789, 0x1122_3344_5566_7788);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 8);
        assert_eq!(log[0].arg1.len(), 8);
        assert_eq!(log[0].arg2.len(), 8);
    }

    /// `trace_cmp2` produces size=2 entries.
    #[test]
    #[serial]
    fn trace_cmp2_size_two() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp2(0xABCD, 0x1234);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 2);
        assert_eq!(log[0].arg1.len(), 2);
        assert_eq!(log[0].arg2.len(), 2);
    }

    /// `trace_cmp1` produces size=1 entries.
    #[test]
    #[serial]
    fn trace_cmp1_size_one() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp1(0xAB, 0xCD);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].size, 1);
        assert_eq!(log[0].arg1.len(), 1);
        assert_eq!(log[0].arg2.len(), 1);
    }

    /// Multiple cmp calls of different sizes interleaved.
    #[test]
    #[serial]
    fn mixed_cmp_sizes_all_recorded() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp1(1, 2);
            __sanitizer_cov_trace_cmp2(10, 20);
            __sanitizer_cov_trace_cmp4(100, 200);
            __sanitizer_cov_trace_cmp8(1000, 2000);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 4);
        assert_eq!(log[0].size, 1);
        assert_eq!(log[1].size, 2);
        assert_eq!(log[2].size, 4);
        assert_eq!(log[3].size, 8);
    }

    /// After calling `reset_cmp_log()`, CMP_COUNT is reset so that subsequent
    /// reads correctly reflect new entries from index 0.
    #[test]
    #[serial]
    fn reset_cmp_log_and_re_record() {
        reset_all();
        unsafe {
            __sanitizer_cov_trace_cmp4(1, 2);
            __sanitizer_cov_trace_cmp4(3, 4);
        }
        assert_eq!(read_cmp_log().len(), 2);
        reset_cmp_log();
        assert!(read_cmp_log().is_empty());
        // Record again after reset.
        unsafe {
            __sanitizer_cov_trace_cmp4(5, 6);
        }
        let log = read_cmp_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].arg1, 5u32.to_le_bytes().to_vec());
    }

    /// `guard_init` assigns guards starting at the current `NUM_GUARDS` offset.
    #[test]
    #[serial]
    fn guard_init_accumulates_across_calls() {
        reset_all();
        let mut g1 = [0u32; 2];
        let mut g2 = [0u32; 3];
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(g1.as_mut_ptr(), g1.as_mut_ptr().add(2));
        }
        // NUM_GUARDS should be 2 now.
        assert_eq!(NUM_GUARDS.load(Ordering::SeqCst), 2);
        unsafe {
            __sanitizer_cov_trace_pc_guard_init(g2.as_mut_ptr(), g2.as_mut_ptr().add(3));
        }
        // After second call: NUM_GUARDS = 5.
        assert_eq!(NUM_GUARDS.load(Ordering::SeqCst), 5);
        // g2 guards should have IDs starting from 2.
        assert_eq!(g2[0], 2);
        assert_eq!(g2[1], 3);
        assert_eq!(g2[2], 4);
    }

    /// `MAX_EDGES` constant is correct.
    #[test]
    fn max_edges_constant() {
        assert_eq!(MAX_EDGES, 65536);
    }

    /// `MAX_CMP_ENTRIES` constant is correct.
    #[test]
    fn max_cmp_entries_constant() {
        assert_eq!(MAX_CMP_ENTRIES, 4096);
    }

    /// `CmpLogEntry` inequality — two entries with different args are not equal.
    #[test]
    fn cmp_log_entry_inequality() {
        let a = CmpLogEntry {
            arg1: vec![1],
            arg2: vec![2],
            size: 1,
        };
        let b = CmpLogEntry {
            arg1: vec![3],
            arg2: vec![4],
            size: 1,
        };
        assert_ne!(a, b);
    }
}
