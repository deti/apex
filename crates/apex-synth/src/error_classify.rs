//! Error classifier for LLM refinement routing.
//!
//! Classifies test execution errors by kind so the refinement prompt can
//! give targeted feedback instead of generic "fix the error" messages.

/// Classification of a test execution error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Import,
    Syntax,
    Assertion,
    Runtime,
    Unknown,
}

/// Classify a test error message into an ErrorKind.
pub fn classify_test_error(error: &str) -> ErrorKind {
    let lower = error.to_lowercase();
    if lower.contains("importerror")
        || lower.contains("modulenotfounderror")
        || lower.contains("no module named")
    {
        ErrorKind::Import
    } else if lower.contains("syntaxerror") || lower.contains("invalid syntax") {
        ErrorKind::Syntax
    } else if lower.contains("assertionerror") || (lower.contains("assert") && lower.contains("fail"))
    {
        ErrorKind::Assertion
    } else if lower.contains("error") || lower.contains("exception") {
        ErrorKind::Runtime
    } else {
        ErrorKind::Unknown
    }
}

/// Generate a targeted refinement prompt based on the error kind.
pub fn refinement_prompt(kind: ErrorKind, error: &str) -> String {
    match kind {
        ErrorKind::Import => format!(
            "The test has an import error: {error}. \
             Fix the import statement — use only modules available in the project."
        ),
        ErrorKind::Syntax => format!(
            "The test has a syntax error: {error}. \
             Fix the syntax and ensure the code is valid."
        ),
        ErrorKind::Assertion => format!(
            "The test assertion failed: {error}. \
             Check the expected values and adjust the assertion."
        ),
        ErrorKind::Runtime => format!(
            "The test raised a runtime error: {error}. \
             Fix the test to avoid this exception."
        ),
        ErrorKind::Unknown => format!(
            "The test yielded an error: {error}. \
             Modify the test to fix it."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_import_error() {
        let err = "ModuleNotFoundError: No module named 'foo'";
        assert_eq!(classify_test_error(err), ErrorKind::Import);
    }

    #[test]
    fn classify_syntax_error() {
        let err = "SyntaxError: invalid syntax\n  File \"test.py\", line 5";
        assert_eq!(classify_test_error(err), ErrorKind::Syntax);
    }

    #[test]
    fn classify_assertion_error() {
        let err = "AssertionError: expected 1, got 2";
        assert_eq!(classify_test_error(err), ErrorKind::Assertion);
    }

    #[test]
    fn classify_runtime_error() {
        let err = "ZeroDivisionError: division by zero";
        assert_eq!(classify_test_error(err), ErrorKind::Runtime);
    }

    #[test]
    fn classify_unknown() {
        let err = "something went wrong";
        assert_eq!(classify_test_error(err), ErrorKind::Unknown);
    }

    #[test]
    fn refinement_prompt_import() {
        let msg = refinement_prompt(ErrorKind::Import, "ModuleNotFoundError: no 'foo'");
        assert!(msg.contains("import"));
    }

    #[test]
    fn refinement_prompt_syntax() {
        let msg = refinement_prompt(ErrorKind::Syntax, "SyntaxError: bad");
        assert!(msg.contains("syntax"));
    }
}
