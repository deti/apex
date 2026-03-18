use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file, strip_string_literals};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::threat_model::should_suppress;
use crate::Detector;

pub struct SecurityPatternDetector;

struct SecurityPattern {
    sink: &'static str,
    description: &'static str,
    category: FindingCategory,
    base_severity: Severity,
    user_input_indicators: &'static [&'static str],
    sanitization_indicators: &'static [&'static str],
    cwe: &'static [u32],
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
        cwe: &[78],
    },
    SecurityPattern {
        sink: "std::process::Command",
        description: "Process command construction — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["format!", "user", "input", "request"],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
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
        cwe: &[94],
    },
    SecurityPattern {
        sink: "exec(",
        description: "exec() with potential user input — code injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "input", "param", "query", "form"],
        sanitization_indicators: &[],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "pickle.load",
        description: "Pickle deserialization — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "upload", "file", "open(", "recv", "socket"],
        sanitization_indicators: &["trusted", "internal", "cache"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "yaml.load(",
        description: "Unsafe YAML loading — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "open(", "read"],
        sanitization_indicators: &["SafeLoader", "safe_load", "CSafeLoader"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "subprocess.call(",
        description: "Subprocess call — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
        sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "subprocess.run(",
        description: "subprocess.run — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
        sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "subprocess.Popen(",
        description: "subprocess.Popen — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
        sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "os.popen(",
        description: "os.popen() — command injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["format(", "f\"", "request", "input", "%s", "+"],
        sanitization_indicators: &["shlex.quote"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "__import__(",
        description: "__import__() — dynamic module loading, code injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "input", "param", "query", "form", "argv"],
        sanitization_indicators: &["allowlist", "whitelist", "ALLOWED"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "os.system(",
        description: "os.system() — command injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["format(", "f\"", "request", "input", "+", "%"],
        sanitization_indicators: &["shlex.quote"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: ".execute(f",
        description: "SQL query with f-string — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[89],
    },
    SecurityPattern {
        sink: ".execute(",
        description: "SQL execute — potential SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["%s", "format(", "+", "%", "f\""],
        sanitization_indicators: &["?", "%s,", "parameterize", "placeholder"],
        cwe: &[89],
    },
    SecurityPattern {
        sink: "mark_safe(",
        description: "mark_safe() — potential XSS if user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "user", "input", "form", "query"],
        sanitization_indicators: &["escape", "bleach", "sanitize", "strip_tags"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "hashlib.md5(",
        description: "MD5 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
        cwe: &[328],
    },
    SecurityPattern {
        sink: "hashlib.sha1(",
        description: "SHA1 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
        cwe: &[328],
    },
    SecurityPattern {
        sink: "verify=False",
        description: "TLS verification disabled — man-in-the-middle risk",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[295],
    },
];

const JS_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() — arbitrary code execution if input is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[
            "req.", "request", "params", "query", "body", "input", "argv",
        ],
        sanitization_indicators: &[],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "Function(",
        description: "new Function() — dynamic code generation, equivalent to eval",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
        sanitization_indicators: &[],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "child_process.exec(",
        description: "child_process.exec — command injection via shell",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[
            "req.", "request", "params", "query", "body", "input", "${", "`",
        ],
        sanitization_indicators: &["escape", "sanitize", "execFile"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "child_process.execSync(",
        description: "child_process.execSync — synchronous command injection via shell",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[
            "req.", "request", "params", "query", "body", "input", "${", "`",
        ],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "child_process.spawn(",
        description: "child_process.spawn — command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "res.write(",
        description: "res.write() — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
        sanitization_indicators: &["escape", "encode", "sanitize", "textContent"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "res.send(",
        description: "res.send() — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
        sanitization_indicators: &["escape", "encode", "sanitize", "textContent", "json"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "require(",
        description: "require() — dynamic module loading, code injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &[
            "req.", "request", "params", "query", "body", "input", "argv",
        ],
        sanitization_indicators: &["allowlist", "whitelist", "ALLOWED", "path.join"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "innerHTML",
        description: "innerHTML assignment — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &[
            "req.", "request", "user", "input", "query", "param", "response",
        ],
        sanitization_indicators: &["sanitize", "escape", "DOMPurify", "encode", "textContent"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "dangerouslySetInnerHTML",
        description: "dangerouslySetInnerHTML — XSS, React's escape hatch for raw HTML",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["user", "input", "props", "state", "data", "response"],
        sanitization_indicators: &["sanitize", "DOMPurify", "bleach"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "document.write(",
        description: "document.write() — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["user", "input", "location", "search", "hash", "referrer"],
        sanitization_indicators: &["escape", "encode", "sanitize"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "vm.runIn",
        description: "vm.runInContext/vm.runInNewContext — sandbox escape risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "user", "input"],
        sanitization_indicators: &[],
        cwe: &[94],
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
        cwe: &[94],
    },
    SecurityPattern {
        sink: "instance_eval",
        description: "instance_eval — arbitrary code execution in object context",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input", "gets"],
        sanitization_indicators: &["define_method", "attr_", "schema", "migration"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "class_eval",
        description: "class_eval — arbitrary code execution in class context",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input"],
        sanitization_indicators: &["define_method", "attr_", "schema", "migration"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "send(",
        description: "send() — arbitrary method invocation if argument is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "request", "input", "gets"],
        sanitization_indicators: &["whitelist", "allow_list", "permitted", "include?"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "constantize",
        description: "constantize — arbitrary class instantiation from user string",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "request", "input"],
        sanitization_indicators: &["whitelist", "allow_list", "permitted", "include?"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: ".html_safe",
        description: ".html_safe — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "user", "input", "request", "@"],
        sanitization_indicators: &["sanitize", "strip_tags", "escape", "h("],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "Marshal.load",
        description: "Marshal.load — arbitrary code execution on untrusted data",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "file", "socket", "params", "upload"],
        sanitization_indicators: &[],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "YAML.load(",
        description: "YAML.load — arbitrary code execution without safe_load",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "params"],
        sanitization_indicators: &["safe_load", "safe_load_file", "permitted_classes"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: ".where(",
        description: "ActiveRecord .where() with potential string interpolation — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["#{", "params", "request", "input", "+"],
        sanitization_indicators: &["sanitize_sql", "?", "placeholder", "where("],
        cwe: &[89],
    },
];

const C_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "gets(",
        description: "gets() — banned function, unbounded read, guaranteed buffer overflow",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Critical,
        user_input_indicators: &[], // always dangerous
        sanitization_indicators: &[],
        cwe: &[242],
    },
    SecurityPattern {
        sink: "strcpy(",
        description: "strcpy() — no bounds checking, use strncpy or strlcpy",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "read(", "getenv"],
        sanitization_indicators: &["strlen", "sizeof", "strlcpy", "strncpy"],
        cwe: &[120],
    },
    SecurityPattern {
        sink: "sprintf(",
        description: "sprintf() — no bounds checking, use snprintf",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "%s"],
        sanitization_indicators: &["snprintf"],
        cwe: &[120],
    },
    SecurityPattern {
        sink: "strcat(",
        description: "strcat() — no bounds checking, use strncat or strlcat",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv"],
        sanitization_indicators: &["strncat", "strlcat", "strlen"],
        cwe: &[120],
    },
    SecurityPattern {
        sink: "system(",
        description: "system() — command injection if argument contains user input",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "sprintf"],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "printf(",
        description: "printf() with non-literal format string — format string vulnerability",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "buf"],
        sanitization_indicators: &["snprintf"],
        cwe: &[134],
    },
    SecurityPattern {
        sink: "malloc(",
        description: "malloc() — integer overflow in size calculation may cause heap overflow",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Medium,
        user_input_indicators: &["*", "argv", "atoi", "strtol", "recv", "read("],
        sanitization_indicators: &["SIZE_MAX", "check", "limit", "max_size", "overflow"],
        cwe: &[190],
    },
];

const CPP_SECURITY_PATTERNS: &[SecurityPattern] = &[
    // Include all C patterns for C++ as well
    SecurityPattern {
        sink: "gets(",
        description: "gets() — banned function, unbounded read, guaranteed buffer overflow",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Critical,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[242],
    },
    SecurityPattern {
        sink: "strcpy(",
        description: "strcpy() — no bounds checking, use strncpy or strlcpy",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "read(", "getenv"],
        sanitization_indicators: &["strlen", "sizeof", "strlcpy", "strncpy"],
        cwe: &[120],
    },
    SecurityPattern {
        sink: "sprintf(",
        description: "sprintf() — no bounds checking, use snprintf",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "%s"],
        sanitization_indicators: &["snprintf"],
        cwe: &[120],
    },
    SecurityPattern {
        sink: "strcat(",
        description: "strcat() — no bounds checking, use strncat or strlcat",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv"],
        sanitization_indicators: &["strncat", "strlcat", "strlen"],
        cwe: &[120],
    },
    SecurityPattern {
        sink: "system(",
        description: "system() — command injection if argument contains user input",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "sprintf"],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "printf(",
        description: "printf() with non-literal format string — format string vulnerability",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "buf"],
        sanitization_indicators: &["snprintf"],
        cwe: &[134],
    },
    SecurityPattern {
        sink: "malloc(",
        description: "malloc() — integer overflow in size calculation may cause heap overflow",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Medium,
        user_input_indicators: &["*", "argv", "atoi", "strtol", "recv", "read("],
        sanitization_indicators: &["SIZE_MAX", "check", "limit", "max_size", "overflow"],
        cwe: &[190],
    },
    // C++-specific patterns
    SecurityPattern {
        sink: "reinterpret_cast<",
        description: "reinterpret_cast — unsafe type cast, may violate type safety",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Medium,
        user_input_indicators: &["void*", "char*", "uint8_t*"],
        sanitization_indicators: &["static_cast", "dynamic_cast"],
        cwe: &[704],
    },
    SecurityPattern {
        sink: "new ",
        description: "raw new without smart pointer — potential memory leak",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Low,
        user_input_indicators: &["return", "="],
        sanitization_indicators: &[
            "unique_ptr",
            "shared_ptr",
            "make_unique",
            "make_shared",
            "delete",
        ],
        cwe: &[401],
    },
    SecurityPattern {
        sink: "std::system(",
        description: "std::system() — command injection via C++ standard library",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["argv", "cin", "getline", "getenv", "string"],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
    },
];

const JAVA_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "Runtime.getRuntime().exec(",
        description: "Runtime.exec — command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "getParameter", "input", "args"],
        sanitization_indicators: &[],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "ProcessBuilder(",
        description: "ProcessBuilder — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "getParameter", "input", "args"],
        sanitization_indicators: &[],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "executeQuery(",
        description: "executeQuery — potential SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["+", "format", "request", "getParameter", "concat"],
        sanitization_indicators: &["PreparedStatement", "parameterized", "?"],
        cwe: &[89],
    },
    SecurityPattern {
        sink: "executeUpdate(",
        description: "executeUpdate — potential SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["+", "format", "request", "getParameter", "concat"],
        sanitization_indicators: &["PreparedStatement", "parameterized", "?"],
        cwe: &[89],
    },
    SecurityPattern {
        sink: "readObject(",
        description: "readObject — unsafe deserialization",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["socket", "request", "upload", "input", "InputStream"],
        sanitization_indicators: &["ObjectInputFilter", "ValidatingObjectInputStream"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "getWriter().print(",
        description: "getWriter().print — potential XSS",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "getParameter", "getHeader", "getCookie"],
        sanitization_indicators: &["encode", "escape", "sanitize", "ESAPI"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "new URL(",
        description: "new URL() — potential SSRF",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["request", "getParameter", "input", "param"],
        sanitization_indicators: &["allowlist", "whitelist", "ALLOWED"],
        cwe: &[918],
    },
    SecurityPattern {
        sink: "MessageDigest.getInstance(\"MD5\"",
        description: "MD5 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret"],
        sanitization_indicators: &[],
        cwe: &[328],
    },
    SecurityPattern {
        sink: "MessageDigest.getInstance(\"SHA-1\"",
        description: "SHA-1 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret"],
        sanitization_indicators: &[],
        cwe: &[328],
    },
];

