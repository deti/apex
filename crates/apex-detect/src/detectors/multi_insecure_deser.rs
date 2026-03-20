//! Multi-language insecure deserialization detector (CWE-502).
//!
//! Detects unsafe deserialization patterns across all supported languages:
//! pickle, marshal, yaml.load, ObjectInputStream, BinaryFormatter, Marshal.load,
//! gob.Decode, eval/Function construction, NSKeyedUnarchiver, and more.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MultiInsecureDeserDetector;

struct DeserPattern {
    name: &'static str,
    regex: &'static str,
    /// Lines matching any of these strings are considered safe — skip finding.
    safe_indicators: &'static [&'static str],
    description: &'static str,
    suggestion: &'static str,
    severity: Severity,
}

fn patterns_for(lang: Language) -> &'static [DeserPattern] {
    match lang {
        Language::Python => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "pickle.loads",
                    regex: r"pickle\.loads?\s*\(",
                    safe_indicators: &[],
                    description: "pickle deserialization can execute arbitrary code",
                    suggestion: "Use JSON or a safe serialization format for untrusted data",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "marshal.loads",
                    regex: r"marshal\.loads?\s*\(",
                    safe_indicators: &[],
                    description: "marshal deserialization can execute arbitrary code",
                    suggestion: "Use JSON for untrusted data; marshal is not secure",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "yaml.load",
                    regex: r"yaml\.load\s*\(",
                    safe_indicators: &["safe_load", "SafeLoader", "yaml.safe_load"],
                    description: "yaml.load without SafeLoader can execute arbitrary code",
                    suggestion: "Use yaml.safe_load() or yaml.load(data, Loader=SafeLoader)",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "shelve.open",
                    regex: r"shelve\.open\s*\(",
                    safe_indicators: &[],
                    description: "shelve uses pickle internally — unsafe for untrusted data",
                    suggestion: "Use a database or JSON for untrusted data",
                    severity: Severity::Medium,
                },
            ];
            P
        }
        Language::JavaScript => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "yaml.load (JS)",
                    regex: r"yaml\.load\s*\(",
                    safe_indicators: &["yaml.safeLoad", "yaml.SAFE_SCHEMA", "safe_load"],
                    description: "yaml.load() without safe schema can execute arbitrary code",
                    suggestion: "Use yaml.safeLoad or yaml.load with SAFE_SCHEMA",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "eval(JSON.parse(...))",
                    regex: r"eval\s*\(\s*JSON\.parse\s*\(",
                    safe_indicators: &[],
                    description: "eval(JSON.parse(...)) — code execution via deserialized JSON",
                    suggestion: "Use JSON.parse() directly without eval",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "new Function",
                    regex: r"new\s+Function\s*\(",
                    safe_indicators: &[],
                    description: "new Function() with dynamic argument is equivalent to eval",
                    suggestion: "Avoid dynamic code generation from untrusted input",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "serialize unsafe",
                    regex: r"serialize\s*\([^)]*unsafe\s*:\s*true",
                    safe_indicators: &[],
                    description: "serialize-javascript with unsafe: true allows arbitrary code",
                    suggestion: "Remove unsafe: true from serialize options",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "node-serialize",
                    regex: r"(?:require|import).*node-serialize",
                    safe_indicators: &[],
                    description: "node-serialize is known to enable remote code execution",
                    suggestion: "Use JSON.stringify/JSON.parse instead of node-serialize",
                    severity: Severity::High,
                },
            ];
            P
        }
        Language::Java | Language::Kotlin => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "ObjectInputStream.readObject",
                    regex: r"\.readObject\s*\(",
                    safe_indicators: &["ObjectInputFilter", "resolveClass", "lookAheadObjectInputStream"],
                    description: "ObjectInputStream.readObject() deserializes arbitrary objects",
                    suggestion: "Use ObjectInputFilter or a type-safe serialization format",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "XMLDecoder",
                    regex: r"XMLDecoder\s*\(",
                    safe_indicators: &[],
                    description: "XMLDecoder can instantiate arbitrary objects",
                    suggestion: "Use a safe XML parser (JAXB with known types)",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "XStream.fromXML",
                    regex: r"XStream.*\.fromXML\s*\(",
                    safe_indicators: &["allowTypes", "XStream.setupDefaultSecurity"],
                    description: "XStream.fromXML can deserialize arbitrary objects",
                    suggestion: "Configure XStream allowTypes or use a safe format",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "Kryo.readObject",
                    regex: r"(?:kryo|Kryo)\.read(?:Object|ClassAndObject)\s*\(",
                    safe_indicators: &["setRegistrationRequired"],
                    description: "Kryo deserialization without registration can be exploited",
                    suggestion: "Use kryo.setRegistrationRequired(true)",
                    severity: Severity::Medium,
                },
            ];
            P
        }
        Language::Go => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "gob.Decode",
                    regex: r"(?:gob\.NewDecoder|\.Decode)\s*\(",
                    safe_indicators: &[],
                    description: "encoding/gob decoding from untrusted input can be exploited",
                    suggestion: "Validate and sanitize input; prefer JSON with typed structs",
                    severity: Severity::Medium,
                },
                DeserPattern {
                    name: "json.Unmarshal into interface{}",
                    regex: r"json\.Unmarshal\s*\([^,]+,\s*&?\s*interface\s*\{\}",
                    safe_indicators: &[],
                    description: "json.Unmarshal into interface{} loses type safety",
                    suggestion: "Unmarshal into a typed struct instead of interface{}",
                    severity: Severity::Low,
                },
            ];
            P
        }
        Language::Ruby => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "Marshal.load",
                    regex: r"Marshal\.load\s*\(",
                    safe_indicators: &[],
                    description: "Marshal.load can execute arbitrary code",
                    suggestion: "Use JSON.parse or YAML.safe_load for untrusted data",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "YAML.load",
                    regex: r"YAML\.load\s*\(",
                    safe_indicators: &["safe_load", "permitted_classes", "safe_load_file"],
                    description: "YAML.load can instantiate arbitrary Ruby objects",
                    suggestion: "Use YAML.safe_load instead",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "Oj.load",
                    regex: r"Oj\.load\s*\(",
                    safe_indicators: &["mode: :strict", "mode: :compat", "Oj::Rails"],
                    description: "Oj.load in object mode can instantiate arbitrary objects",
                    suggestion: "Use Oj.load with mode: :strict or JSON.parse",
                    severity: Severity::Medium,
                },
                DeserPattern {
                    name: "eval",
                    regex: r"\beval\s*\(",
                    safe_indicators: &[],
                    description: "eval() executes arbitrary code",
                    suggestion: "Avoid eval; use safe parsing or deserialization",
                    severity: Severity::High,
                },
            ];
            P
        }
        Language::CSharp => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "BinaryFormatter.Deserialize",
                    regex: r"BinaryFormatter.*\.Deserialize\s*\(",
                    safe_indicators: &[],
                    description: "BinaryFormatter.Deserialize is inherently unsafe (SYSLIB0011)",
                    suggestion: "Use System.Text.Json or XmlSerializer with known types",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "SoapFormatter.Deserialize",
                    regex: r"SoapFormatter.*\.Deserialize\s*\(",
                    safe_indicators: &[],
                    description: "SoapFormatter.Deserialize is inherently unsafe",
                    suggestion: "Use System.Text.Json or XmlSerializer with known types",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "ObjectStateFormatter",
                    regex: r"ObjectStateFormatter.*\.Deserialize\s*\(",
                    safe_indicators: &[],
                    description: "ObjectStateFormatter can deserialize arbitrary objects",
                    suggestion: "Use a type-safe serialization format",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "LosFormatter",
                    regex: r"LosFormatter.*\.Deserialize\s*\(",
                    safe_indicators: &[],
                    description: "LosFormatter can deserialize arbitrary objects",
                    suggestion: "Use a type-safe serialization format",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "NetDataContractSerializer",
                    regex: r"NetDataContractSerializer.*\.(?:Deserialize|ReadObject)\s*\(",
                    safe_indicators: &[],
                    description: "NetDataContractSerializer can deserialize arbitrary types",
                    suggestion: "Use DataContractSerializer with known types or System.Text.Json",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "JsonConvert TypeNameHandling",
                    regex: r"TypeNameHandling\s*[=:]\s*TypeNameHandling\.(?:All|Auto|Objects|Arrays)",
                    safe_indicators: &["SerializationBinder", "ISerializationBinder"],
                    description: "TypeNameHandling != None enables arbitrary type instantiation",
                    suggestion: "Use TypeNameHandling.None or set a SerializationBinder",
                    severity: Severity::High,
                },
            ];
            P
        }
        Language::Swift => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "NSKeyedUnarchiver.unarchiveObject",
                    regex: r"NSKeyedUnarchiver\.unarchiveObject\s*\(",
                    safe_indicators: &["unarchivedObject(ofClass", "unarchivedObject(ofClasses"],
                    description: "NSKeyedUnarchiver.unarchiveObject is deprecated and unsafe",
                    suggestion: "Use unarchivedObject(ofClass:from:) for type-safe unarchiving",
                    severity: Severity::High,
                },
                DeserPattern {
                    name: "NSUnarchiver",
                    regex: r"NSUnarchiver\b",
                    safe_indicators: &[],
                    description: "NSUnarchiver is deprecated and does not validate types",
                    suggestion: "Use NSKeyedUnarchiver with unarchivedObject(ofClass:from:)",
                    severity: Severity::Medium,
                },
            ];
            P
        }
        Language::C | Language::Cpp => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "unserialize (C/C++)",
                    regex: r"\bunserialize\s*\(",
                    safe_indicators: &[],
                    description: "Custom unserialize function may be unsafe with untrusted data",
                    suggestion: "Validate input format and use typed parsing",
                    severity: Severity::Medium,
                },
            ];
            P
        }
        Language::Rust => {
            static P: &[DeserPattern] = &[
                DeserPattern {
                    name: "bincode::deserialize",
                    regex: r"bincode::deserialize\s*[(<]",
                    safe_indicators: &["options().with_limit", "DefaultOptions"],
                    description: "bincode::deserialize from untrusted input can cause issues",
                    suggestion: "Use bincode with size limits: options().with_limit().deserialize()",
                    severity: Severity::Low,
                },
            ];
            P
        }
        Language::Wasm => &[],
    }
}

