//! SBOM (Software Bill of Materials) generation from lock files.
//!
//! Generates SPDX 2.3 or CycloneDX 1.5 JSON documents from parsed
//! lock-file dependencies.

use apex_cpg::architecture::ImportGraph;
use crate::lockfile::{self, Dependency};
use apex_core::error::Result;
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;

/// Supported SBOM output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SbomFormat {
    Spdx,
    CycloneDx,
}

/// Generates SBOM documents from lock files found in a project directory.
pub struct SbomGenerator;

impl SbomGenerator {
    /// Scan `target` for lock files, parse them, and produce a JSON SBOM.
    pub fn generate(target: &Path, format: SbomFormat) -> Result<Value> {
        let deps = Self::collect_dependencies(target)?;
        let project_name = Self::project_name(target);
        let timestamp = Self::iso_timestamp();

        match format {
            SbomFormat::Spdx => Ok(Self::build_spdx(&project_name, &timestamp, &deps)),
            SbomFormat::CycloneDx => Ok(Self::build_cyclonedx(&timestamp, &deps)),
        }
    }

    /// Collect dependencies from all supported lock files in `target`.
    fn collect_dependencies(target: &Path) -> Result<Vec<Dependency>> {
        let mut all_deps = Vec::new();

        let cargo_lock = target.join("Cargo.lock");
        if cargo_lock.exists() {
            match lockfile::parse_cargo_lock(&cargo_lock) {
                Ok(deps) => all_deps.extend(deps),
                Err(e) => {
                    tracing::warn!("failed to parse Cargo.lock: {e}");
                }
            }
        }

        let package_lock = target.join("package-lock.json");
        if package_lock.exists() {
            match lockfile::parse_package_lock(&package_lock) {
                Ok(deps) => all_deps.extend(deps),
                Err(e) => {
                    tracing::warn!("failed to parse package-lock.json: {e}");
                }
            }
        }

        let requirements = target.join("requirements.txt");
        if requirements.exists() {
            match lockfile::parse_requirements(&requirements) {
                Ok(deps) => all_deps.extend(deps),
                Err(e) => {
                    tracing::warn!("failed to parse requirements.txt: {e}");
                }
            }
        }

        Ok(all_deps)
    }

    /// Derive a project name from the directory basename.
    fn project_name(target: &Path) -> String {
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown-project")
            .to_string()
    }

    /// Generate a simple ISO-8601 UTC timestamp without pulling in chrono.
    fn iso_timestamp() -> String {
        // Use std::time to get seconds since epoch, then format manually.
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();

        // Convert epoch seconds to ISO-8601 date-time.
        // Algorithm from Howard Hinnant's `days_from_civil`.
        let days = (secs / 86400) as i64;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        // Civil date from days since 1970-01-01.
        let z = days + 719468;
        let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = (yoe as i64) + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };

        format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
    }

    /// Sanitize a string for use in an SPDX identifier (alphanumeric, dash, dot).
    fn spdx_id(name: &str, version: &str) -> String {
        let sanitize = |s: &str| -> String {
            s.chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '.' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect()
        };
        format!("SPDXRef-Package-{}-{}", sanitize(name), sanitize(version))
    }

    fn build_spdx(project_name: &str, timestamp: &str, deps: &[Dependency]) -> Value {
        let doc_uuid = Uuid::new_v4();
        let namespace = format!("https://spdx.org/spdxdocs/{project_name}-{doc_uuid}");

        let packages: Vec<Value> = deps
            .iter()
            .map(|dep| {
                let spdx_id = Self::spdx_id(&dep.name, &dep.version);
                let download = dep
                    .source_url
                    .as_deref()
                    .unwrap_or("NOASSERTION")
                    .to_string();
                let license = dep.license.as_deref().unwrap_or("NOASSERTION").to_string();

                let mut pkg = json!({
                    "SPDXID": spdx_id,
                    "name": dep.name,
                    "versionInfo": dep.version,
                    "downloadLocation": download,
                    "externalRefs": [{
                        "referenceCategory": "PACKAGE-MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": dep.purl
                    }],
                    "licenseDeclared": license
                });

                if let Some(hash) = &dep.checksum {
                    pkg["checksums"] = json!([{
                        "algorithm": "SHA256",
                        "checksumValue": hash
                    }]);
                }

                pkg
            })
            .collect();

        let relationships: Vec<Value> = deps
            .iter()
            .map(|dep| {
                json!({
                    "spdxElementId": "SPDXRef-DOCUMENT",
                    "relationshipType": "DESCRIBES",
                    "relatedSpdxElement": Self::spdx_id(&dep.name, &dep.version)
                })
            })
            .collect();

        json!({
            "spdxVersion": "SPDX-2.3",
            "dataLicense": "CC0-1.0",
            "SPDXID": "SPDXRef-DOCUMENT",
            "name": project_name,
            "documentNamespace": namespace,
            "creationInfo": {
                "created": timestamp,
                "creators": ["Tool: apex"],
                "licenseListVersion": "3.22"
            },
            "packages": packages,
            "relationships": relationships
        })
    }

    fn build_cyclonedx(timestamp: &str, deps: &[Dependency]) -> Value {
        let components: Vec<Value> = deps
            .iter()
            .map(|dep| {
                let mut comp = json!({
                    "type": "library",
                    "name": dep.name,
                    "version": dep.version,
                    "purl": dep.purl
                });

                if let Some(license) = &dep.license {
                    comp["licenses"] = json!([{
                        "license": { "id": license }
                    }]);
                }

                if let Some(hash) = &dep.checksum {
                    comp["hashes"] = json!([{
                        "alg": "SHA-256",
                        "content": hash
                    }]);
                }

                comp
            })
            .collect();

        json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "version": 1,
            "metadata": {
                "timestamp": timestamp,
                "tools": [{"name": "apex", "version": "0.1.0"}]
            },
            "components": components
        })
    }
}

/// Reachability annotation for an SBOM component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityAnnotation {
    /// The dependency name.
    pub name: String,
    /// Whether this dependency is actually imported/used in the codebase.
    pub imported: bool,
    /// Whether a known vulnerability in this dep is reachable from user-facing code.
    pub vuln_reachable: bool,
}

impl SbomGenerator {
    /// Annotate dependencies with reachability information using the import graph.
    ///
    /// A dependency is "imported" if any module in the project imports it
    /// (by checking if the dependency name appears as a prefix of any import target).
    pub fn annotate_reachability(
        deps: &[Dependency],
        graph: &ImportGraph,
    ) -> Vec<ReachabilityAnnotation> {
        deps.iter()
            .map(|dep| {
                let dep_name_normalized = dep.name.replace('-', "_");

                // Check if any edge in the graph imports this dependency
                let imported = graph.all_edges().iter().any(|edge| {
                    edge.to.starts_with(&dep_name_normalized)
                        || edge.to == dep_name_normalized
                });

                ReachabilityAnnotation {
                    name: dep.name.clone(),
                    imported,
                    vuln_reachable: imported, // conservative: if imported, assume reachable
                }
            })
            .collect()
    }

