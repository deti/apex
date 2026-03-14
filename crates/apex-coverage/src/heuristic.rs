use apex_core::types::BranchId;

/// Comparison operation type for branch conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Continuous [0.0, 1.0] heuristic for a branch condition.
/// 1.0 = covered, 0.0 = maximally far from flipping.
#[derive(Debug, Clone)]
pub struct BranchHeuristic {
    pub branch_id: BranchId,
    pub score: f64, // 0.0..=1.0
    pub operand_a: Option<i64>,
    pub operand_b: Option<i64>,
}

/// Normalize a non-negative distance to [0, 1) range.
/// normalize(0) = 0, normalize(inf) -> 1.
pub fn normalize(x: f64) -> f64 {
    x / (x + 1.0)
}

/// Compute branch distance as a [0.0, 1.0] score.
/// 1.0 means the condition is satisfied; closer to 0.0 means farther away.
pub fn branch_distance(op: CmpOp, a: i64, b: i64) -> f64 {
    match op {
        CmpOp::Eq => {
            if a == b {
                1.0
            } else {
                1.0 - normalize((a as f64 - b as f64).abs())
            }
        }
        CmpOp::Ne => {
            if a != b {
                1.0
            } else {
                0.0 // can't be "close" to not-equal
            }
        }
        CmpOp::Lt => {
            if a < b {
                1.0
            } else {
                // Clamp to 0.0 to guard against f64 precision loss on extreme i64 values.
                1.0 - normalize(((a as f64 - b as f64) + 1.0).max(0.0))
            }
        }
        CmpOp::Le => {
            if a <= b {
                1.0
            } else {
                1.0 - normalize((a as f64 - b as f64).max(0.0))
            }
        }
        CmpOp::Gt => {
            if a > b {
                1.0
            } else {
                1.0 - normalize(((b as f64 - a as f64) + 1.0).max(0.0))
            }
        }
        CmpOp::Ge => {
            if a >= b {
                1.0
            } else {
                1.0 - normalize((b as f64 - a as f64).max(0.0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_equality_exact_match() {
        assert_eq!(branch_distance(CmpOp::Eq, 42, 42), 1.0);
    }

    #[test]
    fn distance_equality_close() {
        let d = branch_distance(CmpOp::Eq, 40, 42);
        // |40-42| = 2; normalize(2) = 2/3; score = 1 - 2/3 = 1/3
        assert!((d - 1.0 / 3.0).abs() < 1e-9, "expected ~0.333, got {d}");
        // Closer values should score higher
        let d_closer = branch_distance(CmpOp::Eq, 41, 42);
        assert!(d_closer > d);
    }

    #[test]
    fn distance_equality_far() {
        let d = branch_distance(CmpOp::Eq, 0, 1_000_000);
        assert!(d < 0.01);
    }

    #[test]
    fn distance_less_than_satisfied() {
        assert_eq!(branch_distance(CmpOp::Lt, 5, 10), 1.0);
    }

    #[test]
    fn distance_less_than_boundary() {
        let d = branch_distance(CmpOp::Lt, 10, 10);
        // a-b+1 = 1; normalize(1) = 0.5; score = 1 - 0.5 = 0.5
        assert!((d - 0.5).abs() < 1e-9, "expected 0.5, got {d}");
    }

    #[test]
    fn distance_greater_than_satisfied() {
        assert_eq!(branch_distance(CmpOp::Gt, 10, 5), 1.0);
    }

    #[test]
    fn distance_greater_than_not_satisfied() {
        let d = branch_distance(CmpOp::Gt, 5, 10);
        // b-a+1 = 6; normalize(6) = 6/7; score = 1 - 6/7 = 1/7
        assert!((d - 1.0 / 7.0).abs() < 1e-9, "expected ~0.143, got {d}");
    }

    #[test]
    fn distance_le_boundary() {
        assert_eq!(branch_distance(CmpOp::Le, 10, 10), 1.0);
    }

    #[test]
    fn distance_ge_boundary() {
        assert_eq!(branch_distance(CmpOp::Ge, 10, 10), 1.0);
    }

    #[test]
    fn distance_ne_same() {
        assert_eq!(branch_distance(CmpOp::Ne, 5, 5), 0.0);
    }

    #[test]
    fn distance_ne_different() {
        assert_eq!(branch_distance(CmpOp::Ne, 5, 6), 1.0);
    }

    #[test]
    fn normalize_zero() {
        assert_eq!(normalize(0.0), 0.0);
    }

    #[test]
    fn normalize_large() {
        let n = normalize(1_000_000.0);
        assert!(n > 0.99 && n < 1.0);
    }

    #[test]
    fn branch_distance_extreme_values() {
        let extreme_pairs = [
            (i64::MAX, i64::MIN),
            (i64::MIN, i64::MAX),
            (i64::MAX, 0),
            (0, i64::MIN),
            (i64::MAX, i64::MAX),
            (i64::MIN, i64::MIN),
        ];
        for op in [
            CmpOp::Eq,
            CmpOp::Ne,
            CmpOp::Lt,
            CmpOp::Le,
            CmpOp::Gt,
            CmpOp::Ge,
        ] {
            for (a, b) in extreme_pairs {
                let d = branch_distance(op, a, b);
                assert!(
                    (0.0..=1.0).contains(&d),
                    "out of range for {op:?}({a}, {b}): {d}"
                );
            }
        }
    }

    #[test]
    fn distance_always_in_range() {
        for op in [
            CmpOp::Eq,
            CmpOp::Ne,
            CmpOp::Lt,
            CmpOp::Le,
            CmpOp::Gt,
            CmpOp::Ge,
        ] {
            for a in [-100, -1, 0, 1, 42, 1000] {
                for b in [-100, -1, 0, 1, 42, 1000] {
                    let d = branch_distance(op, a, b);
                    assert!(
                        d >= 0.0 && d <= 1.0,
                        "out of range for {op:?}({a}, {b}): {d}"
                    );
                }
            }
        }
    }
}
