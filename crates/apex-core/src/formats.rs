//! Coverage format import/export — LCOV, Cobertura, and friends.
//!
//! ## Supported formats
//! - **LCOV** (`import_lcov` / `export_lcov`) — line-based text format, widely used by C/C++/Rust tools.
//! - **Cobertura** (`import_cobertura` / `export_cobertura`) — XML, used by Java/Python CI tooling.
//!
//! ## BranchCoverage
//! A lightweight struct that captures the information common to all supported formats:
//! source file, line number, branch index and arm, and hit count.

use crate::error::{ApexError, Result};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Format enum
// ---------------------------------------------------------------------------

/// Recognised external coverage formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverageFormat {
    Lcov,
    Cobertura,
    Jacoco,
    Istanbul,
    V8,
    GoProfile,
    SimpleCov,
}

impl std::fmt::Display for CoverageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CoverageFormat::Lcov => "lcov",
            CoverageFormat::Cobertura => "cobertura",
            CoverageFormat::Jacoco => "jacoco",
            CoverageFormat::Istanbul => "istanbul",
            CoverageFormat::V8 => "v8",
            CoverageFormat::GoProfile => "go-profile",
            CoverageFormat::SimpleCov => "simplecov",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for CoverageFormat {
    type Err = ApexError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "lcov" => Ok(CoverageFormat::Lcov),
            "cobertura" => Ok(CoverageFormat::Cobertura),
            "jacoco" => Ok(CoverageFormat::Jacoco),
            "istanbul" => Ok(CoverageFormat::Istanbul),
            "v8" => Ok(CoverageFormat::V8),
            "go-profile" | "goprofile" => Ok(CoverageFormat::GoProfile),
            "simplecov" => Ok(CoverageFormat::SimpleCov),
            other => Err(ApexError::Config(format!(
                "unknown coverage format: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// BranchCoverage — portable representation used by all import/export fns
// ---------------------------------------------------------------------------

/// Portable per-branch coverage record.
///
/// Represents one branch arm in an external coverage report.  This type is
/// intentionally simple: it captures just the information that every supported
/// format encodes, without APEX-internal identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchCoverage {
    /// Source file path as it appears in the coverage report.
    pub file: String,
    /// 1-based source line of the branch point.
    pub line: u32,
    /// Branch-point index on the line (used by LCOV `BRDA` block number).
    pub branch_index: u32,
    /// Arm index within the branch point (0 = true/taken, 1 = false/not-taken, …).
    pub arm: u32,
    /// Execution count.  `0` means the arm was not taken.
    pub hits: u32,
}

impl BranchCoverage {
    /// Create a new branch coverage record.
    pub fn new(file: impl Into<String>, line: u32, branch_index: u32, arm: u32, hits: u32) -> Self {
        BranchCoverage {
            file: file.into(),
            line,
            branch_index,
            arm,
            hits,
        }
    }
}

// ---------------------------------------------------------------------------
// LCOV
// ---------------------------------------------------------------------------

/// Parse an LCOV coverage report.
///
/// Recognises `SF:` (source file), `DA:` (line hit), and `BRDA:` (branch) records.
/// Other records (`FN:`, `FNDA:`, `FNF:`, `FNH:`, `BRF:`, `BRH:`, `LF:`, `LH:`,
/// `end_of_record`) are silently skipped.
///
/// Returns one `BranchCoverage` per `BRDA:` line.  `DA:` lines without a branch
/// context are not converted (they carry only line-level, not branch-level, data).
pub fn import_lcov(data: &str) -> Result<Vec<BranchCoverage>> {
    let mut records = Vec::new();
    let mut current_file = String::new();

    for (lineno, raw) in data.lines().enumerate() {
        let line = raw.trim();

        if let Some(rest) = line.strip_prefix("SF:") {
            current_file = rest.trim().to_string();
            continue;
        }

        if line == "end_of_record" {
            current_file.clear();
            continue;
        }

        // BRDA:<line>,<block>,<branch>,<taken>
        if let Some(rest) = line.strip_prefix("BRDA:") {
            let parts: Vec<&str> = rest.splitn(4, ',').collect();
            if parts.len() != 4 {
                return Err(ApexError::Config(format!(
                    "LCOV line {}: malformed BRDA record: {line}",
                    lineno + 1
                )));
            }

            let src_line = parse_u32(parts[0], "BRDA line", lineno)?;
            let block = parse_u32(parts[1], "BRDA block", lineno)?;
            let branch = parse_u32(parts[2], "BRDA branch", lineno)?;
            // <taken> is either a count or "-" (not executed)
            let hits = if parts[3].trim() == "-" {
                0
            } else {
                parse_u32(parts[3].trim(), "BRDA hits", lineno)?
            };

            records.push(BranchCoverage::new(
                current_file.clone(),
                src_line,
                block,
                branch,
                hits,
            ));
        }
        // DA lines are intentionally skipped — line coverage is not BranchCoverage.
    }

    Ok(records)
}

/// Serialise branch coverage data to LCOV format.
///
/// Produces a minimal but valid LCOV file containing only `SF:`, `BRDA:`,
/// `BRF:`, `BRH:`, and `end_of_record` entries.
pub fn export_lcov(branches: &[BranchCoverage]) -> String {
    use std::collections::BTreeMap;

    // Group by file, preserving insertion order within each file.
    let mut by_file: BTreeMap<&str, Vec<&BranchCoverage>> = BTreeMap::new();
    for b in branches {
        by_file.entry(b.file.as_str()).or_default().push(b);
    }

    let mut out = String::new();
    for (file, entries) in &by_file {
        out.push_str("SF:");
        out.push_str(file);
        out.push('\n');

        let mut hit_count = 0u32;
        for b in entries {
            // BRDA:<line>,<block>,<branch>,<taken>
            let taken = if b.hits > 0 {
                hit_count += 1;
                b.hits.to_string()
            } else {
                "-".to_string()
            };
            out.push_str(&format!(
                "BRDA:{},{},{},{}\n",
                b.line, b.branch_index, b.arm, taken
            ));
        }

        out.push_str(&format!("BRF:{}\n", entries.len()));
        out.push_str(&format!("BRH:{hit_count}\n"));
        out.push_str("end_of_record\n");
    }

    out
}

// ---------------------------------------------------------------------------
// Cobertura (XML)
// ---------------------------------------------------------------------------

/// Parse a Cobertura XML coverage report.
///
/// Extracts `<condition>` elements from `<conditions>` blocks inside `<line>`
/// elements, converting them to `BranchCoverage` records.
///
/// This is a lightweight hand-rolled parser that avoids pulling in an XML
/// library — it handles well-formed Cobertura output from standard tools.
pub fn import_cobertura(data: &str) -> Result<Vec<BranchCoverage>> {
    let mut records = Vec::new();
    let mut current_file = String::new();
    let mut current_line: u32 = 0;
    let mut branch_index: u32 = 0;

    for (lineno, raw) in data.lines().enumerate() {
        let line = raw.trim();

        // <class filename="src/main.rs" ...>
        if line.contains("<class") {
            if let Some(fname) = attr_value(line, "filename") {
                current_file = fname;
            }
            continue;
        }

        // <line number="42" hits="3" branch="true" ...>
        if line.contains("<line") && !line.contains("</line>") {
            current_line = attr_value(line, "number")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            branch_index = 0;
            continue;
        }

        // <condition number="0" type="jump" coverage="100%"/>
        // Note: check for "<condition " (with space) or "<condition/>" to avoid
        // matching the surrounding "<conditions>" wrapper element.
        if (line.contains("<condition ") || line.contains("<condition/>"))
            && !line.contains("<conditions")
        {
            let arm = attr_value(line, "number")
                .and_then(|v| v.parse::<u32>().ok())
                .ok_or_else(|| {
                    ApexError::Config(format!(
                        "Cobertura line {}: missing 'number' in <condition>",
                        lineno + 1
                    ))
                })?;

            // coverage="50%" means one arm hit, "100%" both, "0%" none.
            let cov_str = attr_value(line, "coverage").unwrap_or_default();
            let hits = coverage_pct_to_hits(&cov_str, arm);

            if !current_file.is_empty() {
                records.push(BranchCoverage::new(
                    current_file.clone(),
                    current_line,
                    branch_index,
                    arm,
                    hits,
                ));
            }

            // Each two arms belong to the same branch point; increment block after arm 1.
            if arm == 1 {
                branch_index += 1;
            }
        }
    }

    Ok(records)
}

/// Serialise branch coverage data to Cobertura XML format.
///
/// Produces a minimal valid Cobertura document with `<coverage>`, `<packages>`,
/// `<package>`, `<classes>`, `<class>`, `<lines>`, `<line>`, `<conditions>`,
/// and `<condition>` elements.
pub fn export_cobertura(branches: &[BranchCoverage]) -> String {
    use std::collections::BTreeMap;

    // Group by file → line → (branch_index, arm) → hits.
    let mut by_file: BTreeMap<&str, BTreeMap<u32, Vec<&BranchCoverage>>> = BTreeMap::new();
    for b in branches {
        by_file
            .entry(b.file.as_str())
            .or_default()
            .entry(b.line)
            .or_default()
            .push(b);
    }

    let total_branches: usize = branches.len();
    let covered_branches: usize = branches.iter().filter(|b| b.hits > 0).count();
    let branch_rate = if total_branches == 0 {
        1.0f64
    } else {
        covered_branches as f64 / total_branches as f64
    };

    let mut out = String::from("<?xml version=\"1.0\" ?>\n");
    out.push_str(
        "<!DOCTYPE coverage SYSTEM \"http://cobertura.sourceforge.net/xml/coverage-04.dtd\">\n",
    );
    out.push_str(&format!(
        "<coverage branch-rate=\"{branch_rate:.4}\" branches-covered=\"{covered_branches}\" branches-valid=\"{total_branches}\" timestamp=\"0\" version=\"apex\">\n"
    ));
    out.push_str("  <packages>\n");
    out.push_str("    <package name=\".\" branch-rate=\"1\" complexity=\"0\">\n");
    out.push_str("      <classes>\n");

    for (file, lines) in &by_file {
        out.push_str(&format!(
            "        <class filename=\"{file}\" name=\"{file}\" branch-rate=\"1\" complexity=\"0\">\n"
        ));
        out.push_str("          <lines>\n");

        for (line_no, arms) in lines {
            let line_hits: u32 = arms.iter().map(|b| b.hits).max().unwrap_or(0);
            let has_conditions = !arms.is_empty();
            out.push_str(&format!(
                "            <line number=\"{line_no}\" hits=\"{line_hits}\" branch=\"{has_conditions}\">\n"
            ));

            if has_conditions {
                out.push_str("              <conditions>\n");
                for arm in arms {
                    let coverage = if arm.hits > 0 { "100%" } else { "0%" };
                    out.push_str(&format!(
                        "                <condition number=\"{}\" type=\"jump\" coverage=\"{coverage}\"/>\n",
                        arm.arm
                    ));
                }
                out.push_str("              </conditions>\n");
            }

            out.push_str("            </line>\n");
        }

        out.push_str("          </lines>\n");
        out.push_str("        </class>\n");
    }

    out.push_str("      </classes>\n");
    out.push_str("    </package>\n");
    out.push_str("  </packages>\n");
    out.push_str("</coverage>\n");
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_u32(s: &str, label: &str, lineno: usize) -> Result<u32> {
    s.trim().parse::<u32>().map_err(|_| {
        ApexError::Config(format!(
            "LCOV line {}: invalid {label} value: {s}",
            lineno + 1
        ))
    })
}

/// Extract the value of `name="..."` from an XML tag string.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// Convert a Cobertura `coverage` percentage string to a hit count.
///
/// Cobertura stores "0%", "50%", or "100%" per condition arm.  We map this to
/// 0 or 1 (the actual count is not recoverable from the format).
fn coverage_pct_to_hits(s: &str, _arm: u32) -> u32 {
    let trimmed = s.trim_end_matches('%').trim();
    match trimmed.parse::<u32>() {
        Ok(0) => 0,
        Ok(_) => 1,
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- CoverageFormat ----

    #[test]
    fn format_display_roundtrip() {
        for (s, f) in &[
            ("lcov", CoverageFormat::Lcov),
            ("cobertura", CoverageFormat::Cobertura),
            ("jacoco", CoverageFormat::Jacoco),
            ("istanbul", CoverageFormat::Istanbul),
            ("v8", CoverageFormat::V8),
            ("go-profile", CoverageFormat::GoProfile),
            ("simplecov", CoverageFormat::SimpleCov),
        ] {
            assert_eq!(f.to_string(), *s);
            assert_eq!(s.parse::<CoverageFormat>().unwrap(), *f);
        }
    }

    #[test]
    fn format_parse_unknown_is_error() {
        assert!("xml".parse::<CoverageFormat>().is_err());
    }

    // ---- import_lcov ----

    const LCOV_SAMPLE: &str = "\
SF:src/main.rs\n\
DA:1,5\n\
DA:2,0\n\
BRDA:3,0,0,1\n\
BRDA:3,0,1,0\n\
BRDA:5,1,0,-\n\
end_of_record\n\
SF:src/lib.rs\n\
BRDA:10,0,0,3\n\
BRDA:10,0,1,2\n\
end_of_record\n\
";

    #[test]
    fn lcov_parse_branch_data() {
        let records = import_lcov(LCOV_SAMPLE).unwrap();
        assert_eq!(records.len(), 5);

        // First file
        assert_eq!(records[0].file, "src/main.rs");
        assert_eq!(records[0].line, 3);
        assert_eq!(records[0].branch_index, 0);
        assert_eq!(records[0].arm, 0);
        assert_eq!(records[0].hits, 1);

        assert_eq!(records[1].arm, 1);
        assert_eq!(records[1].hits, 0, "explicit 0 taken");

        assert_eq!(records[2].hits, 0, "'-' means not taken");

        // Second file
        assert_eq!(records[3].file, "src/lib.rs");
        assert_eq!(records[3].hits, 3);
        assert_eq!(records[4].hits, 2);
    }

    #[test]
    fn lcov_parse_empty() {
        let records = import_lcov("").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn lcov_parse_da_lines_not_included() {
        // DA lines carry line coverage, not branch coverage — must not appear.
        let data = "SF:src/x.rs\nDA:1,10\nDA:2,0\nend_of_record\n";
        let records = import_lcov(data).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn lcov_parse_malformed_brda_error() {
        let data = "SF:src/x.rs\nBRDA:3,0,0\nend_of_record\n"; // only 3 fields
        assert!(import_lcov(data).is_err());
    }

    // ---- export_lcov ----

    #[test]
    fn lcov_export_contains_sf_and_brda() {
        let branches = vec![
            BranchCoverage::new("src/main.rs", 3, 0, 0, 1),
            BranchCoverage::new("src/main.rs", 3, 0, 1, 0),
        ];
        let lcov = export_lcov(&branches);

        assert!(lcov.contains("SF:src/main.rs"));
        assert!(lcov.contains("BRDA:3,0,0,1"));
        assert!(lcov.contains("BRDA:3,0,1,-"), "zero hits → dash");
        assert!(lcov.contains("BRF:2"));
        assert!(lcov.contains("BRH:1"));
        assert!(lcov.contains("end_of_record"));
    }

    #[test]
    fn lcov_export_empty_input() {
        assert_eq!(export_lcov(&[]), "");
    }

    #[test]
    fn lcov_roundtrip() {
        let original = vec![
            BranchCoverage::new("src/a.rs", 5, 0, 0, 3),
            BranchCoverage::new("src/a.rs", 5, 0, 1, 0),
            BranchCoverage::new("src/b.rs", 12, 1, 0, 7),
        ];
        let lcov_text = export_lcov(&original);
        let parsed = import_lcov(&lcov_text).unwrap();

        assert_eq!(parsed.len(), original.len());
        for (orig, parsed) in original.iter().zip(parsed.iter()) {
            assert_eq!(orig.file, parsed.file);
            assert_eq!(orig.line, parsed.line);
            assert_eq!(orig.branch_index, parsed.branch_index);
            assert_eq!(orig.arm, parsed.arm);
            // Hits: 0 → 0, >0 → same value (export preserves the count).
            if orig.hits == 0 {
                assert_eq!(parsed.hits, 0);
            } else {
                assert_eq!(parsed.hits, orig.hits);
            }
        }
    }

    // ---- import_cobertura ----

    const COBERTURA_SAMPLE: &str = r#"<?xml version="1.0" ?>
<coverage branch-rate="0.75">
  <packages>
    <package name=".">
      <classes>
        <class filename="src/main.rs" name="src/main.rs">
          <lines>
            <line number="10" hits="3" branch="true">
              <conditions>
                <condition number="0" type="jump" coverage="100%"/>
                <condition number="1" type="jump" coverage="0%"/>
              </conditions>
            </line>
            <line number="20" hits="0" branch="true">
              <conditions>
                <condition number="0" type="jump" coverage="0%"/>
                <condition number="1" type="jump" coverage="0%"/>
              </conditions>
            </line>
          </lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>"#;

    #[test]
    fn cobertura_parse_conditions() {
        let records = import_cobertura(COBERTURA_SAMPLE).unwrap();
        assert_eq!(records.len(), 4);

        assert_eq!(records[0].file, "src/main.rs");
        assert_eq!(records[0].line, 10);
        assert_eq!(records[0].arm, 0);
        assert_eq!(records[0].hits, 1, "100% → hit");

        assert_eq!(records[1].arm, 1);
        assert_eq!(records[1].hits, 0, "0% → not hit");

        // Line 20 — both uncovered
        assert_eq!(records[2].line, 20);
        assert_eq!(records[2].hits, 0);
        assert_eq!(records[3].hits, 0);
    }

    #[test]
    fn cobertura_parse_empty() {
        let records = import_cobertura("<coverage></coverage>").unwrap();
        assert!(records.is_empty());
    }

    // ---- export_cobertura ----

    #[test]
    fn cobertura_export_valid_xml_structure() {
        let branches = vec![
            BranchCoverage::new("src/main.rs", 10, 0, 0, 5),
            BranchCoverage::new("src/main.rs", 10, 0, 1, 0),
        ];
        let xml = export_cobertura(&branches);

        assert!(xml.contains("<?xml"));
        assert!(xml.contains("<coverage"));
        assert!(xml.contains("<class filename=\"src/main.rs\""));
        assert!(xml.contains("<line number=\"10\""));
        assert!(xml.contains("<condition number=\"0\""));
        assert!(xml.contains("coverage=\"100%\""));
        assert!(xml.contains("coverage=\"0%\""));
        assert!(xml.contains("</coverage>"));
    }

    #[test]
    fn cobertura_export_branch_rate() {
        // 1 of 2 arms covered → rate 0.5
        let branches = vec![
            BranchCoverage::new("f.rs", 1, 0, 0, 1),
            BranchCoverage::new("f.rs", 1, 0, 1, 0),
        ];
        let xml = export_cobertura(&branches);
        assert!(xml.contains("branch-rate=\"0.5000\""));
    }

    #[test]
    fn cobertura_export_empty_input() {
        let xml = export_cobertura(&[]);
        assert!(xml.contains("<coverage"));
        assert!(xml.contains("branches-valid=\"0\""));
    }
}