    /// Enrich an SPDX SBOM with reachability properties.
    pub fn enrich_spdx_with_reachability(
        mut sbom: Value,
        annotations: &[ReachabilityAnnotation],
    ) -> Value {
        if let Some(packages) = sbom.get_mut("packages").and_then(|v| v.as_array_mut()) {
            for pkg in packages {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    if let Some(ann) = annotations.iter().find(|a| a.name == name) {
                        if let Some(obj) = pkg.as_object_mut() {
                            let annotations_arr = obj
                                .entry("annotations")
                                .or_insert_with(|| json!([]));
                            if let Some(arr) = annotations_arr.as_array_mut() {
                                arr.push(json!({
                                    "annotationType": "REVIEW",
                                    "comment": format!(
                                        "apex:imported={}, apex:vuln-reachable={}",
                                        ann.imported, ann.vuln_reachable
                                    )
                                }));
                            }
                        }
                    }
                }
            }
        }
        sbom
    }

    /// Enrich a CycloneDX SBOM with reachability properties.
    pub fn enrich_cyclonedx_with_reachability(
        mut sbom: Value,
        annotations: &[ReachabilityAnnotation],
    ) -> Value {
        if let Some(components) = sbom.get_mut("components").and_then(|v| v.as_array_mut()) {
            for comp in components {
                if let Some(name) = comp.get("name").and_then(|v| v.as_str()) {
                    if let Some(ann) = annotations.iter().find(|a| a.name == name) {
                        if let Some(obj) = comp.as_object_mut() {
                            let props = obj
                                .entry("properties")
                                .or_insert_with(|| json!([]));
                            if let Some(arr) = props.as_array_mut() {
                                arr.push(json!({"name": "apex:imported", "value": ann.imported.to_string()}));
                                arr.push(json!({"name": "apex:vuln-reachable", "value": ann.vuln_reachable.to_string()}));
                            }
                        }
                    }
                }
            }
        }
        sbom
    }

    /// Generate a VEX (Vulnerability Exploitability Exchange) statement
    /// for non-reachable vulnerabilities.
    pub fn generate_vex(annotations: &[ReachabilityAnnotation]) -> Value {
        let statements: Vec<Value> = annotations
            .iter()
            .filter(|a| !a.vuln_reachable)
            .map(|a| {
                json!({
                    "vulnerability": { "name": format!("any-vuln-in-{}", a.name) },
                    "status": "not_affected",
                    "justification": "code_not_reachable",
                    "impact_statement": format!(
                        "Dependency '{}' is not imported by any project module (apex:imported=false)",
                        a.name
                    )
                })
            })
            .collect();

        json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "vulnerabilities": statements
        })
    }
}

// ── Internal helpers for testing ──────────────────────────────────────────────

/// Build an SPDX SBOM directly from a list of dependencies (for testing).
#[cfg(test)]
fn generate_spdx_from_deps(project_name: &str, deps: &[Dependency]) -> Value {
    let timestamp = "2026-03-14T00:00:00Z";
    SbomGenerator::build_spdx(project_name, timestamp, deps)
}

