use serde::{Deserialize, Serialize};

/// Language-agnostic representation of a branch condition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionTree {
    Compare {
        left: Box<Expr>,
        op: CompareOp,
        right: Box<Expr>,
    },
    And(Box<ConditionTree>, Box<ConditionTree>),
    Or(Box<ConditionTree>, Box<ConditionTree>),
    Not(Box<ConditionTree>),
    TypeCheck {
        expr: Box<Expr>,
        type_name: String,
    },
    Contains {
        needle: Box<Expr>,
        haystack: Box<Expr>,
    },
    NullCheck {
        expr: Box<Expr>,
        is_null: bool,
    },
    LengthCheck {
        expr: Box<Expr>,
        op: CompareOp,
        value: Box<Expr>,
    },
    Unknown(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Expr {
    Variable(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BoolLiteral(bool),
    Null,
    PropertyAccess { object: Box<Expr>, property: String },
    Call(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl std::fmt::Display for CompareOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompareOp::Eq => write!(f, "=="),
            CompareOp::NotEq => write!(f, "!="),
            CompareOp::Lt => write!(f, "<"),
            CompareOp::LtEq => write!(f, "<="),
            CompareOp::Gt => write!(f, ">"),
            CompareOp::GtEq => write!(f, ">="),
        }
    }
}

impl Expr {
    /// Render the expression as a human-readable source string.
    pub fn to_source(&self) -> String {
        match self {
            Expr::Variable(name) => name.clone(),
            Expr::IntLiteral(n) => n.to_string(),
            Expr::FloatLiteral(f) => f.to_string(),
            Expr::StringLiteral(s) => format!("\"{s}\""),
            Expr::BoolLiteral(b) => b.to_string(),
            Expr::Null => "null".to_string(),
            Expr::PropertyAccess { object, property } => {
                format!("{}.{}", object.to_source(), property)
            }
            Expr::Call(name) => format!("{name}()"),
        }
    }
}

impl ConditionTree {
    /// Render the condition tree as a human-readable source constraint.
    ///
    /// Produces output like `x > 10 and name is not null` instead of
    /// SMTLIB2 `(and (> x 10) (not (= name nil)))`.
    pub fn to_source_constraint(&self) -> String {
        match self {
            ConditionTree::Compare { left, op, right } => {
                format!("{} {} {}", left.to_source(), op, right.to_source())
            }
            ConditionTree::And(a, b) => {
                format!(
                    "{} and {}",
                    a.to_source_constraint(),
                    b.to_source_constraint()
                )
            }
            ConditionTree::Or(a, b) => {
                format!(
                    "{} or {}",
                    a.to_source_constraint(),
                    b.to_source_constraint()
                )
            }
            ConditionTree::Not(inner) => {
                format!("not ({})", inner.to_source_constraint())
            }
            ConditionTree::TypeCheck { expr, type_name } => {
                format!("{} is {}", expr.to_source(), type_name)
            }
            ConditionTree::Contains { needle, haystack } => {
                format!("{} contains {}", haystack.to_source(), needle.to_source())
            }
            ConditionTree::NullCheck { expr, is_null } => {
                if *is_null {
                    format!("{} is null", expr.to_source())
                } else {
                    format!("{} is not null", expr.to_source())
                }
            }
            ConditionTree::LengthCheck { expr, op, value } => {
                format!("len({}) {} {}", expr.to_source(), op, value.to_source())
            }
            ConditionTree::Unknown(text) => text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_roundtrip() {
        let cond = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let json = serde_json::to_string(&cond).unwrap();
        let back: ConditionTree = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, back);
    }

    #[test]
    fn and_or_not() {
        let a = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let b = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Lt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        let combined = ConditionTree::And(Box::new(a), Box::new(b));
        let negated = ConditionTree::Not(Box::new(combined));
        assert!(matches!(negated, ConditionTree::Not(_)));
    }

    #[test]
    fn null_check() {
        let cond = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("result".into())),
            is_null: true,
        };
        assert!(matches!(
            cond,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn type_check() {
        let cond = ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable("err".into())),
            type_name: "Error".into(),
        };
        assert!(matches!(cond, ConditionTree::TypeCheck { .. }));
    }

    #[test]
    fn compare_op_display() {
        assert_eq!(CompareOp::Eq.to_string(), "==");
        assert_eq!(CompareOp::NotEq.to_string(), "!=");
        assert_eq!(CompareOp::Lt.to_string(), "<");
        assert_eq!(CompareOp::GtEq.to_string(), ">=");
    }

    #[test]
    fn unknown_preserves_text() {
        let cond = ConditionTree::Unknown("some complex expr".into());
        if let ConditionTree::Unknown(text) = cond {
            assert_eq!(text, "some complex expr");
        } else {
            panic!("expected Unknown");
        }
    }

    // ------------------------------------------------------------------
    // to_source_constraint tests
    // ------------------------------------------------------------------

    #[test]
    fn source_constraint_compare() {
        let cond = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        assert_eq!(cond.to_source_constraint(), "x > 10");
    }

    #[test]
    fn source_constraint_and() {
        let a = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        let b = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("name".into())),
            is_null: false,
        };
        let combined = ConditionTree::And(Box::new(a), Box::new(b));
        assert_eq!(combined.to_source_constraint(), "x > 10 and name is not null");
    }

    #[test]
    fn source_constraint_or() {
        let a = ConditionTree::Compare {
            left: Box::new(Expr::Variable("a".into())),
            op: CompareOp::Eq,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let b = ConditionTree::Compare {
            left: Box::new(Expr::Variable("b".into())),
            op: CompareOp::Eq,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let cond = ConditionTree::Or(Box::new(a), Box::new(b));
        assert_eq!(cond.to_source_constraint(), "a == 0 or b == 0");
    }

    #[test]
    fn source_constraint_not() {
        let inner = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Lt,
            right: Box::new(Expr::IntLiteral(5)),
        };
        let cond = ConditionTree::Not(Box::new(inner));
        assert_eq!(cond.to_source_constraint(), "not (x < 5)");
    }

    #[test]
    fn source_constraint_type_check() {
        let cond = ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable("err".into())),
            type_name: "ValueError".into(),
        };
        assert_eq!(cond.to_source_constraint(), "err is ValueError");
    }

    #[test]
    fn source_constraint_contains() {
        let cond = ConditionTree::Contains {
            needle: Box::new(Expr::StringLiteral("admin".into())),
            haystack: Box::new(Expr::Variable("users".into())),
        };
        assert_eq!(cond.to_source_constraint(), "users contains \"admin\"");
    }

    #[test]
    fn source_constraint_null_check() {
        let null = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("result".into())),
            is_null: true,
        };
        assert_eq!(null.to_source_constraint(), "result is null");

        let not_null = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("result".into())),
            is_null: false,
        };
        assert_eq!(not_null.to_source_constraint(), "result is not null");
    }

    #[test]
    fn source_constraint_length_check() {
        let cond = ConditionTree::LengthCheck {
            expr: Box::new(Expr::Variable("items".into())),
            op: CompareOp::GtEq,
            value: Box::new(Expr::IntLiteral(3)),
        };
        assert_eq!(cond.to_source_constraint(), "len(items) >= 3");
    }

    #[test]
    fn source_constraint_unknown() {
        let cond = ConditionTree::Unknown("complex_expr()".into());
        assert_eq!(cond.to_source_constraint(), "complex_expr()");
    }

    #[test]
    fn expr_to_source_all_variants() {
        assert_eq!(Expr::Variable("x".into()).to_source(), "x");
        assert_eq!(Expr::IntLiteral(42).to_source(), "42");
        assert_eq!(Expr::FloatLiteral(3.14).to_source(), "3.14");
        assert_eq!(Expr::StringLiteral("hi".into()).to_source(), "\"hi\"");
        assert_eq!(Expr::BoolLiteral(true).to_source(), "true");
        assert_eq!(Expr::Null.to_source(), "null");
        assert_eq!(Expr::Call("foo".into()).to_source(), "foo()");
    }

    #[test]
    fn expr_to_source_property_access() {
        let expr = Expr::PropertyAccess {
            object: Box::new(Expr::Variable("obj".into())),
            property: "field".into(),
        };
        assert_eq!(expr.to_source(), "obj.field");
    }

    #[test]
    fn source_constraint_nested_and_or() {
        let a = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let b = ConditionTree::Compare {
            left: Box::new(Expr::Variable("y".into())),
            op: CompareOp::Lt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        let c = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("z".into())),
            is_null: false,
        };
        let and_ab = ConditionTree::And(Box::new(a), Box::new(b));
        let or_with_c = ConditionTree::Or(Box::new(and_ab), Box::new(c));
        assert_eq!(
            or_with_c.to_source_constraint(),
            "x > 0 and y < 10 or z is not null"
        );
    }
}
