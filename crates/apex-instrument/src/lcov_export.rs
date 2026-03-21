//! Export APEX coverage data to LCOV info format.
//!
//! This is the inverse of [`crate::import::parse_lcov`]. Given APEX branch data,
//! it produces an LCOV-format string that round-trips through the parser.

use apex_core::types::BranchId;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::path::PathBuf;

/// Export coverage data as an LCOV info string.
///
/// Groups branches by file, emitting `SF:`, `DA:`, `BRDA:`, summary counters,
/// and `end_of_record` for each file. The output round-trips through
/// [`crate::import::parse_lcov`].
pub fn export_lcov(
    all_branches: &[BranchId],
    executed_branches: &[BranchId],
    file_paths: &HashMap<u64, PathBuf>,
) -> String {
    // Build a set of executed branch IDs for O(1) lookup.
    let executed_set: std::collections::HashSet<&BranchId> = executed_branches.iter().collect();

    // Group branches by file_id, preserving insertion order within each file
    // via BTreeMap<line> so output is deterministic.
    let mut files: BTreeMap<u64, Vec<&BranchId>> = BTreeMap::new();
    for bid in all_branches {
        files.entry(bid.file_id).or_default().push(bid);
    }

    let mut out = String::new();
    let _ = writeln!(out, "TN:");

    for (file_id, branches) in &files {
        // Resolve file path
        let path = file_paths
            .get(file_id)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<{file_id:016x}>"));

        let _ = writeln!(out, "SF:{path}");

        // Separate DA (direction == 0, no BRDA for this line) from BRDA records.
        // A branch with direction > 0 is always a BRDA record.
        // A branch with direction == 0 could be either DA or BRDA depending on
        // whether other branches share the same (file_id, line) with direction > 0.
        //
        // To faithfully round-trip the parser's output, we replicate its logic:
        // - DA records have direction == 0 (line coverage)
        // - BRDA records have any direction value (branch coverage)
        // The parser creates separate BranchId entries for DA and BRDA, so we
        // need to figure out which is which.
        //
        // Heuristic: if a BranchId has col == 0 and direction == 0 and there is
        // no other BranchId on the same line with direction > 0, it's a DA line.
        // Actually, looking at the parser more carefully: DA always creates
        // direction=0, BRDA creates whatever direction the data has. They can
        // coexist on the same line. So we track them by their original type.
        //
        // Since we don't store the original record type, we use the convention
        // from the parser: DA records always have direction == 0. If there are
        // also BRDA records with direction == 0 on the same line, the parser
        // would have created separate BranchId entries (same key → deduplicated
        // by the caller, but in the Vec they are separate entries).
        //
        // For round-trip fidelity, we classify:
        // - Branches where no other branch shares the same line → DA
        // - Branches where multiple branches share a line → first with dir==0 is DA,
        //   rest are BRDA
        //
        // Simplest correct approach: group by line, then decide.

        // Group by line number
        let mut by_line: BTreeMap<u32, Vec<&BranchId>> = BTreeMap::new();
        for bid in branches {
            by_line.entry(bid.line).or_default().push(bid);
        }

        let mut da_lines: Vec<(u32, i64)> = Vec::new();
        let mut brda_records: Vec<(u32, u32, u32, i64)> = Vec::new(); // (line, block, branch, count)

        for (line, line_branches) in &by_line {
            // Check if any branch on this line has direction > 0
            let has_brda = line_branches.iter().any(|b| b.direction > 0);

            if !has_brda && line_branches.len() == 1 {
                // Single branch with direction==0 → DA record
                let bid = line_branches[0];
                let count = if executed_set.contains(bid) { 1 } else { 0 };
                da_lines.push((*line, count));
            } else {
                // Multiple branches on same line, or has direction > 0 → classify
                let mut emitted_da = false;
                for bid in line_branches {
                    if bid.direction == 0 && !emitted_da && !has_brda {
                        // DA record
                        let count = if executed_set.contains(bid) { 1 } else { 0 };
                        da_lines.push((*line, count));
                        emitted_da = true;
                    } else if bid.direction == 0 && has_brda {
                        // This is a DA record that coexists with BRDA records
                        let count = if executed_set.contains(bid) { 1 } else { 0 };
                        da_lines.push((*line, count));
                    } else {
                        // BRDA record
                        let count = if executed_set.contains(bid) { 1 } else { 0 };
                        // block number: we don't have it, use 0
                        // branch number: use direction
                        brda_records.push((*line, 0, bid.direction as u32, count));
                    }
                }
            }
        }

        // Emit DA lines
        for (line, count) in &da_lines {
            let _ = writeln!(out, "DA:{line},{count}");
        }

        // Emit BRDA records
        for (line, block, branch, count) in &brda_records {
            let count_str = if *count == 0 {
                "0".to_string()
            } else {
                count.to_string()
            };
            let _ = writeln!(out, "BRDA:{line},{block},{branch},{count_str}");
        }

        // Summary counters
        let lf = da_lines.len();
        let lh = da_lines.iter().filter(|(_, c)| *c > 0).count();
        let _ = writeln!(out, "LF:{lf}");
        let _ = writeln!(out, "LH:{lh}");
        let _ = writeln!(out, "end_of_record");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;
    use apex_core::hash::fnv1a_hash;

    fn make_branch(file_id: u64, line: u32, direction: u8) -> BranchId {
        BranchId::new(file_id, line, 0, direction)
    }

    #[test]
    fn lcov_export_round_trip_da_only() {
        let file_id = fnv1a_hash("src/lib.rs");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("src/lib.rs"));

        let all = vec![
            make_branch(file_id, 1, 0),
            make_branch(file_id, 2, 0),
            make_branch(file_id, 3, 0),
        ];
        let executed = vec![make_branch(file_id, 1, 0), make_branch(file_id, 3, 0)];

        let lcov = export_lcov(&all, &executed, &file_paths);
        let (parsed_all, parsed_exec, parsed_paths) =
            import::parse_lcov(&lcov).unwrap();

        assert_eq!(parsed_all.len(), all.len());
        assert_eq!(parsed_exec.len(), executed.len());
        assert_eq!(parsed_paths.len(), file_paths.len());
        assert!(parsed_paths.values().any(|p| p == std::path::Path::new("src/lib.rs")));
    }

    #[test]
    fn lcov_export_round_trip_with_brda() {
        let file_id = fnv1a_hash("src/main.rs");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("src/main.rs"));

        let all = vec![
            make_branch(file_id, 10, 0), // DA
            make_branch(file_id, 10, 1), // BRDA direction=1
            make_branch(file_id, 15, 0), // DA only
        ];
        let executed = vec![
            make_branch(file_id, 10, 0),
            make_branch(file_id, 10, 1),
        ];

        let lcov = export_lcov(&all, &executed, &file_paths);
        let (parsed_all, parsed_exec, _) = import::parse_lcov(&lcov).unwrap();

        assert_eq!(parsed_all.len(), all.len());
        assert_eq!(parsed_exec.len(), executed.len());
    }

    #[test]
    fn lcov_export_markers_present() {
        let file_id = fnv1a_hash("src/app.py");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("src/app.py"));

        let all = vec![
            make_branch(file_id, 1, 0),
            make_branch(file_id, 2, 0),
            make_branch(file_id, 5, 0),
            make_branch(file_id, 5, 1),
        ];
        let executed = vec![make_branch(file_id, 1, 0), make_branch(file_id, 5, 1)];

        let lcov = export_lcov(&all, &executed, &file_paths);

        assert!(lcov.contains("TN:"), "missing TN marker");
        assert!(lcov.contains("SF:src/app.py"), "missing SF marker");
        assert!(lcov.contains("DA:1,1"), "missing DA for covered line");
        assert!(lcov.contains("DA:2,0"), "missing DA for uncovered line");
        assert!(lcov.contains("BRDA:5,0,1,1"), "missing BRDA for covered branch");
        assert!(lcov.contains("LF:"), "missing LF marker");
        assert!(lcov.contains("LH:"), "missing LH marker");
        assert!(lcov.contains("end_of_record"), "missing end_of_record");
    }

    #[test]
    fn lcov_export_multiple_files() {
        let fid1 = fnv1a_hash("src/a.rs");
        let fid2 = fnv1a_hash("src/b.rs");
        let mut file_paths = HashMap::new();
        file_paths.insert(fid1, PathBuf::from("src/a.rs"));
        file_paths.insert(fid2, PathBuf::from("src/b.rs"));

        let all = vec![
            make_branch(fid1, 1, 0),
            make_branch(fid2, 1, 0),
            make_branch(fid2, 2, 0),
        ];
        let executed = vec![make_branch(fid1, 1, 0)];

        let lcov = export_lcov(&all, &executed, &file_paths);

        // Should contain two SF records
        let sf_count = lcov.lines().filter(|l| l.starts_with("SF:")).count();
        assert_eq!(sf_count, 2, "expected 2 SF records");

        let eor_count = lcov
            .lines()
            .filter(|l| l == &"end_of_record")
            .count();
        assert_eq!(eor_count, 2, "expected 2 end_of_record markers");
    }

    #[test]
    fn lcov_export_empty() {
        let file_paths = HashMap::new();
        let lcov = export_lcov(&[], &[], &file_paths);
        assert!(lcov.contains("TN:"));
        // No SF records
        assert!(!lcov.contains("SF:"));
    }

    #[test]
    fn lcov_export_round_trip_full() {
        // Build data, export, parse, re-export, compare
        let file_id = fnv1a_hash("src/lib.rs");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("src/lib.rs"));

        let all = vec![
            make_branch(file_id, 1, 0),
            make_branch(file_id, 2, 0),
            make_branch(file_id, 3, 0),
            make_branch(file_id, 10, 0),
            make_branch(file_id, 10, 1),
            make_branch(file_id, 10, 2),
        ];
        let executed = vec![
            make_branch(file_id, 1, 0),
            make_branch(file_id, 3, 0),
            make_branch(file_id, 10, 0),
            make_branch(file_id, 10, 1),
        ];

        let lcov1 = export_lcov(&all, &executed, &file_paths);
        let (parsed_all, parsed_exec, parsed_paths) =
            import::parse_lcov(&lcov1).unwrap();

        // Re-export from parsed data
        let lcov2 = export_lcov(&parsed_all, &parsed_exec, &parsed_paths);

        assert_eq!(lcov1, lcov2, "round-trip export should be identical");
    }
}
