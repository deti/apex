//! RedQueen / CmpLog — input-to-state comparison feedback.
//!
//! Two sources of comparison data:
//! 1. SanCov CMP callbacks (via `apex_sandbox::sancov_rt::read_cmp_log()`)
//! 2. Output parsing fallback (`parse_cmp_hints_from_output()`)
//!
//! ## Data structures
//!
//! - [`CmpEntry`] / [`CmpLog`] — flat, deduplicated comparison log (original).
//! - [`CmpOp`] / [`CmpLogEntry`] / [`CmpLogTable`] — branch-keyed ring-buffer
//!   log with rich comparison metadata, used by [`RedQueenMutator`].

use crate::traits::Mutator;
use apex_core::types::BranchId;
use rand::RngCore;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock};

static RE_EXPECTED: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"expected\s+(\S+?)[\s,]+(?:but\s+)?got\s+(\S+)").unwrap());

static RE_LEFT_RIGHT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"left=`([^`]+)`.*right=`([^`]+)`").unwrap());

/// A single comparison observation: two byte sequences being compared.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CmpEntry {
    pub arg1: Vec<u8>,
    pub arg2: Vec<u8>,
}

impl CmpEntry {
    pub fn new(arg1: Vec<u8>, arg2: Vec<u8>) -> Self {
        Self { arg1, arg2 }
    }
}

/// Deduplicated collection of comparison observations from one execution.
pub struct CmpLog {
    seen: std::collections::HashSet<CmpEntry>,
    log: Vec<CmpEntry>,
}

impl CmpLog {
    pub fn new() -> Self {
        Self {
            seen: std::collections::HashSet::new(),
            log: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: CmpEntry) {
        if self.seen.insert(entry.clone()) {
            self.log.push(entry);
        }
    }

    pub fn entries(&self) -> &[CmpEntry] {
        &self.log
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
}

impl Default for CmpLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse comparison hints from test output (stderr/stdout).
///
/// Recognizes patterns like:
/// - `expected X but got Y` / `expected X, got Y`
/// - `left=`X`, right=`Y`` (Rust assert_eq)
/// - `AssertionError: X != Y`
pub fn parse_cmp_hints_from_output(output: &str) -> Vec<CmpEntry> {
    let mut hints = Vec::new();

    for re in [&*RE_EXPECTED, &*RE_LEFT_RIGHT] {
        for caps in re.captures_iter(output) {
            if let (Some(a), Some(b)) = (caps.get(1), caps.get(2)) {
                hints.push(CmpEntry::new(
                    a.as_str().as_bytes().to_vec(),
                    b.as_str().as_bytes().to_vec(),
                ));
            }
        }
    }

    hints
}

/// Mutator that performs input-to-state replacement using CMP log data.
///
/// For each CMP entry, scans the input for `arg1` and replaces with `arg2`
/// (or vice versa). Picks a random entry and random direction per invocation.
pub struct CmpLogMutator {
    log: CmpLog,
}

impl CmpLogMutator {
    pub fn new(log: CmpLog) -> Self {
        Self { log }
    }
}

impl Mutator for CmpLogMutator {
    fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
        if self.log.is_empty() {
            return input.to_vec();
        }

        let entries = self.log.entries();
        let idx = (rng.next_u32() as usize) % entries.len();
        let entry = &entries[idx];

        // Randomly pick direction: replace arg1->arg2 or arg2->arg1
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let (needle, replacement) = if rng.next_u32() % 2 == 0 {
            (&entry.arg1, &entry.arg2)
        } else {
            (&entry.arg2, &entry.arg1)
        };

        if needle.is_empty() || needle.len() > input.len() {
            return input.to_vec();
        }

        // Find all positions where needle occurs
        let mut positions = Vec::new();
        for i in 0..=input.len() - needle.len() {
            if &input[i..i + needle.len()] == needle.as_slice() {
                positions.push(i);
            }
        }

        if positions.is_empty() {
            return input.to_vec();
        }

        // Replace at a random matching position, handling length differences.
        let pos = positions[(rng.next_u32() as usize) % positions.len()];
        let mut out = input.to_vec();
        if needle.len() == replacement.len() {
            out[pos..pos + needle.len()].copy_from_slice(replacement);
        } else {
            out.splice(pos..pos + needle.len(), replacement.iter().copied());
        }
        out
    }

    fn name(&self) -> &str {
        "cmplog"
    }
}

// ---------------------------------------------------------------------------
// Rich CmpLog with branch-keyed ring buffers (RedQueen / I2S)
// ---------------------------------------------------------------------------

/// Comparison operator observed at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    StartsWith,
    EndsWith,
}

