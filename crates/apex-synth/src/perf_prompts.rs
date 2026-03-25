//! Performance-aware LLM synthesis prompts.
//!
//! Provides prompt strategies that instruct the LLM to generate worst-case
//! inputs, ReDoS proof-of-concept tests, and SLO verification tests.

use apex_core::types::Language;

// ---------------------------------------------------------------------------
// ComplexityClass
// ---------------------------------------------------------------------------

/// Algorithmic complexity class for a function under test.
///
/// Used to guide the LLM toward inputs that trigger worst-case behaviour for
/// a specific complexity profile (e.g. O(n²) requires large `n`; O(2^n)
/// requires exponential branching).
///
/// This type mirrors the `ComplexityClass` planned in `apex-core` (task 1.1).
/// Once the foundation crew lands that type this module will re-export it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityClass {
    /// O(1) — constant time, no worst-case scaling concern.
    Constant,
    /// O(log n) — logarithmic.
    Logarithmic,
    /// O(n) — linear.
    Linear,
    /// O(n log n) — linearithmic.
    Linearithmic,
    /// O(n²) — quadratic; a large input will expose the bottleneck.
    Quadratic,
    /// O(n³) — cubic.
    Cubic,
    /// O(2^n) — exponential; even moderate inputs can stall execution.
    Exponential,
    /// Complexity class could not be determined.
    Unknown,
}