struct CompiledPatterns {
    entries: Vec<(&'static DeserPattern, Regex)>,
}

static COMPILED: LazyLock<Vec<(Language, CompiledPatterns)>> = LazyLock::new(|| {
    use Language::*;
    let langs = [
        Python,
        JavaScript,
        Java,
        Kotlin,
        Go,
        Ruby,
        CSharp,
        Swift,
        C,
        Cpp,
        Rust,
    ];
    langs
        .iter()
        .map(|&lang| {
            let pats = patterns_for(lang);
            let entries = pats
                .iter()
                .map(|p| {
                    let re = Regex::new(p.regex).unwrap_or_else(|e| {
                        panic!("invalid deser regex '{}': {}", p.regex, e)
                    });
                    (p, re)
                })
                .collect();
            (lang, CompiledPatterns { entries })
        })
        .collect()
});

fn compiled_for(lang: Language) -> &'static CompiledPatterns {
    COMPILED
        .iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, c)| c)
        .expect("language not in compiled list")
}

fn is_supported(lang: Language) -> bool {
    !matches!(lang, Language::Wasm)
}

#[async_trait]
impl Detector for MultiInsecureDeserDetector {
    fn name(&self) -> &str {
        "multi-insecure-deser"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if !is_supported(ctx.language) {
            return Ok(Vec::new());
        }

        let compiled = compiled_for(ctx.language);
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if trimmed.is_empty() || is_comment(trimmed, ctx.language) {
                    continue;
                }

                if in_test_block(source, line_num) {
                    continue;
                }

                for (pattern, regex) in &compiled.entries {
                    if !regex.is_match(trimmed) {
                        continue;
                    }

                    // Check safe indicators
                    let is_safe = pattern
                        .safe_indicators
                        .iter()
                        .any(|ind| trimmed.contains(ind));
                    if is_safe {
                        continue;
                    }

                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: pattern.severity,
                        category: FindingCategory::Injection,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Insecure deserialization: {} at line {}",
                            pattern.name, line_1based
                        ),
                        description: format!(
                            "{} in {}:{}",
                            pattern.description,
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: pattern.suggestion.into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![502],
                    });
                    break; // one finding per line
                }
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    fn single_file(name: &str, content: &str, lang: Language) -> AnalysisContext {
        let mut files = HashMap::new();
        files.insert(PathBuf::from(name), content.into());
        make_ctx(files, lang)
    }

    // ---- Python ----

    #[tokio::test]
    async fn multi_insecure_deser_python_pickle_loads() {
        let ctx = single_file("src/data.py", "obj = pickle.loads(data)\n", Language::Python);
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].cwe_ids, vec![502]);
    }

    #[tokio::test]
    async fn multi_insecure_deser_python_pickle_load() {
        let ctx = single_file("src/data.py", "obj = pickle.load(fp)\n", Language::Python);
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_python_yaml_load_unsafe() {
        let ctx = single_file("src/cfg.py", "cfg = yaml.load(raw)\n", Language::Python);
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_python_yaml_safe_load() {
        let ctx = single_file(
            "src/cfg.py",
            "cfg = yaml.safe_load(raw)\n",
            Language::Python,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_python_yaml_with_safe_loader() {
        let ctx = single_file(
            "src/cfg.py",
            "cfg = yaml.load(raw, Loader=SafeLoader)\n",
            Language::Python,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_python_marshal() {
        let ctx = single_file(
            "src/data.py",
            "obj = marshal.loads(data)\n",
            Language::Python,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_python_shelve() {
        let ctx = single_file("src/data.py", "db = shelve.open('data')\n", Language::Python);
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // ---- JavaScript ----

    #[tokio::test]
    async fn multi_insecure_deser_js_yaml_load() {
        let ctx = single_file(
            "src/config.js",
            "const data = yaml.load(rawInput);\n",
            Language::JavaScript,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_js_yaml_safe_load() {
        let ctx = single_file(
            "src/config.js",
            "const data = yaml.safeLoad(rawInput);\n",
            Language::JavaScript,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_js_eval_json_parse() {
        let ctx = single_file(
            "src/handler.js",
            "const result = eval(JSON.parse(input));\n",
            Language::JavaScript,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_js_new_function() {
        let ctx = single_file(
            "src/exec.js",
            "const fn = new Function(userCode);\n",
            Language::JavaScript,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_js_safe_json_parse() {
        let ctx = single_file(
            "src/parse.js",
            "const data = JSON.parse(input);\n",
            Language::JavaScript,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_js_node_serialize() {
        let ctx = single_file(
            "src/ser.js",
            "const serialize = require('node-serialize');\n",
            Language::JavaScript,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- Java ----

    #[tokio::test]
    async fn multi_insecure_deser_java_read_object() {
        let ctx = single_file(
            "src/Data.java",
            "Object obj = ois.readObject();\n",
            Language::Java,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_java_read_object_with_filter() {
        let ctx = single_file(
            "src/Data.java",
            "ois.setObjectInputFilter(filter); Object obj = ois.readObject(); // uses ObjectInputFilter\n",
            Language::Java,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        // The readObject line itself contains ObjectInputFilter in a comment,
        // but the safe_indicators check is per-line, so it should be safe.
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_java_xml_decoder() {
        let ctx = single_file(
            "src/Data.java",
            "XMLDecoder decoder = new XMLDecoder(input);\n",
            Language::Java,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_java_xstream() {
        let ctx = single_file(
            "src/Data.java",
            "Object obj = new XStream().fromXML(xml);\n",
            Language::Java,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- Kotlin ----

    #[tokio::test]
    async fn multi_insecure_deser_kotlin_read_object() {
        let ctx = single_file(
            "src/Data.kt",
            "val obj = ois.readObject()\n",
            Language::Kotlin,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- Go ----

    #[tokio::test]
    async fn multi_insecure_deser_go_gob_decode() {
        let ctx = single_file(
            "main.go",
            "dec := gob.NewDecoder(conn)\n",
            Language::Go,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_go_json_unmarshal_typed() {
        let ctx = single_file(
            "main.go",
            "err := json.Unmarshal(data, &user)\n",
            Language::Go,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        // Typed struct — should not trigger
        assert!(findings.is_empty());
    }

    // ---- Ruby ----

    #[tokio::test]
    async fn multi_insecure_deser_ruby_marshal_load() {
        let ctx = single_file(
            "app/data.rb",
            "obj = Marshal.load(data)\n",
            Language::Ruby,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_ruby_yaml_load() {
        let ctx = single_file(
            "app/config.rb",
            "cfg = YAML.load(raw)\n",
            Language::Ruby,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_ruby_yaml_safe_load() {
        let ctx = single_file(
            "app/config.rb",
            "cfg = YAML.safe_load(raw)\n",
            Language::Ruby,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_ruby_eval() {
        let ctx = single_file(
            "app/exec.rb",
            "result = eval(user_input)\n",
            Language::Ruby,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- C# ----

    #[tokio::test]
    async fn multi_insecure_deser_csharp_binary_formatter() {
        let ctx = single_file(
            "src/Data.cs",
            "var obj = new BinaryFormatter().Deserialize(stream);\n",
            Language::CSharp,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        // The BinaryFormatter regex requires "BinaryFormatter" in the line
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_csharp_soap_formatter() {
        let ctx = single_file(
            "src/Data.cs",
            "var obj = new SoapFormatter().Deserialize(stream);\n",
            Language::CSharp,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_csharp_type_name_handling() {
        let ctx = single_file(
            "src/Config.cs",
            "settings.TypeNameHandling = TypeNameHandling.All;\n",
            Language::CSharp,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_csharp_type_name_none_safe() {
        let ctx = single_file(
            "src/Config.cs",
            "settings.TypeNameHandling = TypeNameHandling.None;\n",
            Language::CSharp,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_csharp_net_data_contract() {
        let ctx = single_file(
            "src/Data.cs",
            "var obj = new NetDataContractSerializer().Deserialize(stream);\n",
            Language::CSharp,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- Swift ----

    #[tokio::test]
    async fn multi_insecure_deser_swift_unarchive_object() {
        let ctx = single_file(
            "Sources/Data.swift",
            "let obj = NSKeyedUnarchiver.unarchiveObject(with: data)\n",
            Language::Swift,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn multi_insecure_deser_swift_safe_unarchive() {
        let ctx = single_file(
            "Sources/Data.swift",
            "let obj = try NSKeyedUnarchiver.unarchivedObject(ofClass: MyClass.self, from: data)\n",
            Language::Swift,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- C/C++ ----

    #[tokio::test]
    async fn multi_insecure_deser_c_unserialize() {
        let ctx = single_file(
            "src/data.c",
            "obj = unserialize(buffer, len);\n",
            Language::C,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- Rust ----

    #[tokio::test]
    async fn multi_insecure_deser_rust_bincode() {
        let ctx = single_file(
            "src/data.rs",
            "let obj: Config = bincode::deserialize(&bytes).unwrap();\n",
            Language::Rust,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[tokio::test]
    async fn multi_insecure_deser_rust_bincode_with_limit_safe() {
        let ctx = single_file(
            "src/data.rs",
            "let obj: Config = DefaultOptions::new().with_limit(1024).deserialize(&bytes).unwrap();\n",
            Language::Rust,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        // DefaultOptions is a safe indicator for bincode, but our regex specifically matches
        // "bincode::deserialize" which won't match "DefaultOptions...deserialize"
        assert!(findings.is_empty());
    }

    // ---- Cross-cutting ----

    #[tokio::test]
    async fn multi_insecure_deser_skips_test_files() {
        let ctx = single_file(
            "tests/test_deser.py",
            "obj = pickle.loads(data)\n",
            Language::Python,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_skips_comments() {
        let ctx = single_file(
            "src/data.py",
            "# obj = pickle.loads(data)\n",
            Language::Python,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multi_insecure_deser_skips_wasm() {
        let ctx = single_file(
            "src/module.wasm",
            "pickle.loads(data)\n",
            Language::Wasm,
        );
        let findings = MultiInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MultiInsecureDeserDetector.uses_cargo_subprocess());
    }
}