/// Indicators that are code patterns (not input sources). These should be
/// excluded from threat model trust classification since they describe how
/// data is used, not where it comes from.
const CODE_PATTERN_INDICATORS: &[&str] = &[
    "shell=true",
    "shell=false",
    "format!",
    "format(",
    "f\"",
    "%s",
    "%s,",
    "%",
    "+",
    "open(",
    "read",
    "parameterize",
    "placeholder",
    "?",
];

fn has_indicator(lines: &[&str], line_num: usize, indicators: &[&str], context_window: usize) -> bool {
    !collect_matched_indicators(lines, line_num, indicators, context_window).is_empty()
}

/// Like `has_indicator`, but returns the matched indicator strings.
/// Used by threat-model-aware suppression to classify trust per indicator.
fn collect_matched_indicators<'a>(
    lines: &[&str],
    line_num: usize,
    indicators: &[&'a str],
    context_window: usize,
) -> Vec<&'a str> {
    if indicators.is_empty() {
        return Vec::new();
    }
    let start = line_num.saturating_sub(context_window);
    let end = (line_num + context_window + 1).min(lines.len());
    let mut matched = Vec::new();
    for line in lines.iter().take(end).skip(start) {
        let line_lower = line.to_lowercase();
        for indicator in indicators {
            if line_lower.contains(&indicator.to_lowercase()) && !matched.contains(indicator) {
                matched.push(*indicator);
            }
        }
    }
    matched
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