impl ComplexityClass {
    /// Return a human-readable description for prompt injection.
    fn description(self) -> &'static str {
        match self {
            ComplexityClass::Constant => "O(1) constant",
            ComplexityClass::Logarithmic => "O(log n) logarithmic",
            ComplexityClass::Linear => "O(n) linear",
            ComplexityClass::Linearithmic => "O(n log n) linearithmic",
            ComplexityClass::Quadratic => "O(n²) quadratic",
            ComplexityClass::Cubic => "O(n³) cubic",
            ComplexityClass::Exponential => "O(2^n) exponential",
            ComplexityClass::Unknown => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// worst_case_prompt
// ---------------------------------------------------------------------------

/// Generate a prompt for worst-case input generation.
///
/// Returns `(system_message, user_message)`.
///
/// The system message establishes the LLM as a performance testing expert.
/// The user message includes the source code, an optional complexity hint,
/// and asks for a test that demonstrates worst-case behaviour with timing
/// assertions.
pub fn worst_case_prompt(
    function_name: &str,
    source_segment: &str,
    language: Language,
    complexity_hint: Option<ComplexityClass>,
) -> (String, String) {
    let system = "You are a performance testing expert. \
                  Generate an input that maximizes execution time or memory \
                  consumption for the given function."
        .to_string();

    let complexity_section = match complexity_hint {
        Some(cls) => format!(
            "\n\nComplexity hint: This function is believed to have {} complexity. \
             Choose an input size and structure that exploits this.",
            cls.description()
        ),
        None => String::new(),
    };

    let lang_name = language.to_string();

    let user = format!(
        "Function: `{function_name}`\nLanguage: {lang_name}{complexity_section}\n\n\
         Source:\n```{lang_name}\n{source_segment}\n```\n\n\
         Write a {lang_name} test that:\n\
         1. Constructs or selects a worst-case input for `{function_name}`.\n\
         2. Calls the function with that input.\n\
         3. Measures elapsed wall-clock time.\n\
         4. Asserts that the measured time is notably larger than a baseline \
            (small-input) call, demonstrating the worst-case growth.",
    );

    (system, user)
}

// ---------------------------------------------------------------------------
// redos_prompt
// ---------------------------------------------------------------------------

/// Generate a prompt for a ReDoS proof-of-concept test.
///
/// Returns `(system_message, user_message)`.
///
/// The system message establishes the LLM as a ReDoS security expert.
/// The user message includes the regex pattern and asks for a test that
/// demonstrates catastrophic backtracking using a pump string.
pub fn redos_prompt(
    regex_pattern: &str,
    file_path: &str,
    line: u32,
    language: Language,
) -> (String, String) {
    let system = "You are a security testing expert specializing in \
                  Regular Expression Denial of Service (ReDoS)."
        .to_string();

    let lang_name = language.to_string();

    let user = format!(
        "A potentially vulnerable regex has been identified.\n\n\
         File: {file_path}:{line}\n\
         Pattern: `{regex_pattern}`\n\
         Language: {lang_name}\n\n\
         Write a {lang_name} test that:\n\
         1. Constructs a pump string that triggers catastrophic backtracking \
            in the pattern `{regex_pattern}`.\n\
         2. Applies the regex to the pump string and measures elapsed time.\n\
         3. Asserts that the match takes longer than a safe baseline input, \
            demonstrating the exponential backtracking behaviour.\n\n\
         The pump string should be a repeating sequence that causes the regex \
         engine to explore an exponential number of paths before rejecting.",
    );

    (system, user)
}

// ---------------------------------------------------------------------------
// slo_verification_prompt
// ---------------------------------------------------------------------------

/// Generate a prompt for SLO verification tests.
///
/// Returns `(system_message, user_message)`.
///
/// The system message establishes the LLM as a performance testing expert.
/// The user message includes the function, SLO parameters, and asks for
/// boundary tests that verify the function meets its latency SLO.
pub fn slo_verification_prompt(
    function_name: &str,
    source_segment: &str,
    slo_latency_ms: u64,
    slo_input_size: &str,
    language: Language,
) -> (String, String) {
    let system = "You are a performance testing expert. \
                  Generate tests that verify a function meets its SLO."
        .to_string();

    let lang_name = language.to_string();

    let user = format!(
        "Function: `{function_name}`\nLanguage: {lang_name}\n\n\
         SLO requirement: must complete within {slo_latency_ms} ms \
         for input size {slo_input_size}.\n\n\
         Source:\n```{lang_name}\n{source_segment}\n```\n\n\
         Write {lang_name} tests that:\n\
         1. Test exactly at the SLO boundary ({slo_input_size} input) — \
            assert the function completes in under {slo_latency_ms} ms.\n\
         2. Test just below the SLO boundary (smaller input) — assert faster \
            completion to confirm sub-linear overhead.\n\
         3. Document the SLO threshold clearly in the test name or a comment \
            so failures are immediately actionable.",
    );

    (system, user)
}

// ---------------------------------------------------------------------------
// timing_wrapper
// ---------------------------------------------------------------------------

/// Generate timing instrumentation code for a language.
///
/// Returns `(before_code, after_code)` snippets that bracket the call under
/// measurement.  The `elapsed` variable is available after `after_code`.
pub fn timing_wrapper(language: Language) -> (&'static str, &'static str) {
    match language {
        Language::Python => (
            "import time; _start = time.perf_counter()",
            "elapsed = time.perf_counter() - _start",
        ),
        Language::JavaScript => (
            "const _start = performance.now();",
            "const elapsed = performance.now() - _start;",
        ),
        Language::Rust => (
            "let _start = std::time::Instant::now();",
            "let elapsed = _start.elapsed();",
        ),
        Language::Go => ("_start := time.Now()", "elapsed := time.Since(_start)"),
        // Fallback for languages without a dedicated wrapper: use a no-op
        // that keeps the API uniform.
        _ => ("", ""),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // worst_case_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn worst_case_prompt_python_valid_messages() {
        let (system, user) = worst_case_prompt(
            "sort_items",
            "def sort_items(lst):\n    return sorted(lst)\n",
            Language::Python,
            None,
        );
        assert!(!system.is_empty(), "system message must not be empty");
        assert!(!user.is_empty(), "user message must not be empty");
        assert!(
            system.contains("performance testing expert"),
            "system should establish expert role, got: {system}"
        );
        assert!(
            user.contains("sort_items"),
            "user message must name the function, got: {user}"
        );
        assert!(
            user.contains("python"),
            "user message must name the language, got: {user}"
        );
        assert!(
            user.contains("worst-case"),
            "user message must mention worst-case, got: {user}"
        );
    }

    #[test]
    fn worst_case_prompt_with_complexity_hint() {
        let (_, user) = worst_case_prompt(
            "bubble_sort",
            "def bubble_sort(lst): ...",
            Language::Python,
            Some(ComplexityClass::Quadratic),
        );
        assert!(
            user.contains("quadratic"),
            "user message should mention the complexity hint, got: {user}"
        );
    }

    #[test]
    fn worst_case_prompt_no_hint_no_complexity_text() {
        let (_, user) = worst_case_prompt("process", "fn process() {}", Language::Rust, None);
        // Without a hint there should be no stray "Complexity hint:" section.
        assert!(
            !user.contains("Complexity hint:"),
            "no hint → no complexity section in prompt, got: {user}"
        );
    }

    #[test]
    fn worst_case_prompt_includes_timing_assertion_instruction() {
        let (_, user) = worst_case_prompt("compute", "def compute(n): ...", Language::Python, None);
        assert!(
            user.contains("elapsed") || user.contains("time"),
            "user message should mention timing measurement, got: {user}"
        );
    }

    // -----------------------------------------------------------------------
    // redos_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn redos_prompt_includes_regex_pattern() {
        let pattern = r"^(a+)+$";
        let (system, user) = redos_prompt(pattern, "src/validator.py", 42, Language::Python);
        assert!(
            user.contains(pattern),
            "user message must embed the regex pattern, got: {user}"
        );
        assert!(
            system.contains("ReDoS"),
            "system message must mention ReDoS, got: {system}"
        );
    }

    #[test]
    fn redos_prompt_mentions_backtracking() {
        let (_, user) = redos_prompt(r"(a|aa)+", "lib/check.js", 10, Language::JavaScript);
        assert!(
            user.contains("backtracking"),
            "user message must mention backtracking, got: {user}"
        );
    }

    #[test]
    fn redos_prompt_includes_file_and_line() {
        let (_, user) = redos_prompt(r"(x+)+", "app/parse.py", 99, Language::Python);
        assert!(
            user.contains("app/parse.py"),
            "user message must include file path, got: {user}"
        );
        assert!(
            user.contains("99"),
            "user message must include line number, got: {user}"
        );
    }

    #[test]
    fn redos_prompt_mentions_pump_string() {
        let (_, user) = redos_prompt(r"(a+)+$", "src/re.py", 5, Language::Python);
        assert!(
            user.contains("pump"),
            "user message should mention a pump string, got: {user}"
        );
    }

    // -----------------------------------------------------------------------
    // slo_verification_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn slo_verification_prompt_includes_latency_threshold() {
        let (system, user) = slo_verification_prompt(
            "fetch_user",
            "async def fetch_user(id): ...",
            200,
            "1000 requests",
            Language::Python,
        );
        assert!(
            user.contains("200"),
            "user message must include the latency threshold, got: {user}"
        );
        assert!(
            system.contains("SLO"),
            "system message must mention SLO, got: {system}"
        );
    }