/// A single comparison observation tied to a specific branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmpLogEntry {
    pub op: CmpOp,
    /// Left operand (often input-derived).
    pub operand_a: Vec<u8>,
    /// Right operand (often a constant / expected value).
    pub operand_b: Vec<u8>,
    /// The branch where this comparison was observed.
    pub branch_id: BranchId,
}

/// Maximum entries stored per branch in the ring buffer.
const CMPLOG_RING_MAX: usize = 256;

/// Per-branch ring-buffer collection of comparison observations.
///
/// Maintains at most [`CMPLOG_RING_MAX`] entries per branch. When the limit
/// is exceeded the oldest entry for that branch is evicted.
pub struct CmpLogTable {
    entries: HashMap<BranchId, VecDeque<CmpLogEntry>>,
}

impl CmpLogTable {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Record a comparison observation, evicting the oldest entry for that
    /// branch if the ring buffer is full.
    pub fn record(&mut self, entry: CmpLogEntry) {
        let ring = self.entries.entry(entry.branch_id.clone()).or_default();
        if ring.len() >= CMPLOG_RING_MAX {
            ring.pop_front();
        }
        ring.push_back(entry);
    }

    /// Return all entries for a given branch (empty vec if none).
    pub fn entries_for(&self, branch: &BranchId) -> Vec<&CmpLogEntry> {
        match self.entries.get(branch) {
            Some(ring) => ring.iter().collect(),
            None => Vec::new(),
        }
    }

    /// Iterate over every recorded entry across all branches.
    pub fn all_entries(&self) -> impl Iterator<Item = &CmpLogEntry> {
        self.entries.values().flat_map(|ring| ring.iter())
    }

    /// Total number of entries across all branches.
    pub fn len(&self) -> usize {
        self.entries.values().map(|r| r.len()).sum()
    }

    /// Whether the table contains any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.values().all(|r| r.is_empty())
    }

    /// Remove all entries from every branch.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for CmpLogTable {
    fn default() -> Self {
        Self::new()
    }
}

/// RedQueen-style mutator: uses [`CmpLogTable`] to perform input-to-state
/// value injection.
///
/// For each mutation attempt it picks a random [`CmpLogEntry`], searches for
/// `operand_a` in the input, and replaces it with `operand_b`. When the
/// operands differ in length the input is resized accordingly (splice
/// semantics).
pub struct RedQueenMutator {
    cmplog: Arc<CmpLogTable>,
}

impl RedQueenMutator {
    pub fn new(cmplog: Arc<CmpLogTable>) -> Self {
        Self { cmplog }
    }
}

impl Mutator for RedQueenMutator {
    fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
        // Collect all entries (cheaply borrow).
        let all: Vec<&CmpLogEntry> = self.cmplog.all_entries().collect();
        if all.is_empty() {
            return input.to_vec();
        }

        let entry = all[(rng.next_u32() as usize) % all.len()];

        // Randomly pick direction: a→b or b→a.
        #[allow(clippy::manual_is_multiple_of)]
        let (needle, replacement) = if rng.next_u32() % 2 == 0 {
            (&entry.operand_a, &entry.operand_b)
        } else {
            (&entry.operand_b, &entry.operand_a)
        };

        if needle.is_empty() || needle.len() > input.len() {
            return input.to_vec();
        }

        // Find all positions where needle occurs.
        let mut positions = Vec::new();
        for i in 0..=input.len() - needle.len() {
            if &input[i..i + needle.len()] == needle.as_slice() {
                positions.push(i);
            }
        }

        if positions.is_empty() {
            return input.to_vec();
        }