// Go security patterns
const GO_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "exec.Command(",
        description: "Command execution — verify input is not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "param", "input", "query", "body", "form"],
        sanitization_indicators: &["filepath.Clean", "regexp.MustCompile"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "db.Query(fmt.Sprintf",
        description: "SQL query with format string — use parameterized queries",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "param", "input", "query"],
        sanitization_indicators: &["Prepare", "stmt"],
        cwe: &[89],
    },
    SecurityPattern {
        sink: "http.Get(",
        description: "HTTP request — verify URL is not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "param", "input", "url"],
        sanitization_indicators: &["url.Parse", "allowlist"],
        cwe: &[918],
    },
    SecurityPattern {
        sink: "template.HTML(",
        description: "Unescaped HTML template — may enable XSS",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["request", "input"],
        sanitization_indicators: &["template.HTMLEscapeString"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "os.Open(",
        description: "File open — verify path is not user-controlled",
        category: FindingCategory::PathTraversal,
        base_severity: Severity::Medium,
        user_input_indicators: &["request", "param", "input", "path"],
        sanitization_indicators: &["filepath.Clean", "filepath.Abs"],
        cwe: &[22],
    },
    SecurityPattern {
        sink: "md5.New(",
        description: "Weak hash algorithm MD5",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[327],
    },
    SecurityPattern {
        sink: "sha1.New(",
        description: "Weak hash algorithm SHA1",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[327],
    },
];