    #[test]
    fn slo_verification_prompt_includes_function_name() {
        let (_, user) = slo_verification_prompt(
            "process_batch",
            "def process_batch(items): ...",
            500,
            "10000 items",
            Language::Python,
        );
        assert!(
            user.contains("process_batch"),
            "user message must name the function, got: {user}"
        );
    }

    #[test]
    fn slo_verification_prompt_includes_input_size() {
        let (_, user) = slo_verification_prompt(
            "query",
            "fn query(n: usize) {}",
            100,
            "n=10000",
            Language::Rust,
        );
        assert!(
            user.contains("n=10000"),
            "user message must include the SLO input size, got: {user}"
        );
    }

    // -----------------------------------------------------------------------
    // timing_wrapper
    // -----------------------------------------------------------------------

    #[test]
    fn timing_wrapper_python() {
        let (before, after) = timing_wrapper(Language::Python);
        assert!(
            before.contains("perf_counter"),
            "Python before snippet should use perf_counter, got: {before}"
        );
        assert!(
            after.contains("elapsed"),
            "Python after snippet should assign elapsed, got: {after}"
        );
    }

    #[test]
    fn timing_wrapper_javascript() {
        let (before, after) = timing_wrapper(Language::JavaScript);
        assert!(
            before.contains("performance.now()"),
            "JS before snippet should use performance.now(), got: {before}"
        );
        assert!(
            after.contains("elapsed"),
            "JS after snippet should assign elapsed, got: {after}"
        );
    }

    #[test]
    fn timing_wrapper_rust() {
        let (before, after) = timing_wrapper(Language::Rust);
        assert!(
            before.contains("Instant::now()"),
            "Rust before snippet should use Instant::now(), got: {before}"
        );
        assert!(
            after.contains("elapsed"),
            "Rust after snippet should assign elapsed, got: {after}"
        );
    }

    #[test]
    fn timing_wrapper_go() {
        let (before, after) = timing_wrapper(Language::Go);
        assert!(
            before.contains("time.Now()"),
            "Go before snippet should use time.Now(), got: {before}"
        );
        assert!(
            after.contains("time.Since"),
            "Go after snippet should use time.Since, got: {after}"
        );
    }

    #[test]
    fn timing_wrapper_all_languages_return_static_strs() {
        // Verify no language panics and that before/after are consistent.
        for lang in [
            Language::Python,
            Language::JavaScript,
            Language::Rust,
            Language::Go,
            Language::Java,
            Language::C,
            Language::Kotlin,
            Language::Swift,
            Language::Ruby,
            Language::CSharp,
        ] {
            let (before, after) = timing_wrapper(lang);
            // before and after must be valid (possibly empty) static strings.
            let _ = before.len();
            let _ = after.len();
        }
    }

    // -----------------------------------------------------------------------
    // ComplexityClass description
    // -----------------------------------------------------------------------

    #[test]
    fn complexity_class_descriptions_are_non_empty() {
        let classes = [
            ComplexityClass::Constant,
            ComplexityClass::Logarithmic,
            ComplexityClass::Linear,
            ComplexityClass::Linearithmic,
            ComplexityClass::Quadratic,
            ComplexityClass::Cubic,
            ComplexityClass::Exponential,
            ComplexityClass::Unknown,
        ];
        for cls in classes {
            assert!(
                !cls.description().is_empty(),
                "ComplexityClass::{cls:?} description must not be empty"
            );
        }
    }
}
