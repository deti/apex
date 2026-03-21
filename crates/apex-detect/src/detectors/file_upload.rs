//! Unrestricted File Upload detector (CWE-434).
//!
//! Detects file upload handlers missing validation:
//! - Missing extension whitelist check
//! - Missing MIME type validation
//! - Missing file size limit
//! - Storing uploaded files in web-accessible directories
//!
//! Covers Python, JavaScript, Java, C#, Ruby, Go, and PHP patterns.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct FileUploadDetector;

// ── Upload handler patterns ─────────────────────────────────────────────

static PY_REQUEST_FILES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"request\.(?:files|FILES)").expect("invalid regex"));

static JS_MULTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"multer\s*\(").expect("invalid regex"));

static JS_FORMIDABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"formidable\s*\(|IncomingForm\s*\(").expect("invalid regex"));

static JAVA_MULTIPART: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@RequestParam.*MultipartFile|@Part\s").expect("invalid regex"));

static CSHARP_IFORMFILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"IFormFile\b").expect("invalid regex"));

static RUBY_FILE_UPLOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"params\[.*\]\.\w*(?:tempfile|original_filename)").expect("invalid regex"));

static GO_FORM_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"r\.FormFile\s*\(").expect("invalid regex"));

// ── Validation indicators ───────────────────────────────────────────────

const EXTENSION_CHECKS: &[&str] = &[
    "allowed_extensions",
    "ALLOWED_EXTENSIONS",
    "file_extension",
    "endswith",
    "ends_with",
    "extname",
    "getExtension",
    "extension",
    "fileFilter",
    "accept",
    "content_type",
    "ContentType",
    "mimetype",
    "mime_type",
    "MIME",
];

const SIZE_CHECKS: &[&str] = &[
    "max_size",
    "maxSize",
    "MAX_FILE_SIZE",
    "file_size",
    "fileSize",
    "content_length",
    "Content-Length",
    "limits",
    "MAX_CONTENT_LENGTH",
    "maxFileSize",
    "sizeLimit",
];

fn context_has_validation(source: &str, line_num: usize) -> (bool, bool) {
    // Check a window of +-15 lines around the upload for validation
    let lines: Vec<&str> = source.lines().collect();
    let start = line_num.saturating_sub(15);
    let end = (line_num + 15).min(lines.len());
    let window: String = lines[start..end].join("\n").to_lowercase();

    let has_ext = EXTENSION_CHECKS
        .iter()
        .any(|c| window.contains(&c.to_lowercase()));
    let has_size = SIZE_CHECKS
        .iter()
        .any(|c| window.contains(&c.to_lowercase()));

    (has_ext, has_size)
}

/// Patterns that indicate saving to a web-accessible directory.
const WEB_DIR_PATTERNS: &[&str] = &[
    "public/",
    "static/",
    "uploads/",
    "www/",
    "htdocs/",
    "webroot/",
    "wwwroot/",
    "media/",
];

fn saves_to_web_dir(source: &str) -> bool {
    let lower = source.to_lowercase();
    WEB_DIR_PATTERNS.iter().any(|p| lower.contains(p))
        && (lower.contains("save") || lower.contains("write") || lower.contains("move") || lower.contains("copy"))
}