// Swift security patterns
const SWIFT_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "Process()",
        description: "Process execution — verify arguments are not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "input", "userInput"],
        sanitization_indicators: &[],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "URLSession.shared.dataTask(",
        description: "HTTP request — verify URL is not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "input", "userInput", "url"],
        sanitization_indicators: &["allowlist", "validate"],
        cwe: &[918],
    },
    SecurityPattern {
        sink: "NSAppleScript(source:",
        description: "AppleScript execution from string — code injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["input", "userInput"],
        sanitization_indicators: &[],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "UserDefaults.standard.set(",
        description: "UserDefaults for sensitive data — use Keychain instead",
        category: FindingCategory::HardcodedSecret,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key"],
        sanitization_indicators: &["Keychain", "SecItem"],
        cwe: &[312],
    },
    SecurityPattern {
        sink: "NSKeyedUnarchiver.unarchiveObject(",
        description: "Insecure deserialization — use unarchivedObject(ofClass:from:)",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &[],
        sanitization_indicators: &["unarchivedObject", "NSSecureCoding"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "try!",
        description: "Force try — crashes on error, use do/catch in library code",
        category: FindingCategory::PanicPath,
        base_severity: Severity::Medium,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[755],
    },
    SecurityPattern {
        sink: "fatalError(",
        description: "fatalError in library code — return Result/Optional instead",
        category: FindingCategory::PanicPath,
        base_severity: Severity::Medium,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[705],
    },
];

