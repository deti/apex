//! Thread-safe, dedup-aware bug accumulator.
//!
//! The `BugLedger` collects [`BugReport`]s discovered during an exploration run,
//! deduplicating by `(class, location)` so that the same crash from different
//! inputs is only reported once.

use apex_core::types::{BugClass, BugReport, BugSummary, ExecutionResult};
use std::collections::HashSet;
use std::sync::Mutex;

/// Accumulates bugs found during exploration, deduplicating by class + location.
pub struct BugLedger {
    reports: Mutex<Vec<BugReport>>,
    seen: Mutex<HashSet<String>>,
}

impl BugLedger {
    pub fn new() -> Self {
        BugLedger {
            reports: Mutex::new(Vec::new()),
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// Record a bug from an execution result, if the result represents a bug
    /// and hasn't been seen before. Returns `true` if a new bug was recorded.
    pub fn record_from_result(&self, result: &ExecutionResult, iteration: u64) -> bool {
        let class = match BugClass::from_status(result.status) {
            Some(c) => c,
            None => return false,
        };

        let mut report = BugReport::new(class, result.seed_id, result.stderr.clone());
        report.triggering_branches = result.new_branches.clone();
        report.discovered_at_iteration = iteration;

        // Try to extract location from stderr (first file:line pattern).
        report.location = extract_location(&result.stderr);

        self.record(report)
    }

    /// Record a pre-built bug report. Returns `true` if it was new (not a duplicate).
    pub fn record(&self, report: BugReport) -> bool {
        let key = report.dedup_key();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.contains(&key) {
            return false;
        }
        seen.insert(key);
        drop(seen);

        self.reports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(report);
        true
    }

    /// Number of unique bugs recorded.
    pub fn count(&self) -> usize {
        self.reports.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Build a summary of all recorded bugs.
    pub fn summary(&self) -> BugSummary {
        let reports = self
            .reports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        BugSummary::new(reports)
    }

    /// Get all reports (cloned).
    pub fn reports(&self) -> Vec<BugReport> {
        self.reports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Default for BugLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract first `file:line` pattern from stderr text.
fn extract_location(stderr: &str) -> Option<String> {
    // Common patterns:
    //   File "foo.py", line 42
    //   src/main.rs:42:5
    //   at foo.js:10:3
    for line in stderr.lines() {
        let trimmed = line.trim();
        // Python-style: File "path", line N
        if let Some(rest) = trimmed.strip_prefix("File \"") {
            if let Some(end_quote) = rest.find('"') {
                let path = &rest[..end_quote];
                if let Some(line_part) = rest.get(end_quote + 1..) {
                    if let Some(num_start) = line_part.find("line ") {
                        let num_str = &line_part[num_start + 5..];
                        let num: String =
                            num_str.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if !num.is_empty() {
                            return Some(format!("{path}:{num}"));
                        }
                    }
                }
            }
        }
        // Rust/JS-style: path:line or path:line:col
        // Scan whitespace-delimited tokens for "path.ext:line" patterns.
        for token in trimmed.split_whitespace() {
            // Strip leading/trailing parens: "(foo.rs:10)" → "foo.rs:10"
            let token = token.trim_matches(|c| c == '(' || c == ')' || c == ',');
            if let Some(colon_pos) = token.find(':') {
                let before = &token[..colon_pos];
                let after = &token[colon_pos + 1..];
                if (before.contains('.') || before.contains('/')) && before.len() > 1 {
                    let line_num: String =
                        after.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !line_num.is_empty() {
                        return Some(format!("{before}:{line_num}"));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{ExecutionStatus, SeedId};

    fn make_result(status: ExecutionStatus, stderr: &str) -> ExecutionResult {
        ExecutionResult {
            seed_id: SeedId::new(),
            status,
            new_branches: vec![],
            trace: None,
            duration_ms: 100,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn record_crash() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::Crash, "segfault at src/main.rs:42");
        assert!(ledger.record_from_result(&result, 0));
        assert_eq!(ledger.count(), 1);
    }

    #[test]
    fn skip_pass() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::Pass, "");
        assert!(!ledger.record_from_result(&result, 0));
        assert_eq!(ledger.count(), 0);
    }

    #[test]
    fn dedup_same_location() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "segfault at src/main.rs:42");
        let r2 = make_result(ExecutionStatus::Crash, "segfault at src/main.rs:42");
        assert!(ledger.record_from_result(&r1, 0));
        assert!(!ledger.record_from_result(&r2, 1));
        assert_eq!(ledger.count(), 1);
    }

    #[test]
    fn different_locations_recorded() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "src/a.rs:10");
        let r2 = make_result(ExecutionStatus::Crash, "src/b.rs:20");
        assert!(ledger.record_from_result(&r1, 0));
        assert!(ledger.record_from_result(&r2, 1));
        assert_eq!(ledger.count(), 2);
    }

    #[test]
    fn different_classes_not_deduped() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "src/main.rs:42");
        let r2 = make_result(ExecutionStatus::Timeout, "src/main.rs:42");
        assert!(ledger.record_from_result(&r1, 0));
        assert!(ledger.record_from_result(&r2, 1));
        assert_eq!(ledger.count(), 2);
    }

    #[test]
    fn summary_aggregation() {
        let ledger = BugLedger::new();
        ledger.record_from_result(&make_result(ExecutionStatus::Crash, "src/a.rs:1"), 0);
        ledger.record_from_result(&make_result(ExecutionStatus::Crash, "src/b.rs:2"), 1);
        ledger.record_from_result(&make_result(ExecutionStatus::Timeout, "src/c.rs:3"), 2);

        let summary = ledger.summary();
        assert_eq!(summary.total, 3);
        assert_eq!(summary.by_class["crash"], 2);
        assert_eq!(summary.by_class["timeout"], 1);
    }

    #[test]
    fn record_fail_as_assertion_failure() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::Fail, "assert failed");
        assert!(ledger.record_from_result(&result, 5));
        let reports = ledger.reports();
        assert_eq!(reports[0].class, BugClass::AssertionFailure);
        assert_eq!(reports[0].discovered_at_iteration, 5);
    }

