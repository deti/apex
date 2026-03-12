use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file, strip_string_literals};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct SecurityPatternDetector;

struct SecurityPattern {
    sink: &'static str,
    description: &'static str,
    category: FindingCategory,
    base_severity: Severity,
    user_input_indicators: &'static [&'static str],
    sanitization_indicators: &'static [&'static str],
}

const RUST_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "Command::new(",
        description: "Command injection — user input flows into shell command",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &[
            "format!", "user", "input", "request", "query", "arg(", "&str",
        ],
        sanitization_indicators: &["escape", "sanitize", "quote", "shell_escape"],
    },
    SecurityPattern {
        sink: "std::process::Command",
        description: "Process command construction — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["format!", "user", "input", "request"],
        sanitization_indicators: &["escape", "sanitize"],
    },
];

const PYTHON_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() with potential user input — code injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[
            "request", "input", "param", "query", "form", "argv", "stdin",
        ],
        sanitization_indicators: &["ast.literal_eval", "safe_eval"],
    },
    SecurityPattern {
        sink: "exec(",
        description: "exec() with potential user input — code injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "input", "param", "query", "form"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "pickle.load",
        description: "Pickle deserialization — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "upload", "file", "open(", "recv", "socket"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "yaml.load(",
        description: "Unsafe YAML loading — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "open(", "read"],
        sanitization_indicators: &["SafeLoader", "safe_load", "CSafeLoader"],
    },
    SecurityPattern {
        sink: "subprocess.call(",
        description: "Subprocess call — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
        sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
    },
    SecurityPattern {
        sink: "os.system(",
        description: "os.system() — command injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["format(", "f\"", "request", "input", "+", "%"],
        sanitization_indicators: &["shlex.quote"],
    },
    SecurityPattern {
        sink: ".execute(f",
        description: "SQL query with f-string — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: ".execute(",
        description: "SQL execute — potential SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["%s", "format(", "+", "%", "f\""],
        sanitization_indicators: &["?", "%s,", "parameterize", "placeholder"],
    },
    SecurityPattern {
        sink: "mark_safe(",
        description: "mark_safe() — potential XSS if user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "user", "input", "form", "query"],
        sanitization_indicators: &["escape", "bleach", "sanitize", "strip_tags"],
    },
    SecurityPattern {
        sink: "hashlib.md5(",
        description: "MD5 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "hashlib.sha1(",
        description: "SHA1 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "verify=False",
        description: "TLS verification disabled — man-in-the-middle risk",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &[],
        sanitization_indicators: &[],
    },
];

const JS_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() — arbitrary code execution if input is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input", "argv"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "Function(",
        description: "new Function() — dynamic code generation, equivalent to eval",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "child_process.exec(",
        description: "child_process.exec — command injection via shell",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input", "${", "`"],
        sanitization_indicators: &["escape", "sanitize", "execFile"],
    },
    SecurityPattern {
        sink: "innerHTML",
        description: "innerHTML assignment — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "user", "input", "query", "param", "response"],
        sanitization_indicators: &["sanitize", "escape", "DOMPurify", "encode", "textContent"],
    },
    SecurityPattern {
        sink: "dangerouslySetInnerHTML",
        description: "dangerouslySetInnerHTML — XSS, React's escape hatch for raw HTML",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["user", "input", "props", "state", "data", "response"],
        sanitization_indicators: &["sanitize", "DOMPurify", "bleach"],
    },
    SecurityPattern {
        sink: "document.write(",
        description: "document.write() — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["user", "input", "location", "search", "hash", "referrer"],
        sanitization_indicators: &["escape", "encode", "sanitize"],
    },
    SecurityPattern {
        sink: "vm.runIn",
        description: "vm.runInContext/vm.runInNewContext — sandbox escape risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "user", "input"],
        sanitization_indicators: &[],
    },
];

const RUBY_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input", "gets", "ARGV"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "instance_eval",
        description: "instance_eval — arbitrary code execution in object context",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input", "gets"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "class_eval",
        description: "class_eval — arbitrary code execution in class context",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "send(",
        description: "send() — arbitrary method invocation if argument is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "request", "input", "gets"],
        sanitization_indicators: &["whitelist", "allow_list", "permitted", "include?"],
    },
    SecurityPattern {
        sink: "constantize",
        description: "constantize — arbitrary class instantiation from user string",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "request", "input"],
        sanitization_indicators: &["whitelist", "allow_list", "permitted", "include?"],
    },
    SecurityPattern {
        sink: ".html_safe",
        description: ".html_safe — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "user", "input", "request", "@"],
        sanitization_indicators: &["sanitize", "strip_tags", "escape", "h("],
    },
    SecurityPattern {
        sink: "Marshal.load",
        description: "Marshal.load — arbitrary code execution on untrusted data",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "file", "socket", "params", "upload"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "YAML.load(",
        description: "YAML.load — arbitrary code execution without safe_load",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "params"],
        sanitization_indicators: &["safe_load", "safe_load_file", "permitted_classes"],
    },
    SecurityPattern {
        sink: ".where(",
        description: "ActiveRecord .where() with potential string interpolation — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["#{", "params", "request", "input", "+"],
        sanitization_indicators: &["sanitize_sql", "?", "placeholder", "where("],
    },
];

const C_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "gets(",
        description: "gets() — unbounded read, guaranteed buffer overflow",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Critical,
        user_input_indicators: &[], // always dangerous
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "strcpy(",
        description: "strcpy() — no bounds checking, use strncpy or strlcpy",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "read(", "getenv"],
        sanitization_indicators: &["strlen", "sizeof", "strlcpy", "strncpy"],
    },
    SecurityPattern {
        sink: "sprintf(",
        description: "sprintf() — no bounds checking, use snprintf",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "%s"],
        sanitization_indicators: &["snprintf"],
    },
    SecurityPattern {
        sink: "strcat(",
        description: "strcat() — no bounds checking, use strncat or strlcat",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv"],
        sanitization_indicators: &["strncat", "strlcat", "strlen"],
    },
    SecurityPattern {
        sink: "system(",
        description: "system() — command injection if argument contains user input",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "sprintf"],
        sanitization_indicators: &["escape", "sanitize"],
    },
];

const CONTEXT_WINDOW: usize = 3;

fn has_indicator(lines: &[&str], line_num: usize, indicators: &[&str]) -> bool {
    if indicators.is_empty() {
        return false;
    }
    let start = line_num.saturating_sub(CONTEXT_WINDOW);
    let end = (line_num + CONTEXT_WINDOW + 1).min(lines.len());
    for line in lines.iter().take(end).skip(start) {
        let line_lower = line.to_lowercase();
        for indicator in indicators {
            if line_lower.contains(&indicator.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

fn adjust_severity(
    base: Severity,
    has_user_input: bool,
    has_sanitization: bool,
    indicators_defined: bool,
) -> Severity {
    // If no user_input_indicators were defined, pattern is inherently dangerous — stay at base
    let sev = if !indicators_defined || has_user_input {
        base
    } else {
        downgrade(base)
    };
    if has_sanitization {
        downgrade(sev)
    } else {
        sev
    }
}

fn downgrade(s: Severity) -> Severity {
    match s {
        Severity::Critical => Severity::High,
        Severity::High => Severity::Medium,
        Severity::Medium => Severity::Low,
        Severity::Low => Severity::Low,
        Severity::Info => Severity::Info,
    }
}

fn patterns_for_language(lang: Language) -> &'static [SecurityPattern] {
    match lang {
        Language::Python => PYTHON_SECURITY_PATTERNS,
        Language::Rust => RUST_SECURITY_PATTERNS,
        Language::JavaScript => JS_SECURITY_PATTERNS,
        Language::Ruby => RUBY_SECURITY_PATTERNS,
        Language::C => C_SECURITY_PATTERNS,
        _ => &[],
    }
}

#[async_trait]
impl Detector for SecurityPatternDetector {
    fn name(&self) -> &str {
        "security-pattern"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        let patterns = patterns_for_language(ctx.language);

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let all_lines: Vec<&str> = source.lines().collect();

            for (line_num, line) in all_lines.iter().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if trimmed == "#[test]" || trimmed == "#[tokio::test]" {
                    continue;
                }

                if in_test_block(source, line_num) {
                    continue;
                }

                let stripped = strip_string_literals(trimmed);
                for pattern in patterns {
                    if stripped.contains(pattern.sink) {
                        let line_1based = (line_num + 1) as u32;

                        let has_user_input =
                            has_indicator(&all_lines, line_num, pattern.user_input_indicators);
                        let has_sanitization =
                            has_indicator(&all_lines, line_num, pattern.sanitization_indicators);
                        let indicators_defined = !pattern.user_input_indicators.is_empty();

                        let severity = adjust_severity(
                            pattern.base_severity,
                            has_user_input,
                            has_sanitization,
                            indicators_defined,
                        );

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity,
                            category: pattern.category,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!("{} at line {line_1based}", pattern.description),
                            description: format!(
                                "Pattern `{}` found in {}:{}",
                                pattern.sink,
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion: "Validate and sanitize input before use".into(),
                            explanation: None,
                            fix: None,
                        });
                        break; // One finding per line max
                    }
                }
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx(source_files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: lang,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: source_files,
            fuzz_corpus: None,
            config: DetectConfig::default(),
            runner: Arc::new(apex_core::command::RealCommandRunner),
        }
    }

    #[tokio::test]
    async fn rust_command_injection_with_format() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn run(user: &str) {\n    let cmd = format!(\"echo {}\", user);\n    Command::new(cmd);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].severity == Severity::High || findings[0].severity == Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn python_eval_with_request_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def handle(request):\n    data = request.get('expr')\n    result = eval(data)\n    return result\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn python_eval_without_user_input_downgraded() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/calc.py"),
            "def compute():\n    x = '2 + 2'\n    return eval(x)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High); // downgraded from Critical
    }

    #[tokio::test]
    async fn python_yaml_safe_loader_downgraded() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/loader.py"),
            "import yaml\ndef load(path):\n    with open(path) as f:\n        return yaml.load(f, Loader=SafeLoader)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        // base=High, open( in context window -> has_user_input=true -> stays High,
        // SafeLoader on same line -> has_sanitization=true -> downgrade to Medium
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn python_sql_fstring_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.py"),
            "def query(name):\n    cursor.execute(f\"SELECT * FROM users WHERE name='{name}'\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn python_pickle_from_socket_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/net.py"),
            "import pickle\nimport socket\ndef recv(sock):\n    data = sock.recv(4096)\n    return pickle.load(data)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn python_verify_false_is_high() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/http.py"),
            "import requests\ndef fetch(url):\n    return requests.get(url, verify=False)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_app.py"),
            "def test_eval():\n    eval('2+2')\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_findings_for_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.wasm"),
            "eval('alert(1)');\n".into(),
        );
        let ctx = make_ctx(files, Language::Wasm);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn js_eval_with_user_input_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/handler.js"),
            "function handle(req) {\n    const result = eval(req.body.code);\n    return result;\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn js_innerhtml_with_user_data_is_high() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/render.js"),
            "function render(userData) {\n    el.innerHTML = userData;\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn ruby_eval_with_params_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/controllers/calc_controller.rb"),
            "def calculate\n  result = eval(params[:expr])\n  render json: result\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn ruby_marshal_from_request_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/services/deserialize.rb"),
            "def load_data(request)\n  Marshal.load(request.body)\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn c_gets_is_always_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "int main() {\n    char buf[64];\n    gets(buf);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn c_strcpy_without_user_input_is_medium() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/util.c"),
            "void copy(char *dst, const char *src) {\n    strcpy(dst, src);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn skips_comments_python() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "# eval(request.data)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "// Command::new(format!(\"echo {}\", user));\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_attribute_lines() {
        // Individual #[test] fns are NOT inside #[cfg(test)] blocks,
        // so they are still scanned. Only #[cfg(test)] mod blocks are skipped.
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "#[test]\nfn test_cmd() { Command::new(\"echo\"); }\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_cfg_test_block() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "pub fn real() {}\n\n#[cfg(test)]\nmod tests {\n    fn test_it() {\n        let _ = eval(\"1+1\");\n    }\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn sanitization_with_user_input_downgrades_once() {
        // Python subprocess with shell=True (user input indicator) + shlex.quote (sanitization)
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.py"),
            "import shlex\ndef run(request):\n    cmd = shlex.quote(request.input)\n    subprocess.call(cmd, shell=True)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        // base=High, user input (shell=True + request) → stays High, sanitization (shlex.quote) → Medium
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn empty_source_no_findings() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/empty.py"), "".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn multiple_findings_different_lines() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/bad.py"),
            "eval(x)\nexec(y)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
    }

    #[tokio::test]
    async fn c_system_with_argv_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "int main(int argc, char *argv[]) {\n    system(argv[1]);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn ruby_html_safe_with_user_params() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/helpers/tag_helper.rb"),
            "def render_tag(params)\n  params[:html].html_safe\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn js_child_process_exec_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/exec.js"),
            "const cp = require('child_process');\nfunction run(req) {\n    cp.child_process.exec(req.body.cmd);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn detector_name_is_correct() {
        assert_eq!(SecurityPatternDetector.name(), "security-pattern");
    }

    #[test]
    fn downgrade_low_stays_low() {
        assert_eq!(downgrade(Severity::Low), Severity::Low);
    }

    #[test]
    fn downgrade_info_stays_info() {
        assert_eq!(downgrade(Severity::Info), Severity::Info);
    }

    #[test]
    fn adjust_severity_no_indicators_defined() {
        // Empty indicators = always dangerous, stays at base
        let sev = adjust_severity(Severity::Critical, false, false, false);
        assert_eq!(sev, Severity::Critical);
    }

    #[test]
    fn adjust_severity_indicators_defined_but_absent() {
        // Indicators defined but no match → downgrade
        let sev = adjust_severity(Severity::Critical, false, false, true);
        assert_eq!(sev, Severity::High);
    }

    #[test]
    fn adjust_severity_user_input_and_sanitization() {
        // Both present → base stays, then downgrade for sanitization
        let sev = adjust_severity(Severity::Critical, true, true, true);
        assert_eq!(sev, Severity::High);
    }
}