// C# security patterns
const CSHARP_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "Process.Start(",
        description: "Process execution — verify arguments are not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["Request", "input", "userInput", "param", "[FromBody]", "HttpContext"],
        sanitization_indicators: &["ProcessStartInfo", "ArgumentList"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "new Process",
        description: "Process creation — verify arguments are not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["Request", "input", "userInput", "param", "[FromBody]", "HttpContext"],
        sanitization_indicators: &["ProcessStartInfo", "ArgumentList"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "SqlCommand(",
        description: "SQL command — use parameterized queries with SqlParameter",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["Request", "input", "param", "query", "[FromBody]", "HttpContext"],
        sanitization_indicators: &["Parameters.Add", "SqlParameter", "@"],
        cwe: &[89],
    },
    SecurityPattern {
        sink: "SqlConnection(",
        description: "SQL connection string construction — may include user-controlled credentials",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["Request", "input", "param", "query", "[FromBody]"],
        sanitization_indicators: &["SqlConnectionStringBuilder", "ConnectionString"],
        cwe: &[89],
    },
    SecurityPattern {
        sink: "Deserialize<",
        description: "Generic deserialization — verify type is trusted before deserializing",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &["Request", "input", "userInput", "[FromBody]", "HttpContext"],
        sanitization_indicators: &["JsonSerializerOptions", "KnownTypeAttribute"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "HttpClient",
        description: "HTTP request — verify URL is not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["Request", "input", "url", "userInput"],
        sanitization_indicators: &["allowlist", "Uri.IsWellFormedUriString"],
        cwe: &[918],
    },
    SecurityPattern {
        sink: "BinaryFormatter",
        description: "BinaryFormatter is insecure — use System.Text.Json or protobuf",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Critical,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "MD5.Create(",
        description: "Weak hash algorithm MD5",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[327],
    },
    SecurityPattern {
        sink: "SHA1.Create(",
        description: "Weak hash algorithm SHA1",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &[],
        sanitization_indicators: &[],
        cwe: &[327],
    },
    SecurityPattern {
        sink: "Response.Write(",
        description: "Direct response write — may enable XSS",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["Request", "input", "userInput"],
        sanitization_indicators: &["HtmlEncode", "AntiXss"],
        cwe: &[79],
    },
    SecurityPattern {
        sink: "File.ReadAllText(",
        description: "File read — verify path is not user-controlled",
        category: FindingCategory::PathTraversal,
        base_severity: Severity::Medium,
        user_input_indicators: &["Request", "input", "path", "userInput"],
        sanitization_indicators: &["Path.GetFullPath", "Path.Combine"],
        cwe: &[22],
    },
];

const KOTLIN_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "ProcessBuilder(",
        description: "ProcessBuilder — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "params", "input", "query", "body"],
        sanitization_indicators: &["escape", "sanitize"],
        cwe: &[78],
    },
    SecurityPattern {
        sink: "${",
        description: "String template injection — verify interpolated value is not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["request", "params", "input", "query", "body"],
        sanitization_indicators: &["escape", "sanitize", "encode"],
        cwe: &[94],
    },
    SecurityPattern {
        sink: "Gson().fromJson",
        description: "Gson deserialization — unsafe type deserialization risk",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &["request", "params", "input", "body"],
        sanitization_indicators: &["GsonBuilder", "TypeAdapter"],
        cwe: &[502],
    },
    SecurityPattern {
        sink: "executeQuery(",
        description: "SQL query — potential SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["+", "format", "request", "params", "input"],
        sanitization_indicators: &["PreparedStatement", "parameterized", "?"],
        cwe: &[89],
    },
];