/// Build a CycloneDX SBOM directly from a list of dependencies (for testing).
#[cfg(test)]
fn generate_cyclonedx_from_deps(deps: &[Dependency]) -> Value {
    let timestamp = "2026-03-14T00:00:00Z";
    SbomGenerator::build_cyclonedx(timestamp, deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::Dependency;

    fn mock_cargo_deps() -> Vec<Dependency> {
        vec![
            Dependency {
                name: "serde".into(),
                version: "1.0.200".into(),
                purl: "pkg:cargo/serde@1.0.200".into(),
                source_url: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
                checksum: Some("abc123".into()),
                license: Some("MIT OR Apache-2.0".into()),
            },
            Dependency {
                name: "tokio".into(),
                version: "1.37.0".into(),
                purl: "pkg:cargo/tokio@1.37.0".into(),
                source_url: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
                checksum: None,
                license: Some("MIT".into()),
            },
        ]
    }

    fn mock_npm_deps() -> Vec<Dependency> {
        vec![Dependency {
            name: "express".into(),
            version: "4.18.2".into(),
            purl: "pkg:npm/express@4.18.2".into(),
            source_url: Some("https://registry.npmjs.org/express/-/express-4.18.2.tgz".into()),
            checksum: Some("sha512-abc".into()),
            license: Some("MIT".into()),
        }]
    }

    fn mock_pypi_deps() -> Vec<Dependency> {
        vec![Dependency {
            name: "requests".into(),
            version: "2.31.0".into(),
            purl: "pkg:pypi/requests@2.31.0".into(),
            source_url: None,
            checksum: None,
            license: None,
        }]
    }

    // ── SPDX tests ────────────────────────────────────────────────────────────

    #[test]
    fn spdx_output_from_cargo_deps() {
        let deps = mock_cargo_deps();
        let sbom = generate_spdx_from_deps("my-project", &deps);

        assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
        assert_eq!(sbom["dataLicense"], "CC0-1.0");
        assert_eq!(sbom["SPDXID"], "SPDXRef-DOCUMENT");
        assert_eq!(sbom["name"], "my-project");
        assert!(sbom["documentNamespace"]
            .as_str()
            .unwrap()
            .starts_with("https://spdx.org/spdxdocs/my-project-"));
        assert_eq!(sbom["creationInfo"]["creators"][0], "Tool: apex");
        assert_eq!(sbom["creationInfo"]["licenseListVersion"], "3.22");

        let packages = sbom["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 2);

        // First package: serde (has checksum)
        let serde_pkg = &packages[0];
        assert_eq!(serde_pkg["SPDXID"], "SPDXRef-Package-serde-1.0.200");
        assert_eq!(serde_pkg["name"], "serde");
        assert_eq!(serde_pkg["versionInfo"], "1.0.200");
        assert_eq!(serde_pkg["licenseDeclared"], "MIT OR Apache-2.0");
        assert_eq!(
            serde_pkg["externalRefs"][0]["referenceLocator"],
            "pkg:cargo/serde@1.0.200"
        );
        assert_eq!(serde_pkg["checksums"][0]["algorithm"], "SHA256");
        assert_eq!(serde_pkg["checksums"][0]["checksumValue"], "abc123");

        // Second package: tokio (no checksum)
        let tokio_pkg = &packages[1];
        assert_eq!(tokio_pkg["SPDXID"], "SPDXRef-Package-tokio-1.37.0");
        assert!(tokio_pkg.get("checksums").is_none());

        // Relationships
        let rels = sbom["relationships"].as_array().unwrap();
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0]["spdxElementId"], "SPDXRef-DOCUMENT");
        assert_eq!(rels[0]["relationshipType"], "DESCRIBES");
    }

    // ── CycloneDX tests ───────────────────────────────────────────────────────

    #[test]
    fn cyclonedx_output_from_cargo_deps() {
        let deps = mock_cargo_deps();
        let sbom = generate_cyclonedx_from_deps(&deps);

        assert_eq!(sbom["bomFormat"], "CycloneDX");
        assert_eq!(sbom["specVersion"], "1.5");
        assert_eq!(sbom["version"], 1);
        assert_eq!(sbom["metadata"]["tools"][0]["name"], "apex");
        assert_eq!(sbom["metadata"]["tools"][0]["version"], "0.1.0");

        let components = sbom["components"].as_array().unwrap();
        assert_eq!(components.len(), 2);

        let serde_comp = &components[0];
        assert_eq!(serde_comp["type"], "library");
        assert_eq!(serde_comp["name"], "serde");
        assert_eq!(serde_comp["version"], "1.0.200");
        assert_eq!(serde_comp["purl"], "pkg:cargo/serde@1.0.200");
        assert_eq!(
            serde_comp["licenses"][0]["license"]["id"],
            "MIT OR Apache-2.0"
        );
        assert_eq!(serde_comp["hashes"][0]["alg"], "SHA-256");
        assert_eq!(serde_comp["hashes"][0]["content"], "abc123");

        // tokio: no checksum, no hashes field
        let tokio_comp = &components[1];
        assert!(tokio_comp.get("hashes").is_none());
    }

    // ── PURL correctness ──────────────────────────────────────────────────────

    #[test]
    fn purl_format_correctness() {
        let cargo_deps = mock_cargo_deps();
        let npm_deps = mock_npm_deps();
        let pypi_deps = mock_pypi_deps();

        // Cargo PURLs
        assert!(cargo_deps[0].purl.starts_with("pkg:cargo/"));
        assert_eq!(cargo_deps[0].purl, "pkg:cargo/serde@1.0.200");

        // npm PURLs
        assert!(npm_deps[0].purl.starts_with("pkg:npm/"));
        assert_eq!(npm_deps[0].purl, "pkg:npm/express@4.18.2");

        // PyPI PURLs
        assert!(pypi_deps[0].purl.starts_with("pkg:pypi/"));
        assert_eq!(pypi_deps[0].purl, "pkg:pypi/requests@2.31.0");
    }

    // ── Multiple lockfile types combined ──────────────────────────────────────

    #[test]
    fn multiple_lockfile_types_combined() {
        let mut all_deps = Vec::new();
        all_deps.extend(mock_cargo_deps());
        all_deps.extend(mock_npm_deps());
        all_deps.extend(mock_pypi_deps());

        // SPDX
        let spdx = generate_spdx_from_deps("multi-project", &all_deps);
        let spdx_packages = spdx["packages"].as_array().unwrap();
        assert_eq!(spdx_packages.len(), 4); // 2 cargo + 1 npm + 1 pypi

        // Verify all ecosystems present
        let purls: Vec<&str> = spdx_packages
            .iter()
            .map(|p| p["externalRefs"][0]["referenceLocator"].as_str().unwrap())
            .collect();
        assert!(purls.iter().any(|p| p.starts_with("pkg:cargo/")));
        assert!(purls.iter().any(|p| p.starts_with("pkg:npm/")));
        assert!(purls.iter().any(|p| p.starts_with("pkg:pypi/")));

        // CycloneDX
        let cdx = generate_cyclonedx_from_deps(&all_deps);
        let cdx_components = cdx["components"].as_array().unwrap();
        assert_eq!(cdx_components.len(), 4);
    }

    // ── Empty project ─────────────────────────────────────────────────────────

    #[test]
    fn empty_project_produces_valid_spdx() {
        let deps: Vec<Dependency> = vec![];
        let sbom = generate_spdx_from_deps("empty", &deps);

        assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
        assert_eq!(sbom["name"], "empty");
        assert_eq!(sbom["packages"].as_array().unwrap().len(), 0);
        assert_eq!(sbom["relationships"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn empty_project_produces_valid_cyclonedx() {
        let deps: Vec<Dependency> = vec![];
        let sbom = generate_cyclonedx_from_deps(&deps);

        assert_eq!(sbom["bomFormat"], "CycloneDX");
        assert_eq!(sbom["specVersion"], "1.5");
        assert_eq!(sbom["components"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn empty_project_no_lockfiles() {
        let dir = tempfile::tempdir().unwrap();
        let sbom = SbomGenerator::generate(dir.path(), SbomFormat::Spdx).unwrap();
        assert_eq!(sbom["packages"].as_array().unwrap().len(), 0);

        let sbom = SbomGenerator::generate(dir.path(), SbomFormat::CycloneDx).unwrap();
        assert_eq!(sbom["components"].as_array().unwrap().len(), 0);
    }

    // ── SPDX ID sanitization ─────────────────────────────────────────────────

    #[test]
    fn spdx_id_sanitizes_special_chars() {
        // Scoped npm packages have @ and / which are not valid in SPDX IDs
        let id = SbomGenerator::spdx_id("@scope/pkg", "2.0.0");
        assert_eq!(id, "SPDXRef-Package--scope-pkg-2.0.0");
        // No invalid chars (only alphanumeric, dash, dot allowed after SPDXRef-)
        assert!(id
            .strip_prefix("SPDXRef-")
            .unwrap()
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '.'));
    }

    // ── Optional fields ──────────────────────────────────────────────────────

    #[test]
    fn spdx_no_license_shows_noassertion() {
        let deps = mock_pypi_deps(); // requests has no license
        let sbom = generate_spdx_from_deps("test", &deps);
        let pkg = &sbom["packages"][0];
        assert_eq!(pkg["licenseDeclared"], "NOASSERTION");
    }

    #[test]
    fn spdx_no_source_url_shows_noassertion() {
        let deps = mock_pypi_deps(); // requests has no source_url
        let sbom = generate_spdx_from_deps("test", &deps);
        let pkg = &sbom["packages"][0];
        assert_eq!(pkg["downloadLocation"], "NOASSERTION");
    }

    #[test]
    fn cyclonedx_omits_license_when_absent() {
        let deps = mock_pypi_deps();
        let sbom = generate_cyclonedx_from_deps(&deps);
        let comp = &sbom["components"][0];
        assert!(comp.get("licenses").is_none());
    }

    #[test]
    fn cyclonedx_omits_hashes_when_absent() {
        let deps = mock_pypi_deps();
        let sbom = generate_cyclonedx_from_deps(&deps);
        let comp = &sbom["components"][0];
        assert!(comp.get("hashes").is_none());
    }

    // ── Filesystem integration ────────────────────────────────────────────────

    #[test]
    fn generate_from_cargo_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.lock"),
            r#"
[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc123"
"#,
        )
        .unwrap();

        let sbom = SbomGenerator::generate(dir.path(), SbomFormat::Spdx).unwrap();
        let packages = sbom["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0]["name"], "serde");
    }

    #[test]
    fn generate_from_multiple_lockfiles() {
        let dir = tempfile::tempdir().unwrap();

        // Cargo.lock
        std::fs::write(
            dir.path().join("Cargo.lock"),
            "[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\n",
        )
        .unwrap();

        // requirements.txt
        std::fs::write(dir.path().join("requirements.txt"), "requests==2.31.0\n").unwrap();

        let sbom = SbomGenerator::generate(dir.path(), SbomFormat::CycloneDx).unwrap();
        let components = sbom["components"].as_array().unwrap();
        assert_eq!(components.len(), 2);

        let names: Vec<&str> = components
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"serde"));
        assert!(names.contains(&"requests"));
    }

    // ── Reachability SBOM tests ─────────────────────────────────────────────

    use apex_cpg::architecture::{ImportEdge, ImportGraph};

    fn make_dep(name: &str) -> Dependency {
        Dependency {
            name: name.into(),
            version: "1.0.0".into(),
            purl: format!("pkg:pypi/{name}@1.0.0"),
            source_url: None,
            checksum: None,
            license: None,
        }
    }

    #[test]
    fn annotate_reachability_imported_dep() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "app".into(),
            to: "requests".into(),
            line: 1,
        });
        let deps = vec![make_dep("requests")];
        let anns = SbomGenerator::annotate_reachability(&deps, &graph);
        assert_eq!(anns.len(), 1);
        assert!(anns[0].imported);
        assert!(anns[0].vuln_reachable);
    }

    #[test]
    fn annotate_reachability_not_imported() {
        let graph = ImportGraph::new();
        let deps = vec![make_dep("unused_lib")];
        let anns = SbomGenerator::annotate_reachability(&deps, &graph);
        assert_eq!(anns.len(), 1);
        assert!(!anns[0].imported);
        assert!(!anns[0].vuln_reachable);
    }

    #[test]
    fn annotate_reachability_normalizes_hyphens() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "app".into(),
            to: "my_dep".into(),
            line: 1,
        });
        let deps = vec![make_dep("my-dep")];
        let anns = SbomGenerator::annotate_reachability(&deps, &graph);
        assert!(anns[0].imported);
    }

    #[test]
    fn annotate_reachability_empty_graph() {
        let graph = ImportGraph::new();
        let deps = vec![make_dep("foo"), make_dep("bar")];
        let anns = SbomGenerator::annotate_reachability(&deps, &graph);
        assert!(anns.iter().all(|a| !a.imported));
    }

    #[test]
    fn enrich_spdx_adds_annotations() {
        let deps = vec![make_dep("requests")];
        let sbom = generate_spdx_from_deps("test", &deps);
        let anns = vec![ReachabilityAnnotation {
            name: "requests".into(),
            imported: true,
            vuln_reachable: true,
        }];
        let enriched = SbomGenerator::enrich_spdx_with_reachability(sbom, &anns);
        let pkg = &enriched["packages"][0];
        let ann_arr = pkg["annotations"].as_array().unwrap();
        assert_eq!(ann_arr.len(), 1);
        assert_eq!(ann_arr[0]["annotationType"], "REVIEW");
        let comment = ann_arr[0]["comment"].as_str().unwrap();
        assert!(comment.contains("apex:imported=true"));
        assert!(comment.contains("apex:vuln-reachable=true"));
    }

    #[test]
    fn enrich_spdx_no_packages_noop() {
        let sbom = json!({"spdxVersion": "SPDX-2.3"});
        let anns = vec![ReachabilityAnnotation {
            name: "foo".into(),
            imported: true,
            vuln_reachable: true,
        }];
        let result = SbomGenerator::enrich_spdx_with_reachability(sbom.clone(), &anns);
        assert_eq!(result, sbom);
    }

    #[test]
    fn enrich_cyclonedx_adds_properties() {
        let deps = vec![make_dep("requests")];
        let sbom = generate_cyclonedx_from_deps(&deps);
        let anns = vec![ReachabilityAnnotation {
            name: "requests".into(),
            imported: true,
            vuln_reachable: true,
        }];
        let enriched = SbomGenerator::enrich_cyclonedx_with_reachability(sbom, &anns);
        let comp = &enriched["components"][0];
        let props = comp["properties"].as_array().unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0]["name"], "apex:imported");
        assert_eq!(props[0]["value"], "true");
        assert_eq!(props[1]["name"], "apex:vuln-reachable");
        assert_eq!(props[1]["value"], "true");
    }

    #[test]
    fn enrich_cyclonedx_no_components_noop() {
        let sbom = json!({"bomFormat": "CycloneDX", "specVersion": "1.5"});
        let anns = vec![ReachabilityAnnotation {
            name: "foo".into(),
            imported: true,
            vuln_reachable: true,
        }];
        let result = SbomGenerator::enrich_cyclonedx_with_reachability(sbom.clone(), &anns);
        assert_eq!(result, sbom);
    }

    #[test]
    fn generate_vex_for_non_reachable() {
        let anns = vec![
            ReachabilityAnnotation {
                name: "used_lib".into(),
                imported: true,
                vuln_reachable: true,
            },
            ReachabilityAnnotation {
                name: "unused_lib".into(),
                imported: false,
                vuln_reachable: false,
            },
        ];
        let vex = SbomGenerator::generate_vex(&anns);
        let vulns = vex["vulnerabilities"].as_array().unwrap();
        assert_eq!(vulns.len(), 1);
        assert_eq!(vulns[0]["vulnerability"]["name"], "any-vuln-in-unused_lib");
        assert_eq!(vulns[0]["status"], "not_affected");
    }

    #[test]
    fn generate_vex_empty_when_all_reachable() {
        let anns = vec![ReachabilityAnnotation {
            name: "used".into(),
            imported: true,
            vuln_reachable: true,
        }];
        let vex = SbomGenerator::generate_vex(&anns);
        let vulns = vex["vulnerabilities"].as_array().unwrap();
        assert!(vulns.is_empty());
    }

    #[test]
    fn annotate_reachability_multiple_deps() {
        let mut graph = ImportGraph::new();
        graph.add_edge(ImportEdge {
            from: "app".into(),
            to: "requests".into(),
            line: 1,
        });
        let deps = vec![
            make_dep("requests"),
            make_dep("unused_a"),
            make_dep("unused_b"),
        ];
        let anns = SbomGenerator::annotate_reachability(&deps, &graph);
        assert_eq!(anns.len(), 3);
        assert!(anns[0].imported);
        assert!(!anns[1].imported);
        assert!(!anns[2].imported);
    }

    #[test]
    fn vex_justification_is_code_not_reachable() {
        let anns = vec![ReachabilityAnnotation {
            name: "dormant".into(),
            imported: false,
            vuln_reachable: false,
        }];
        let vex = SbomGenerator::generate_vex(&anns);
        let vulns = vex["vulnerabilities"].as_array().unwrap();
        assert_eq!(vulns[0]["justification"], "code_not_reachable");
        let impact = vulns[0]["impact_statement"].as_str().unwrap();
        assert!(impact.contains("dormant"));
        assert!(impact.contains("not imported"));
    }
}