        // Replace at a random matching position, handling length differences.
        let pos = positions[(rng.next_u32() as usize) % positions.len()];
        let mut out = Vec::with_capacity(input.len() - needle.len() + replacement.len());
        out.extend_from_slice(&input[..pos]);
        out.extend_from_slice(replacement);
        out.extend_from_slice(&input[pos + needle.len()..]);
        out
    }

    fn name(&self) -> &str {
        "redqueen"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_entry_new() {
        let e = CmpEntry::new(vec![1, 2, 3, 4], vec![5, 6, 7, 8]);
        assert_eq!(e.arg1, vec![1, 2, 3, 4]);
        assert_eq!(e.arg2, vec![5, 6, 7, 8]);
    }

    #[test]
    fn cmp_log_add_and_entries() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(vec![0xAA], vec![0xBB]));
        log.add(CmpEntry::new(vec![0xCC], vec![0xDD]));
        assert_eq!(log.entries().len(), 2);
    }

    #[test]
    fn cmp_log_dedup() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(vec![1], vec![2]));
        log.add(CmpEntry::new(vec![1], vec![2])); // duplicate
        log.add(CmpEntry::new(vec![3], vec![4]));
        assert_eq!(log.entries().len(), 2);
    }

    #[test]
    fn parse_hints_assertion_expected_got() {
        let stderr = "AssertionError: expected 42 but got 0";
        let hints = parse_cmp_hints_from_output(stderr);
        assert!(!hints.is_empty());
        // Should find the numeric pair (42, 0)
        assert!(hints.iter().any(|e| e.arg1 == b"42" && e.arg2 == b"0"));
    }

    #[test]
    fn parse_hints_not_equal() {
        let stderr = "assert_eq failed: left=`hello`, right=`world`";
        let hints = parse_cmp_hints_from_output(stderr);
        assert!(hints.iter().any(|e| {
            std::str::from_utf8(&e.arg1).ok() == Some("hello")
                && std::str::from_utf8(&e.arg2).ok() == Some("world")
        }));
    }

    #[test]
    fn parse_hints_empty_string() {
        let hints = parse_cmp_hints_from_output("");
        assert!(hints.is_empty());
    }

    #[test]
    fn parse_hints_no_comparisons() {
        let hints = parse_cmp_hints_from_output("some random output with no comparisons");
        assert!(hints.is_empty());
    }

    // CmpLogMutator tests

    #[test]
    fn cmplog_mutator_replaces_arg1_with_arg2() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"AAAA".to_vec(), b"BBBB".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"xxAAAAyy";
        let mut rng = rand::rng();
        // Run multiple times -- should eventually produce the replacement
        let mut found = false;
        for _ in 0..50 {
            let out = m.mutate(input, &mut rng);
            if out == b"xxBBBByy" {
                found = true;
                break;
            }
        }
        assert!(found, "CmpLogMutator should replace AAAA with BBBB");
    }

    #[test]
    fn cmplog_mutator_replaces_arg2_with_arg1() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"XX".to_vec(), b"YY".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"__YY__";
        let mut rng = rand::rng();
        let mut found = false;
        for _ in 0..50 {
            let out = m.mutate(input, &mut rng);
            if out == b"__XX__" {
                found = true;
                break;
            }
        }
        assert!(found, "CmpLogMutator should also try reverse replacement");
    }

    #[test]
    fn cmplog_mutator_no_match_returns_original() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"ZZZZ".to_vec(), b"WWWW".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"no match here";
        let mut rng = rand::rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_empty_log_returns_original() {
        let m = CmpLogMutator::new(CmpLog::new());
        let input = b"test";
        let mut rng = rand::rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_name() {
        let m = CmpLogMutator::new(CmpLog::new());
        assert_eq!(m.name(), "cmplog");
    }

    #[test]
    fn cmp_log_default() {
        let log = CmpLog::default();
        assert!(log.is_empty());
        assert_eq!(log.entries().len(), 0);
    }

    #[test]
    fn cmp_log_is_empty_after_add() {
        let mut log = CmpLog::new();
        assert!(log.is_empty());
        log.add(CmpEntry::new(vec![1], vec![2]));
        assert!(!log.is_empty());
    }

    #[test]
    fn cmplog_mutator_needle_longer_than_input() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"LONGNEEDLE".to_vec(), b"SHORT".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"tiny";
        let mut rng = rand::rng();
        // needle.len() > input.len(), should return input unchanged
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_empty_needle_returns_original() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(vec![], b"replacement".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"test data";
        let mut rng = rand::rng();
        // empty needle should return input unchanged
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_multiple_match_positions() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"AA".to_vec(), b"BB".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"AAXAA"; // "AA" at positions 0 and 3
        let mut rng = rand::rng();
        let mut saw_first = false;
        let mut saw_second = false;
        for _ in 0..100 {
            let out = m.mutate(input, &mut rng);
            if out == b"BBXAA" || out == b"BBXBB" {
                saw_first = true;
            }
            if out == b"AAXBB" || out == b"BBXBB" {
                saw_second = true;
            }
            if saw_first && saw_second {
                break;
            }
        }
        // At least one position should be replaced
        assert!(saw_first || saw_second);
    }

    #[test]
    fn parse_hints_expected_comma_got() {
        let output = "expected 100, got 200";
        let hints = parse_cmp_hints_from_output(output);
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|e| e.arg1 == b"100" && e.arg2 == b"200"));
    }

    #[test]
    fn parse_hints_multiple_matches() {
        let output = "expected 1 but got 2\nexpected 3 but got 4";
        let hints = parse_cmp_hints_from_output(output);
        assert!(hints.len() >= 2);
    }

    #[test]
    fn cmp_entry_clone_and_eq() {
        let e1 = CmpEntry::new(vec![1, 2], vec![3, 4]);
        let e2 = e1.clone();
        assert_eq!(e1, e2);
    }

    #[test]
    fn cmp_entry_hash_consistent() {
        use std::collections::HashSet;
        let e1 = CmpEntry::new(vec![10], vec![20]);
        let e2 = CmpEntry::new(vec![10], vec![20]);
        let mut set = HashSet::new();
        set.insert(e1);
        set.insert(e2);
        assert_eq!(set.len(), 1); // duplicates deduplicated
    }

    #[test]
    fn cmp_entry_debug() {
        let e = CmpEntry::new(vec![0xAA], vec![0xBB]);
        let debug = format!("{e:?}");
        assert!(debug.contains("CmpEntry"));
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn cmp_log_add_same_entry_three_times_only_stored_once() {
        let mut log = CmpLog::new();
        let entry = CmpEntry::new(vec![1, 2, 3], vec![4, 5, 6]);
        log.add(entry.clone());
        log.add(entry.clone());
        log.add(entry);
        assert_eq!(log.entries().len(), 1);
    }

    #[test]
    fn cmp_log_entries_returns_in_insertion_order() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(vec![10], vec![20]));
        log.add(CmpEntry::new(vec![30], vec![40]));
        log.add(CmpEntry::new(vec![50], vec![60]));
        let entries = log.entries();
        assert_eq!(entries[0].arg1, vec![10]);
        assert_eq!(entries[1].arg1, vec![30]);
        assert_eq!(entries[2].arg1, vec![50]);
    }

    #[test]
    fn parse_hints_left_right_pattern() {
        let output = "thread 'main' panicked: assertion failed: left=`hello`, right=`world`";
        let hints = parse_cmp_hints_from_output(output);
        assert!(!hints.is_empty());
        assert!(hints
            .iter()
            .any(|e| e.arg1 == b"hello" && e.arg2 == b"world"));
    }

    #[test]
    fn cmplog_mutator_with_same_needle_and_replacement_is_identity() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"AB".to_vec(), b"AB".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"xABy";
        let mut rng = rand::rng();
        // Replacing AB with AB is the identity
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_multiple_entries_selects_one() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"XX".to_vec(), b"YY".to_vec()));
        log.add(CmpEntry::new(b"AA".to_vec(), b"BB".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"XXAA";
        let mut rng = rand::rng();
        // Should not panic regardless of which entry is selected
        let out = m.mutate(input, &mut rng);
        assert_eq!(out.len(), input.len()); // replacements are same length
    }

    #[test]
    fn cmp_entry_ne_when_different() {
        let e1 = CmpEntry::new(vec![1], vec![2]);
        let e2 = CmpEntry::new(vec![1], vec![3]);
        assert_ne!(e1, e2);

        let e3 = CmpEntry::new(vec![5], vec![2]);
        assert_ne!(e1, e3);
    }

    #[test]
    fn parse_hints_returns_multiple_left_right_pairs() {
        let output = "left=`alpha`, right=`beta`\nleft=`foo`, right=`bar`";
        let hints = parse_cmp_hints_from_output(output);
        assert!(hints.len() >= 2);
    }

    // -----------------------------------------------------------------------
    // CmpLogTable tests
    // -----------------------------------------------------------------------

    fn branch(line: u32) -> BranchId {
        BranchId::new(1, line, 0, 0)
    }

    fn make_entry(line: u32, a: &[u8], b: &[u8]) -> CmpLogEntry {
        CmpLogEntry {
            op: CmpOp::Eq,
            operand_a: a.to_vec(),
            operand_b: b.to_vec(),
            branch_id: branch(line),
        }
    }

    #[test]
    fn cmplog_table_record_and_retrieve() {
        let mut table = CmpLogTable::new();
        table.record(make_entry(10, b"hello", b"world"));
        table.record(make_entry(10, b"foo", b"bar"));
        table.record(make_entry(20, b"aaa", b"bbb"));

        assert_eq!(table.len(), 3);
        assert!(!table.is_empty());
        assert!(!table.entries_for(&branch(10)).is_empty());
        assert!(!table.entries_for(&branch(20)).is_empty());
        assert!(table.entries_for(&branch(99)).is_empty());
    }

    #[test]
    fn cmplog_table_evicts_at_256() {
        let mut table = CmpLogTable::new();
        let b = branch(1);
        for i in 0..300u16 {
            table.record(CmpLogEntry {
                op: CmpOp::Eq,
                operand_a: i.to_le_bytes().to_vec(),
                operand_b: vec![0],
                branch_id: b.clone(),
            });
        }
        // Should have exactly 256 entries for that branch (oldest evicted).
        assert_eq!(table.all_entries().count(), CMPLOG_RING_MAX);
        // The oldest entries (0..44) should have been evicted.
        let first = table.entries_for(&b);
        assert!(!first.is_empty());
        // First surviving entry should be index 44 (300 - 256).
        assert_eq!(first[0].operand_a, 44u16.to_le_bytes().to_vec());
    }

    #[test]
    fn cmplog_table_clear() {
        let mut table = CmpLogTable::new();
        table.record(make_entry(1, b"a", b"b"));
        table.record(make_entry(2, b"c", b"d"));
        assert!(!table.is_empty());
        table.clear();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn cmplog_table_default() {
        let table = CmpLogTable::default();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn cmplog_table_all_entries_iterates_all_branches() {
        let mut table = CmpLogTable::new();
        table.record(make_entry(1, b"x", b"y"));
        table.record(make_entry(2, b"a", b"b"));
        table.record(make_entry(3, b"m", b"n"));
        let all: Vec<_> = table.all_entries().collect();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn cmpop_variants_exist() {
        // Ensure all variants are constructable.
        let ops = [
            CmpOp::Eq,
            CmpOp::Ne,
            CmpOp::Lt,
            CmpOp::Le,
            CmpOp::Gt,
            CmpOp::Ge,
            CmpOp::Contains,
            CmpOp::StartsWith,
            CmpOp::EndsWith,
        ];
        assert_eq!(ops.len(), 9);
    }

    #[test]
    fn cmplog_entry_fields() {
        let e = CmpLogEntry {
            op: CmpOp::Ne,
            operand_a: b"left".to_vec(),
            operand_b: b"right".to_vec(),
            branch_id: branch(42),
        };
        assert_eq!(e.op, CmpOp::Ne);
        assert_eq!(e.operand_a, b"left");
        assert_eq!(e.operand_b, b"right");
        assert_eq!(e.branch_id, branch(42));
    }

    // -----------------------------------------------------------------------
    // RedQueenMutator tests
    // -----------------------------------------------------------------------

    #[test]
    fn redqueen_solves_magic_bytes() {
        let mut table = CmpLogTable::new();
        table.record(CmpLogEntry {
            op: CmpOp::Eq,
            operand_a: b"AAAA".to_vec(),
            operand_b: b"MAGIC".to_vec(), // different length on purpose
            branch_id: branch(1),
        });
        let m = RedQueenMutator::new(Arc::new(table));
        let input = b"xxAAAAyy";
        let mut rng = rand::rng();
        let mut found = false;
        for _ in 0..100 {
            let out = m.mutate(input, &mut rng);
            if out == b"xxMAGICyy" {
                found = true;
                break;
            }
        }
        assert!(found, "RedQueenMutator should replace AAAA with MAGIC");
    }

    #[test]
    fn redqueen_no_match_returns_original() {
        let mut table = CmpLogTable::new();
        table.record(make_entry(1, b"ZZZZ", b"WWWW"));
        let m = RedQueenMutator::new(Arc::new(table));
        let input = b"no match here";
        let mut rng = rand::rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn redqueen_empty_table_returns_original() {
        let table = CmpLogTable::new();
        let m = RedQueenMutator::new(Arc::new(table));
        let input = b"test data";
        let mut rng = rand::rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn redqueen_handles_different_length_operands() {
        let mut table = CmpLogTable::new();
        // needle shorter than replacement
        table.record(CmpLogEntry {
            op: CmpOp::Eq,
            operand_a: b"AB".to_vec(),
            operand_b: b"XYZW".to_vec(),
            branch_id: branch(1),
        });
        let m = RedQueenMutator::new(Arc::new(table));
        let input = b"__AB__";
        let mut rng = rand::rng();
        let mut found_grow = false;
        let mut found_shrink = false;
        for _ in 0..200 {
            let out = m.mutate(input, &mut rng);
            if out == b"__XYZW__" {
                found_grow = true; // AB→XYZW: input grew
            }
            // reverse direction: XYZW→AB won't match since input has no XYZW
            if found_grow {
                break;
            }
        }
        // Also test shrink: input contains XYZW, replace with AB
        let input2 = b"__XYZW__";
        for _ in 0..200 {
            let out = m.mutate(input2, &mut rng);
            if out == b"__AB__" {
                found_shrink = true;
                break;
            }
        }
        assert!(found_grow, "Should grow input when replacement is longer");
        assert!(
            found_shrink,
            "Should shrink input when replacement is shorter"
        );
    }

    #[test]
    fn redqueen_empty_needle_returns_original() {
        let mut table = CmpLogTable::new();
        table.record(CmpLogEntry {
            op: CmpOp::Eq,
            operand_a: vec![],
            operand_b: b"something".to_vec(),
            branch_id: branch(1),
        });
        let m = RedQueenMutator::new(Arc::new(table));
        let input = b"test";
        let mut rng = rand::rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn redqueen_name() {
        let m = RedQueenMutator::new(Arc::new(CmpLogTable::new()));
        assert_eq!(m.name(), "redqueen");
    }

    #[test]
    fn redqueen_needle_longer_than_input() {
        let mut table = CmpLogTable::new();
        table.record(make_entry(1, b"LONGNEEDLE", b"X"));
        let m = RedQueenMutator::new(Arc::new(table));
        let input = b"tiny";
        let mut rng = rand::rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_different_length_operands() {
        // Task 5: copy_from_slice panics when needle.len() != replacement.len()
        let mut log = CmpLog::new();
        // 0x41414141 = "AAAA" (4 bytes), 0x424242 = "BBB" (3 bytes)
        log.add(CmpEntry::new(
            0x41414141u32.to_be_bytes().to_vec(),    // b"AAAA"
            0x424242u32.to_be_bytes()[1..].to_vec(), // b"BBB"
        ));
        let m = CmpLogMutator::new(log);
        let input = b"xxAAAAyy";
        let mut rng = rand::rng();
        let mut found = false;
        for _ in 0..100 {
            let out = m.mutate(input, &mut rng);
            assert!(!out.is_empty(), "result must be non-empty");
            if out == b"xxBBByy" {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "CmpLogMutator should splice different-length operands"
        );
    }

    #[test]
    fn entries_for_returns_all_after_wrap() {
        // Task 6: as_slices().0 drops entries after VecDeque wraps
        let mut table = CmpLogTable::new();
        let b = branch(1);
        // Insert 300 entries to force wrap (ring max = 256)
        for i in 0..300u16 {
            table.record(CmpLogEntry {
                op: CmpOp::Eq,
                operand_a: i.to_le_bytes().to_vec(),
                operand_b: vec![0],
                branch_id: b.clone(),
            });
        }
        let entries = table.entries_for(&b);
        assert_eq!(
            entries.len(),
            CMPLOG_RING_MAX,
            "entries_for must return all 256 entries, not just the first contiguous slice"
        );
    }
}
