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
}
