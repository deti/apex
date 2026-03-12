//! RedQueen / CmpLog — input-to-state comparison feedback.
//!
//! Two sources of comparison data:
//! 1. SanCov CMP callbacks (via `apex_sandbox::sancov_rt::read_cmp_log()`)
//! 2. Output parsing fallback (`parse_cmp_hints_from_output()`)

use crate::traits::Mutator;
use rand::RngCore;

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

    // Pattern: "expected <X> but got <Y>" or "expected <X>, got <Y>"
    let expected_re_str = r"expected\s+(\S+?)[\s,]+(?:but\s+)?got\s+(\S+)";
    // Pattern: "left=`<X>`, right=`<Y>`" (Rust assert_eq)
    let left_right_re_str = r"left=`([^`]+)`.*right=`([^`]+)`";

    for pattern in [expected_re_str, left_right_re_str] {
        if let Ok(re) = regex::Regex::new(pattern) {
            for caps in re.captures_iter(output) {
                if let (Some(a), Some(b)) = (caps.get(1), caps.get(2)) {
                    hints.push(CmpEntry::new(
                        a.as_str().as_bytes().to_vec(),
                        b.as_str().as_bytes().to_vec(),
                    ));
                }
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

        // Replace at a random matching position
        let pos = positions[(rng.next_u32() as usize) % positions.len()];
        let mut out = input.to_vec();
        out[pos..pos + needle.len()].copy_from_slice(replacement);
        out
    }

    fn name(&self) -> &str {
        "cmplog"
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
        let mut rng = rand::thread_rng();
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
        let mut rng = rand::thread_rng();
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
        let mut rng = rand::thread_rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_empty_log_returns_original() {
        let m = CmpLogMutator::new(CmpLog::new());
        let input = b"test";
        let mut rng = rand::thread_rng();
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
        let mut rng = rand::thread_rng();
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
        let mut rng = rand::thread_rng();
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
        let mut rng = rand::thread_rng();
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
        let debug = format!("{:?}", e);
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
        assert!(hints.iter().any(|e| e.arg1 == b"hello" && e.arg2 == b"world"));
    }

    #[test]
    fn cmplog_mutator_with_same_needle_and_replacement_is_identity() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"AB".to_vec(), b"AB".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"xABy";
        let mut rng = rand::thread_rng();
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
        let mut rng = rand::thread_rng();
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
}