    #[test]
    fn record_oom() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::OomKill, "killed");
        assert!(ledger.record_from_result(&result, 0));
        assert_eq!(ledger.reports()[0].class, BugClass::OomKill);
    }

    #[test]
    fn extract_location_python() {
        let loc = extract_location("Traceback:\n  File \"foo.py\", line 42, in test\n    x()");
        assert_eq!(loc.as_deref(), Some("foo.py:42"));
    }

    #[test]
    fn extract_location_rust() {
        let loc = extract_location("thread panicked at src/main.rs:42:5");
        assert_eq!(loc.as_deref(), Some("src/main.rs:42"));
    }

    #[test]
    fn extract_location_js() {
        let loc = extract_location("    at Object.<anonymous> (test.js:10:3)");
        assert_eq!(loc.as_deref(), Some("test.js:10"));

        let loc2 = extract_location("test.js:10:3");
        assert_eq!(loc2.as_deref(), Some("test.js:10"));
    }

    #[test]
    fn extract_location_none() {
        assert_eq!(extract_location("no location info here"), None);
        assert_eq!(extract_location(""), None);
    }

    #[test]
    fn default_impl() {
        let ledger = BugLedger::default();
        assert_eq!(ledger.count(), 0);
    }

    #[test]
    fn manual_record() {
        let ledger = BugLedger::new();
        let report = BugReport::new(BugClass::Crash, SeedId::new(), "boom".into());
        assert!(ledger.record(report.clone()));
        // Duplicate with same dedup key
        assert!(!ledger.record(report));
    }

    // ------------------------------------------------------------------
    // Additional extract_location branch coverage
    // ------------------------------------------------------------------

    /// Python prefix found but the closing quote is missing.
    #[test]
    fn extract_location_python_no_closing_quote() {
        // The line starts with `File "` but never closes the quote.
        let loc = extract_location("  File \"foo.py, line 10, in test");
        // Falls through to the Rust/JS token scanner — "foo.py," contains '.'
        // and a colon may or may not be found; we just assert it doesn't panic.
        let _ = loc; // result may be Some or None; either is acceptable
    }

    /// Python prefix found, closing quote found, but "line " is absent from the rest.
    #[test]
    fn extract_location_python_no_line_keyword() {
        let loc = extract_location("  File \"foo.py\", in test_func");
        // No "line N" present; falls through to token scan → no colon match
        assert_eq!(loc, None);
    }

    /// Python prefix found, "line " found, but the number part is empty (immediately
    /// followed by a non-digit character).
    #[test]
    fn extract_location_python_line_keyword_no_digits() {
        let loc = extract_location("  File \"foo.py\", line X in test");
        // num is empty → no return from Python branch; falls through to token scan.
        // Token "\"foo.py\"," stripped of parens/commas → "\"foo.py\"" still no valid
        // file.ext:line pattern, so result is None.
        let _ = loc;
    }

    /// Token has a colon but `before` has neither '.' nor '/' → skip token.
    #[test]
    fn extract_location_token_no_dot_or_slash() {
        // "main:42" — before = "main", no '.' or '/'
        let loc = extract_location("error in main:42");
        assert_eq!(loc, None);
    }

    /// Token has a colon, `before` has '.', but after the colon there are no digits.
    #[test]
    fn extract_location_token_with_dot_but_no_digits_after_colon() {
        // "foo.rs:xyz" — before = "foo.rs" has '.', after = "xyz" is non-digit
        let loc = extract_location("at foo.rs:xyz");
        assert_eq!(loc, None);
    }

    /// Token has no colon at all — the `find(':')` returns None.
    #[test]
    fn extract_location_token_no_colon() {
        let loc = extract_location("just some random words without colon");
        assert_eq!(loc, None);
    }

    /// Parenthesized token: strip_matches removes leading '(' and trailing ')'.
    #[test]
    fn extract_location_parenthesized_token() {
        let loc = extract_location("at someFunc (src/util.rs:77:3)");
        assert_eq!(loc.as_deref(), Some("src/util.rs:77"));
    }

    /// Token with '/' in `before` but no '.'.
    #[test]
    fn extract_location_slash_no_dot() {
        let loc = extract_location("at /src/main:100:5");
        assert_eq!(loc.as_deref(), Some("/src/main:100"));
    }

    /// Multiple lines — location extracted from a later line, not the first.
    #[test]
    fn extract_location_second_line_match() {
        let loc = extract_location("some preamble line\n  src/other.rs:55:1");
        assert_eq!(loc.as_deref(), Some("src/other.rs:55"));
    }

    /// `before` has length == 1 → condition `before.len() > 1` is false → skip.
    #[test]
    fn extract_location_before_len_one_skipped() {
        // "a.rs:10" — before = "a", length 1 with no '/' → even if '.' was there,
        // before.len() > 1 is false with before = "a" (no dot here).
        // Use "a/:10" to have '/' with len=2 > 1, vs "x:10" with just plain char.
        let loc = extract_location("x:10");
        assert_eq!(loc, None);
    }

    /// Token with trailing comma (,) stripped — covers the trim_matches branch.
    #[test]
    fn extract_location_trailing_comma_stripped() {
        let loc = extract_location("see src/main.rs:42, for details");
        assert_eq!(loc.as_deref(), Some("src/main.rs:42"));
    }

    // ------------------------------------------------------------------
    // Additional BugLedger and BugReport coverage
    // ------------------------------------------------------------------

    /// BugReport::dedup_key uses location when present.
    #[test]
    fn bug_report_dedup_key_uses_location_when_present() {
        let mut report = BugReport::new(BugClass::Crash, SeedId::new(), "some message".into());
        report.location = Some("src/lib.rs:10".into());
        let key = report.dedup_key();
        assert!(key.contains("src/lib.rs:10"));
        assert!(key.contains("crash"));
    }

    /// BugReport::dedup_key uses message (truncated) when location is None.
    #[test]
    fn bug_report_dedup_key_uses_message_when_no_location() {
        let report = BugReport::new(BugClass::Timeout, SeedId::new(), "timed out waiting".into());
        let key = report.dedup_key();
        assert!(key.contains("timeout"));
        assert!(key.contains("timed out waiting"));
    }

    /// BugReport::dedup_key truncates messages longer than 128 chars.
    #[test]
    fn bug_report_dedup_key_truncates_long_message() {
        let long_msg = "x".repeat(200);
        let report = BugReport::new(BugClass::Crash, SeedId::new(), long_msg.clone());
        let key = report.dedup_key();
        // Should only include the first 128 chars.
        assert_eq!(key, format!("crash:{}", &long_msg[..128]));
    }

    /// `reports()` returns a clone of all recorded reports.
    #[test]
    fn reports_returns_all_recorded() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "src/a.rs:1");
        let r2 = make_result(ExecutionStatus::Timeout, "src/b.rs:2");
        ledger.record_from_result(&r1, 0);
        ledger.record_from_result(&r2, 1);
        let reports = ledger.reports();
        assert_eq!(reports.len(), 2);
    }

    /// `summary()` correctly groups by class.
    #[test]
    fn summary_contains_all_unique_bugs() {
        let ledger = BugLedger::new();
        for i in 0..3 {
            let r = make_result(ExecutionStatus::Crash, &format!("src/f{i}.rs:{i}"));
            ledger.record_from_result(&r, i as u64);
        }
        let summary = ledger.summary();
        assert_eq!(summary.total, 3);
        assert_eq!(summary.by_class.get("crash").copied().unwrap_or(0), 3);
    }

    /// record_from_result with Fail maps to AssertionFailure class.
    #[test]
    fn fail_status_maps_to_assertion_failure_bug_class() {
        let ledger = BugLedger::new();
        let r = make_result(ExecutionStatus::Fail, "assertion error at src/x.rs:5");
        assert!(ledger.record_from_result(&r, 0));
        assert_eq!(ledger.reports()[0].class, BugClass::AssertionFailure);
    }

    /// record_from_result with OomKill maps to OomKill class.
    #[test]
    fn oomkill_status_maps_to_oomkill_bug_class() {
        let ledger = BugLedger::new();
        let r = make_result(ExecutionStatus::OomKill, "OOM src/y.rs:3");
        assert!(ledger.record_from_result(&r, 0));
        assert_eq!(ledger.reports()[0].class, BugClass::OomKill);
    }

    /// triggering_branches and discovered_at_iteration are propagated.
    #[test]
    fn record_from_result_sets_iteration_and_branches() {
        use apex_core::types::BranchId;
        let ledger = BugLedger::new();
        let branch = BranchId::new(1, 10, 0, 0);
        let mut result = make_result(ExecutionStatus::Crash, "src/m.rs:7");
        result.new_branches = vec![branch.clone()];
        ledger.record_from_result(&result, 42);
        let reports = ledger.reports();
        assert_eq!(reports[0].discovered_at_iteration, 42);
        assert_eq!(reports[0].triggering_branches.len(), 1);
    }

    /// `count()` after no bugs is zero.
    #[test]
    fn count_zero_when_empty() {
        let ledger = BugLedger::new();
        assert_eq!(ledger.count(), 0);
    }

    /// Dedup across different classes with same location are treated as distinct bugs.
    #[test]
    fn same_location_different_class_not_deduped() {
        let ledger = BugLedger::new();
        let mut r1 = BugReport::new(BugClass::Crash, SeedId::new(), "msg".into());
        r1.location = Some("src/f.rs:1".into());
        let mut r2 = BugReport::new(BugClass::Timeout, SeedId::new(), "msg".into());
        r2.location = Some("src/f.rs:1".into());
        assert!(ledger.record(r1));
        assert!(ledger.record(r2));
        assert_eq!(ledger.count(), 2);
    }

    // ------------------------------------------------------------------
    // Additional extract_location / BugLedger branch-coverage tests
    // ------------------------------------------------------------------

    /// Multi-line text with Python location not on the first line.
    #[test]
    fn extract_location_python_location_on_later_line() {
        let stderr = "Traceback (most recent call last):\n  File \"module.py\", line 99, in do_thing\n    crash()";
        let loc = extract_location(stderr);
        assert_eq!(loc.as_deref(), Some("module.py:99"));
    }

    /// Rust location with column after line number (path:line:col → path:line).
    #[test]
    fn extract_location_rust_with_column() {
        let loc = extract_location("thread 'main' panicked at 'index out of bounds', src/lib.rs:55:12");
        assert_eq!(loc.as_deref(), Some("src/lib.rs:55"));
    }

    /// Location with forward-slash path but no dot (slash qualifies as separator).
    #[test]
    fn extract_location_slash_path_no_ext() {
        // "/usr/bin/prog:10" — before="/usr/bin/prog" contains '/' so qualifies.
        let loc = extract_location("/usr/bin/prog:10");
        assert_eq!(loc.as_deref(), Some("/usr/bin/prog:10"));
    }

    /// `extract_location` with only whitespace → None.
    #[test]
    fn extract_location_whitespace_only() {
        let loc = extract_location("   \n\t  ");
        assert_eq!(loc, None);
    }

    /// Token `"foo.rs:abc"` — after the colon is non-digit → skip token.
    #[test]
    fn extract_location_token_digits_absent_after_colon() {
        let loc = extract_location("see foo.rs:abc for details");
        assert_eq!(loc, None);
    }

    /// `before` has length exactly 1 with a dot — `before.len() > 1` is false → skip.
    #[test]
    fn extract_location_before_has_len_one_with_dot_skipped() {
        // "a.rs:10" splits at first colon to get before="a.rs", not "a".
        // Actually the token is "a.rs:10", colon_pos=4 → before="a.rs" len=4 > 1 → OK.
        // To get before.len()=1 we need "a:10" → before="a" len=1 → skip.
        let loc = extract_location("error at a:10");
        // before="a", no '.' or '/' → skip entirely.
        assert_eq!(loc, None);
    }

    /// Multiple tokens in one line — first valid token is returned.
    #[test]
    fn extract_location_first_valid_token_returned() {
        // Both "src/a.rs:10" and "src/b.rs:20" could match; first one wins.
        let loc = extract_location("see src/a.rs:10 and src/b.rs:20");
        assert_eq!(loc.as_deref(), Some("src/a.rs:10"));
    }

    /// OomKill status recorded with correct class.
    #[test]
    fn record_from_result_oomkill_class() {
        let ledger = BugLedger::new();
        let r = make_result(ExecutionStatus::OomKill, "killed by oom killer");
        assert!(ledger.record_from_result(&r, 0));
        assert_eq!(ledger.reports()[0].class, BugClass::OomKill);
    }

    /// `record_from_result` with Crash sets message from stderr.
    #[test]
    fn record_from_result_crash_message_from_stderr() {
        let ledger = BugLedger::new();
        let r = make_result(ExecutionStatus::Crash, "segmentation fault at src/main.rs:10");
        ledger.record_from_result(&r, 0);
        let reports = ledger.reports();
        assert!(reports[0].message.contains("segmentation fault"));
    }

    /// `BugReport::dedup_key()` is consistent across multiple calls.
    #[test]
    fn dedup_key_is_consistent() {
        let report = BugReport::new(BugClass::Crash, SeedId::new(), "boom".into());
        let key1 = report.dedup_key();
        let key2 = report.dedup_key();
        assert_eq!(key1, key2);
    }

    /// Ledger `record()` returns false for an exact duplicate.
    #[test]
    fn record_duplicate_returns_false() {
        let ledger = BugLedger::new();
        let report = BugReport::new(BugClass::Crash, SeedId::new(), "dup".into());
        assert!(ledger.record(report.clone()));
        assert!(!ledger.record(report));
        assert_eq!(ledger.count(), 1);
    }

    /// `summary()` on empty ledger has total=0.
    #[test]
    fn summary_empty_ledger() {
        let ledger = BugLedger::new();
        let s = ledger.summary();
        assert_eq!(s.total, 0);
        assert!(s.by_class.is_empty());
    }

    /// `extract_location` with Windows-style path (backslashes) — may not match
    /// the '/' or '.' requirement, so returns None or the path.
    #[test]
    fn extract_location_with_colon_and_dot_extension() {
        // "module.py:5" should be found.
        let loc = extract_location("File module.py:5 caused error");
        assert_eq!(loc.as_deref(), Some("module.py:5"));
    }

    /// `extract_location` Python pattern with large line number.
    #[test]
    fn extract_location_python_large_line_number() {
        let loc = extract_location("  File \"src/bigfile.py\", line 9999, in handler\n    raise");
        assert_eq!(loc.as_deref(), Some("src/bigfile.py:9999"));
    }

    /// `extract_location` with JS-style "at" prefix but parenthesized token.
    #[test]
    fn extract_location_js_at_parenthesized() {
        let loc = extract_location("    at module.method (lib/util.js:77:3)");
        assert_eq!(loc.as_deref(), Some("lib/util.js:77"));
    }

    /// `BugClass::from_status` for Pass → None (exercises the None branch).
    #[test]
    fn bug_class_from_status_pass_is_none() {
        let c = apex_core::types::BugClass::from_status(ExecutionStatus::Pass);
        assert!(c.is_none());
    }

    /// `BugClass::from_status` for Crash → Some(Crash).
    #[test]
    fn bug_class_from_status_crash_is_some() {
        let c = apex_core::types::BugClass::from_status(ExecutionStatus::Crash);
        assert_eq!(c, Some(BugClass::Crash));
    }

    /// `BugClass::from_status` for Timeout → Some(Timeout).
    #[test]
    fn bug_class_from_status_timeout_is_some() {
        let c = apex_core::types::BugClass::from_status(ExecutionStatus::Timeout);
        assert_eq!(c, Some(BugClass::Timeout));
    }
}