fn patterns_for_language(lang: Language) -> &'static [SecurityPattern] {
    match lang {
        Language::Python => PYTHON_SECURITY_PATTERNS,
        Language::Rust => RUST_SECURITY_PATTERNS,
        Language::JavaScript => JS_SECURITY_PATTERNS,
        Language::Ruby => RUBY_SECURITY_PATTERNS,
        Language::C => C_SECURITY_PATTERNS,
        Language::Cpp => CPP_SECURITY_PATTERNS,
        Language::Java => JAVA_SECURITY_PATTERNS,
        Language::Kotlin => KOTLIN_SECURITY_PATTERNS,
        Language::Go => GO_SECURITY_PATTERNS,
        Language::Swift => SWIFT_SECURITY_PATTERNS,
        Language::CSharp => CSHARP_SECURITY_PATTERNS,
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
                        // Bug 1/13 (CWE-134): Skip printf/similar with a literal format string —
                        // e.g. printf("hello %d", x) is safe; printf(buf) is not.
                        // pattern.sink already ends with '(' (e.g. "printf("), so `after` is
                        // the first argument directly.
                        if pattern.cwe.contains(&134) {
                            if let Some(pos) = trimmed.find(pattern.sink) {
                                let after =
                                    trimmed[pos + pattern.sink.len()..].trim_start();
                                if after.starts_with('"') {
                                    continue; // literal format string — safe
                                }
                            }
                        }

                        // Bug 4 (CWE-94): Skip eval/new-Function findings in compiler/codegen files
                        if pattern.cwe.contains(&94) {
                            let path_str = path.to_string_lossy();
                            if path_str.contains("compiler")
                                || path_str.contains("codegen")
                                || path_str.contains("generator")
                                || path_str.contains("emitter")
                                || path_str.contains("transformer")
                            {
                                continue;
                            }
                        }

                        let line_1based = (line_num + 1) as u32;

                        let context_window = ctx.config.context_window;
                        let matched_user_inputs = collect_matched_indicators(
                            &all_lines,
                            line_num,
                            pattern.user_input_indicators,
                            context_window,
                        );
                        let has_user_input = !matched_user_inputs.is_empty();
                        let has_sanitization = has_indicator(
                            &all_lines,
                            line_num,
                            pattern.sanitization_indicators,
                            context_window,
                        );
                        let indicators_defined = !pattern.user_input_indicators.is_empty();

                        // Threat model suppression: filter out code patterns
                        // (shell=True, format!, etc.) and only classify actual
                        // input source indicators.
                        let source_indicators: Vec<&str> = matched_user_inputs
                            .iter()
                            .copied()
                            .filter(|ind| {
                                !CODE_PATTERN_INDICATORS
                                    .iter()
                                    .any(|cp| ind.eq_ignore_ascii_case(cp))
                            })
                            .collect();
                        if let Some(true) = should_suppress(&ctx.threat_model, &source_indicators) {
                            break;
                        }

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
                            cwe_ids: pattern.cwe.to_vec(),
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
            language: lang,
            source_cache: source_files,
            ..AnalysisContext::test_default()
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
        assert!(
            findings[0].severity == Severity::High || findings[0].severity == Severity::Critical
        );
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
            "def query(name):\n    cursor.execute(f\"SELECT * FROM users WHERE name='{name}'\")\n"
                .into(),
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
        files.insert(PathBuf::from("src/main.wasm"), "eval('alert(1)');\n".into());
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
        files.insert(PathBuf::from("src/app.py"), "# eval(request.data)\n".into());
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
        // base=High, user input (shell=True + request) -> stays High, sanitization (shlex.quote) -> Medium
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
        files.insert(PathBuf::from("src/bad.py"), "eval(x)\nexec(y)\n".into());
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
            "const cp = import('child_process');\nfunction run(req) {\n    cp.child_process.exec(req.body.cmd);\n}\n".into(),
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
        // Indicators defined but no match -> downgrade
        let sev = adjust_severity(Severity::Critical, false, false, true);
        assert_eq!(sev, Severity::High);
    }

    #[test]
    fn adjust_severity_user_input_and_sanitization() {
        // Both present -> base stays, then downgrade for sanitization
        let sev = adjust_severity(Severity::Critical, true, true, true);
        assert_eq!(sev, Severity::High);
    }

    // Task 1 tests — Python command injection patterns

    #[tokio::test]
    async fn detects_subprocess_run_shell_true() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def execute(cmd):\n    subprocess.run(cmd, shell=True)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect subprocess.run");
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn detects_subprocess_popen() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def run(cmd):\n    p = subprocess.Popen(cmd, shell=True)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect subprocess.Popen");
    }

    #[tokio::test]
    async fn detects_os_popen() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def run(cmd):\n    os.popen(cmd)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect os.popen");
    }

    #[tokio::test]
    async fn detects_dunder_import() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def load(name):\n    mod = __import__(name)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect __import__");
    }

    // Task 2 tests — JS framework security patterns

    #[tokio::test]
    async fn detects_res_write_xss() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.js"),
            "function handle(req, res) {\n  res.write(req.query.data);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn detects_child_process_spawn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.js"),
            "const { spawn } = require('child_process');\nfunction run(req) {\n  child_process.spawn(req.body.cmd);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_require_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.js"),
            "function load(req) {\n  const mod = require(req.params.module);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    // Task 6 tests — Java security patterns

    #[tokio::test]
    async fn detects_java_runtime_exec() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/App.java"),
            "public void run(String cmd) {\n    Runtime.getRuntime().exec(cmd);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn detects_java_sql_injection() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Dao.java"),
            "public void find(String userId) {\n    stmt.executeQuery(\"SELECT * FROM users WHERE id=\" + userId);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_java_deserialization() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Server.java"),
            "public void handle(Socket socket) {\n    ObjectInputStream ois = new ObjectInputStream(socket.getInputStream());\n    Object obj = ois.readObject();\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn java_no_findings_for_safe_query() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Dao.java"),
            "public void find(String id) {\n    PreparedStatement ps = conn.prepareStatement(\"SELECT * FROM users WHERE id = ?\");\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // -- Threat model suppression tests --

    fn make_ctx_with_threat_model(
        source_files: HashMap<PathBuf, String>,
        lang: Language,
        threat_model: apex_core::config::ThreatModelConfig,
    ) -> AnalysisContext {
        let mut ctx = make_ctx(source_files, lang);
        ctx.threat_model = threat_model;
        ctx
    }

    #[tokio::test]
    async fn cli_tool_suppresses_argv_command_injection() {
        // CLI tool that runs a command with user input from arg() — trusted for CLI
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn main() {\n    let input = matches.get_one(\"cmd\");\n    Command::new(input);\n}\n"
                .into(),
        );
        let tm = apex_core::config::ThreatModelConfig {
            model_type: Some(apex_core::config::ThreatModelType::CliTool),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        };
        let ctx = make_ctx_with_threat_model(files, Language::Rust, tm);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        // "input" indicator is trusted for cli-tool → suppressed
        assert!(
            findings.is_empty(),
            "expected suppression for CLI input, got {findings:?}"
        );
    }

    #[tokio::test]
    async fn web_service_does_not_suppress_request_injection() {
        // Web service with request.args flowing into subprocess — untrusted, NOT suppressed
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app.py"),
            "def run():\n    cmd = request.args.get('cmd')\n    subprocess.call(cmd, shell=True)\n"
                .into(),
        );
        let tm = apex_core::config::ThreatModelConfig {
            model_type: Some(apex_core::config::ThreatModelType::WebService),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        };
        let ctx = make_ctx_with_threat_model(files, Language::Python, tm);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "web service request input should NOT be suppressed"
        );
    }

    #[tokio::test]
    async fn no_threat_model_reports_all_findings() {
        // No threat model configured — all findings reported (same as before)
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn main() {\n    let arg = std::env::args().next();\n    Command::new(arg);\n}\n"
                .into(),
        );
        let ctx = make_ctx(files, Language::Rust); // default = no threat model
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "without threat model, all findings reported"
        );
    }

    #[tokio::test]
    async fn cli_tool_does_not_suppress_socket_input() {
        // CLI tool with socket input — untrusted even for CLI
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app.py"),
            "def run():\n    data = socket.recv(1024)\n    subprocess.call(data, shell=True)\n"
                .into(),
        );
        let tm = apex_core::config::ThreatModelConfig {
            model_type: Some(apex_core::config::ThreatModelType::CliTool),
            trusted_sources: vec![],
            untrusted_sources: vec![],
        };
        let ctx = make_ctx_with_threat_model(files, Language::Python, tm);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "socket input is untrusted even for CLI tools"
        );
    }

    #[tokio::test]
    async fn user_override_trusted_suppresses() {
        // Web service but user explicitly trusts "request" — suppressed
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app.py"),
            "def run():\n    cmd = request.args.get('cmd')\n    subprocess.call(cmd, shell=True)\n"
                .into(),
        );
        let tm = apex_core::config::ThreatModelConfig {
            model_type: Some(apex_core::config::ThreatModelType::WebService),
            trusted_sources: vec!["request".into()],
            untrusted_sources: vec![],
        };
        let ctx = make_ctx_with_threat_model(files, Language::Python, tm);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "user-override trusted 'request' should suppress, got {findings:?}"
        );
    }

    #[test]
    fn collect_matched_indicators_returns_matches() {
        let lines = vec![
            "def run():",
            "    cmd = request.args.get('cmd')",
            "    subprocess.call(cmd, shell=True)",
        ];
        let indicators = &["request", "argv", "input"];
        let matched = collect_matched_indicators(&lines, 2, indicators, 3);
        assert_eq!(matched, vec!["request"]);
    }

    #[test]
    fn collect_matched_indicators_deduplicates() {
        let lines = vec![
            "request = get_request()",
            "x = request.args",
            "subprocess.call(x)",
        ];
        let indicators = &["request"];
        let matched = collect_matched_indicators(&lines, 2, indicators, 3);
        assert_eq!(matched.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Bug 1/13 (CWE-134): printf with literal format string should NOT trigger
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn c_printf_with_literal_format_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "void log_val(int x) {\n    printf(\"Value: %d\\n\", x);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "printf with literal format string should not trigger CWE-134, got {findings:?}"
        );
    }

    #[tokio::test]
    async fn c_printf_with_variable_is_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "void log_user(char *buf) {\n    printf(buf);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "printf(buf) with user-controlled buffer should trigger CWE-134"
        );
        assert!(findings[0].cwe_ids.contains(&134));
    }

    #[tokio::test]
    async fn cpp_printf_literal_format_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/util.cpp"),
            "void debug(int x) {\n    printf(\"x=%d\", x);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Cpp);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "printf with literal format in C++ should not trigger, got {findings:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Bug 4 (CWE-94): eval/new Function in compiler/codegen files should be skipped
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn js_eval_in_codegen_file_skipped() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/compiler/codegen.js"),
            "function generate(req) {\n    const code = eval(req.body.template);\n    return code;\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "eval in codegen file should be skipped (legitimate use), got {findings:?}"
        );
    }

    #[tokio::test]
    async fn js_eval_in_non_codegen_file_reported() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/handler.js"),
            "function handle(req) {\n    eval(req.body.expr);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "eval in non-codegen file should still be reported"
        );
    }

    // -----------------------------------------------------------------------
    // Bug 9: Ruby class_eval/instance_eval with schema/migration context — suppressed
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ruby_class_eval_in_migration_downgraded() {
        // class_eval with define_method (metaprogramming / migration context):
        // No user-input indicators match (no params/request/input) → severity downgraded.
        // define_method is a sanitization indicator → downgraded a second time.
        // Net result: Critical → High → Medium.  No user input = not a real injection threat.
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("db/migrate/add_user_table.rb"),
            "ActiveRecord::Migration.class_eval do\n  define_method(:up) { add_column :users, :name, :string }\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        // Sanitization indicators suppress user-input-free class_eval to Medium or below
        for f in &findings {
            assert!(
                f.severity <= Severity::Medium,
                "class_eval in migration context should be downgraded to Medium or lower, got {:?}",
                f.severity
            );
        }
    }

    #[tokio::test]
    async fn ruby_class_eval_with_user_params_is_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/services/eval_service.rb"),
            "def exec_user(params)\n  Klass.class_eval(params[:code])\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "class_eval with user params should trigger"
        );
    }

    // -----------------------------------------------------------------------
    // Bug 10: Python pickle with trusted/internal/cache context — suppressed
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn python_pickle_with_trusted_cache_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/cache.py"),
            "def load_cache(path):\n    # Load trusted internal cache file\n    with open(path, 'rb') as f:\n        return pickle.load(f)  # internal, trusted\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        // "trusted" and "internal" are sanitization indicators — should suppress or downgrade
        assert!(
            findings.is_empty() || findings[0].severity <= Severity::Medium,
            "pickle.load with trusted/internal context should be suppressed or downgraded"
        );
    }

    // -----------------------------------------------------------------------
    // Bug 7: C# new Process / SqlConnection / Deserialize< patterns
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn csharp_new_process_with_user_input() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Controller.cs"),
            "public void Run(string input) {\n    var p = new Process();\n    p.StartInfo.FileName = input;\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "new Process with user input should trigger");
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn csharp_deserialize_with_from_body() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ApiController.cs"),
            "public IActionResult Post([FromBody] string body) {\n    var obj = JsonSerializer.Deserialize<UserModel>(body);\n    return Ok(obj);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "Deserialize< with [FromBody] should trigger");
    }

    // -----------------------------------------------------------------------
    // Bug 8: Kotlin patterns
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn kotlin_process_builder_with_user_input() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Exec.kt"),
            "fun run(request: Request): String {\n    val pb = ProcessBuilder(request.params[\"cmd\"])\n    return pb.start().toString()\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "Kotlin ProcessBuilder with user input should trigger");
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn kotlin_gson_deserialize_with_user_body() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Handler.kt"),
            "fun handle(body: String): Any {\n    return Gson().fromJson(body, Any::class.java)\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "Kotlin Gson().fromJson with user body should trigger");
    }
}