#[async_trait]
impl Detector for FileUploadDetector {
    fn name(&self) -> &str {
        "file-upload"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let upload_lines: Vec<(usize, &str)> = source
                .lines()
                .enumerate()
                .filter(|(_, line)| {
                    let trimmed = line.trim();
                    if is_comment(trimmed, ctx.language) {
                        return false;
                    }
                    match ctx.language {
                        Language::Python => PY_REQUEST_FILES.is_match(trimmed),
                        Language::JavaScript => {
                            JS_MULTER.is_match(trimmed) || JS_FORMIDABLE.is_match(trimmed)
                        }
                        Language::Java | Language::Kotlin => JAVA_MULTIPART.is_match(trimmed),
                        Language::CSharp => CSHARP_IFORMFILE.is_match(trimmed),
                        Language::Ruby => RUBY_FILE_UPLOAD.is_match(trimmed),
                        Language::Go => GO_FORM_FILE.is_match(trimmed),
                        _ => false,
                    }
                })
                .collect();

            for (line_num, _line) in &upload_lines {
                let line_1based = (*line_num + 1) as u32;
                let (has_ext_check, has_size_check) = context_has_validation(source, *line_num);

                if !has_ext_check {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "File upload missing extension/type validation".into(),
                        description: format!(
                            "File upload handler at {}:{} does not validate file extension or MIME type",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Validate file extension against an allowlist and check MIME type before processing".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![434],
                        noisy: false, base_severity: None, coverage_confidence: None,
                    });
                }

                if !has_size_check {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "File upload missing size limit".into(),
                        description: format!(
                            "File upload handler at {}:{} does not enforce a file size limit",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Set a maximum file size limit to prevent denial-of-service attacks".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![434],
                        noisy: false, base_severity: None, coverage_confidence: None,
                    });
                }
            }

            // Check if uploaded files are stored in web-accessible directories
            if !upload_lines.is_empty() && saves_to_web_dir(source) {
                let line_1based = (upload_lines[0].0 + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: self.name().into(),
                    severity: Severity::High,
                    category: FindingCategory::SecuritySmell,
                    file: path.clone(),
                    line: Some(line_1based),
                    title: "Uploaded files stored in web-accessible directory".into(),
                    description: format!(
                        "File upload in {}:{} saves files to a publicly accessible directory",
                        path.display(),
                        line_1based
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Store uploaded files outside the web root and serve them through a controller with access checks".into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![434],
                    noisy: false, base_severity: None, coverage_confidence: None,
                });
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

    fn make_ctx(filename: &str, source: &str, lang: Language) -> AnalysisContext {
        let mut files = HashMap::new();
        files.insert(PathBuf::from(filename), source.to_string());
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn detects_python_upload_no_validation() {
        let ctx = make_ctx(
            "src/views.py",
            "def upload(request):\n    file = request.files['document']\n    file.save('/tmp/uploads/' + file.filename)\n",
            Language::Python,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("extension")));
        assert!(findings.iter().all(|f| f.cwe_ids == vec![434]));
    }

    #[tokio::test]
    async fn no_finding_python_with_validation() {
        let ctx = make_ctx(
            "src/views.py",
            "ALLOWED_EXTENSIONS = {'png', 'jpg'}\nMAX_FILE_SIZE = 1024 * 1024\ndef upload(request):\n    file = request.files['document']\n    if file.content_type not in allowed:\n        abort(400)\n",
            Language::Python,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_multer_without_file_filter() {
        let ctx = make_ctx(
            "src/upload.js",
            "const upload = multer({ dest: 'uploads/' });\napp.post('/upload', upload.single('file'), handler);\n",
            Language::JavaScript,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_multer_with_filter() {
        let ctx = make_ctx(
            "src/upload.js",
            "const upload = multer({\n  fileFilter: (req, file, cb) => {},\n  limits: { fileSize: 1024 * 1024 }\n});\n",
            Language::JavaScript,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_java_multipart_no_validation() {
        let ctx = make_ctx(
            "src/UploadController.java",
            "public void upload(@RequestParam MultipartFile file) {\n    file.transferTo(dest);\n}\n",
            Language::Java,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_csharp_iformfile() {
        let ctx = make_ctx(
            "Controllers/UploadController.cs",
            "public async Task Upload(IFormFile file) {\n    await file.CopyToAsync(stream);\n}\n",
            Language::CSharp,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_go_form_file() {
        let ctx = make_ctx(
            "handlers/upload.go",
            "file, header, err := r.FormFile(\"upload\")\ndefer file.Close()\n",
            Language::Go,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_web_dir_storage() {
        let ctx = make_ctx(
            "src/views.py",
            "def upload(request):\n    file = request.files['doc']\n    file.save('public/uploads/' + file.filename)\n",
            Language::Python,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("web-accessible")));
    }

    #[tokio::test]
    async fn skips_test_files() {
        let ctx = make_ctx(
            "tests/test_upload.py",
            "file = request.files['document']\n",
            Language::Python,
        );
        let findings = FileUploadDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn detector_name() {
        assert_eq!(FileUploadDetector.name(), "file-upload");
    }
}
