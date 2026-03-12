//! Function summaries for Python stdlib functions.
//!
//! Each summary describes a function's arity and provides a `generate`
//! callback that produces SMTLIB2 constraints modelling the function's
//! observable behaviour.  These constraints are injected into the
//! symbolic session when the concolic tracer encounters a call to a
//! summarised function.

use std::collections::HashMap;
use std::sync::LazyLock;

/// A symbolic summary of a Python function.
pub struct FunctionSummary {
    /// Fully-qualified name (e.g. `"builtins.len"`).
    pub name: &'static str,
    /// Expected number of arguments (-1 for variadic).
    pub arity: i32,
    /// Generate SMTLIB2 constraints given the return-value variable name and
    /// argument variable names.
    generate_fn: fn(ret: &str, args: &[&str]) -> Vec<String>,
}

impl FunctionSummary {
    /// Produce SMTLIB2 constraints for this function.
    pub fn generate(&self, ret: &str, args: &[&str]) -> Vec<String> {
        (self.generate_fn)(ret, args)
    }
}

// ---------------------------------------------------------------------------
// Summary registry
// ---------------------------------------------------------------------------

/// Look up a function summary by fully-qualified name.
pub fn lookup(name: &str) -> Option<&'static FunctionSummary> {
    SUMMARIES.get(name).copied()
}

static SUMMARIES: LazyLock<HashMap<&'static str, &'static FunctionSummary>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    for s in ALL_SUMMARIES.iter() {
        m.insert(s.name, s);
    }
    m
});

static ALL_SUMMARIES: &[FunctionSummary] = &[
    // builtins.len: len(x) >= 0
    FunctionSummary {
        name: "builtins.len",
        arity: 1,
        generate_fn: |ret, _args| vec![format!("(>= {} 0)", ret)],
    },
    // builtins.range: range(n) — result count >= 0
    FunctionSummary {
        name: "builtins.range",
        arity: -1,
        generate_fn: |ret, _args| vec![format!("(>= {} 0)", ret)],
    },
    // builtins.int: int(x) — no constraints (any integer)
    FunctionSummary {
        name: "builtins.int",
        arity: 1,
        generate_fn: |_ret, _args| Vec::new(),
    },
    // builtins.str: str(x) — no constraints
    FunctionSummary {
        name: "builtins.str",
        arity: 1,
        generate_fn: |_ret, _args| Vec::new(),
    },
    // builtins.max: max(a, b) => ret >= a AND ret >= b
    FunctionSummary {
        name: "builtins.max",
        arity: -1,
        generate_fn: |ret, args| args.iter().map(|a| format!("(>= {ret} {a})")).collect(),
    },
    // builtins.min: min(a, b) => ret <= a AND ret <= b
    FunctionSummary {
        name: "builtins.min",
        arity: -1,
        generate_fn: |ret, args| args.iter().map(|a| format!("(<= {ret} {a})")).collect(),
    },
    // builtins.abs: abs(x) => ret >= 0 AND (ret >= x) (since ret = |x|)
    FunctionSummary {
        name: "builtins.abs",
        arity: 1,
        generate_fn: |ret, args| {
            let mut v = vec![format!("(>= {ret} 0)")];
            if let Some(a) = args.first() {
                v.push(format!("(>= {ret} {a})"));
            }
            v
        },
    },
    // str.split: result length >= 1
    FunctionSummary {
        name: "str.split",
        arity: -1,
        generate_fn: |ret, _args| vec![format!("(>= {} 1)", ret)],
    },
    // dict.get: no constraints (could return default)
    FunctionSummary {
        name: "dict.get",
        arity: -1,
        generate_fn: |_ret, _args| Vec::new(),
    },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_len_exists() {
        let s = lookup("builtins.len").expect("builtins.len should exist");
        assert_eq!(s.name, "builtins.len");
        assert_eq!(s.arity, 1);
    }

    #[test]
    fn lookup_range_exists() {
        let s = lookup("builtins.range").expect("builtins.range should exist");
        assert_eq!(s.name, "builtins.range");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("nonexistent.function").is_none());
    }

    #[test]
    fn len_summary_generates_constraint() {
        let s = lookup("builtins.len").unwrap();
        let constraints = s.generate("ret", &["xs"]);
        assert_eq!(constraints.len(), 1);
        assert!(
            constraints[0].contains(">="),
            "expected >= in: {}",
            constraints[0]
        );
    }

    #[test]
    fn int_summary_no_constraints() {
        let s = lookup("builtins.int").unwrap();
        let constraints = s.generate("ret", &["x"]);
        assert!(constraints.is_empty());
    }

    #[test]
    fn max_summary_generates_gte() {
        let s = lookup("builtins.max").unwrap();
        let constraints = s.generate("ret", &["a", "b"]);
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains(">="));
        assert!(constraints[1].contains(">="));
    }

    #[test]
    fn min_summary_generates_lte() {
        let s = lookup("builtins.min").unwrap();
        let constraints = s.generate("ret", &["a", "b"]);
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains("<="));
        assert!(constraints[1].contains("<="));
    }

    #[test]
    fn range_summary_generates_constraint() {
        let s = lookup("builtins.range").unwrap();
        let constraints = s.generate("ret", &["n"]);
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains(">="));
    }

    #[test]
    fn str_summary_no_constraints() {
        let s = lookup("builtins.str").unwrap();
        let constraints = s.generate("ret", &["x"]);
        assert!(constraints.is_empty());
    }

    #[test]
    fn abs_summary_generates_two_constraints() {
        let s = lookup("builtins.abs").unwrap();
        let constraints = s.generate("ret", &["x"]);
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains(">="));
        assert!(constraints[1].contains(">="));
    }

    #[test]
    fn abs_summary_no_args() {
        let s = lookup("builtins.abs").unwrap();
        let constraints = s.generate("ret", &[]);
        assert_eq!(constraints.len(), 1); // only (>= ret 0), no arg constraint
    }

    #[test]
    fn split_summary_generates_gte_one() {
        let s = lookup("str.split").unwrap();
        let constraints = s.generate("ret", &[]);
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains(">= "));
    }

    #[test]
    fn dict_get_no_constraints() {
        let s = lookup("dict.get").unwrap();
        let constraints = s.generate("ret", &["key"]);
        assert!(constraints.is_empty());
    }

    #[test]
    fn max_summary_no_args_empty() {
        let s = lookup("builtins.max").unwrap();
        let constraints = s.generate("ret", &[]);
        assert!(constraints.is_empty());
    }

    #[test]
    fn max_summary_three_args() {
        let s = lookup("builtins.max").unwrap();
        let constraints = s.generate("ret", &["a", "b", "c"]);
        assert_eq!(constraints.len(), 3);
        for c in &constraints {
            assert!(c.contains(">="));
        }
    }

    #[test]
    fn min_summary_three_args() {
        let s = lookup("builtins.min").unwrap();
        let constraints = s.generate("ret", &["a", "b", "c"]);
        assert_eq!(constraints.len(), 3);
        for c in &constraints {
            assert!(c.contains("<="));
        }
    }

    #[test]
    fn abs_summary_two_args() {
        let s = lookup("builtins.abs").unwrap();
        // abs with multiple args - only first arg used for second constraint
        let constraints = s.generate("ret", &["x", "y"]);
        assert_eq!(constraints.len(), 2);
        assert!(constraints[1].contains("x"));
    }

    #[test]
    fn range_summary_arity_is_variadic() {
        let s = lookup("builtins.range").unwrap();
        assert_eq!(s.arity, -1);
    }

    #[test]
    fn split_summary_arity_is_variadic() {
        let s = lookup("str.split").unwrap();
        assert_eq!(s.arity, -1);
    }

    #[test]
    fn len_summary_ret_name_used() {
        let s = lookup("builtins.len").unwrap();
        let constraints = s.generate("my_ret", &["xs"]);
        assert!(constraints[0].contains("my_ret"));
    }

    #[test]
    fn all_summaries_have_unique_names() {
        let mut names = std::collections::HashSet::new();
        for s in super::ALL_SUMMARIES.iter() {
            assert!(names.insert(s.name), "duplicate summary name: {}", s.name);
        }
    }

    #[test]
    fn min_summary_single_arg() {
        let s = lookup("builtins.min").unwrap();
        let constraints = s.generate("ret", &["x"]);
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("<="));
    }
}
