//! Empirical complexity estimation using least-squares regression.
//!
//! Implements the Goldsmith et al. (2007) approach: execute a function with
//! systematically increasing input sizes, measure resource consumption, fit
//! measurements to asymptotic complexity models, and report the best-fitting
//! model by R² coefficient of determination.

use apex_core::types::{ComplexityClass, ComplexityEstimate};

/// Empirical complexity estimator.
///
/// Runs a function with increasing input sizes and fits execution time
/// to asymptotic complexity models (O(1), O(log n), O(n), O(n log n),
/// O(n²), O(n³), O(2^n)) using least-squares regression.
pub struct ComplexityEstimator {
    /// Input sizes to test (default: [10, 50, 100, 500, 1000, 5000, 10000])
    pub input_sizes: Vec<usize>,
    /// Iterations per size for statistical stability (default: 3)
    pub iterations_per_size: usize,
}

impl Default for ComplexityEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityEstimator {
    /// Create a new estimator with default sizes [10, 50, 100, 500, 1000, 5000, 10000].
    pub fn new() -> Self {
        ComplexityEstimator {
            input_sizes: vec![10, 50, 100, 500, 1000, 5000, 10000],
            iterations_per_size: 3,
        }
    }

    /// Estimate the complexity class from (input_size, measurement) sample pairs.
    ///
    /// For each candidate complexity model, the input sizes are transformed and
    /// a least-squares linear regression is fitted.  The model with the highest
    /// R² is selected; the R² value is reported as `confidence`.
    ///
    /// Models attempted:
    /// - O(1):       y vs constant (uses variance; perfect fit ⟹ R²=1)
    /// - O(log n):   y vs ln(n)
    /// - O(n):       y vs n
    /// - O(n log n): y vs n·ln(n)
    /// - O(n²):      y vs n²
    /// - O(n³):      y vs n³
    /// - O(2^n):     ln(y) vs n  (only when all y > 0)
    pub fn estimate_from_samples(samples: &[(usize, f64)]) -> ComplexityEstimate {
        if samples.is_empty() {
            return ComplexityEstimate::new(ComplexityClass::Constant, 0.0, 0);
        }

        let n = samples.len();
        let ys: Vec<f64> = samples.iter().map(|&(_, y)| y).collect();
        let ns: Vec<f64> = samples.iter().map(|&(size, _)| size as f64).collect();

        // Candidate models: (class, transformed_xs, transformed_ys)
        // For O(1) we handle separately using variance.
        let mut best_class = ComplexityClass::Constant;
        let mut best_r2 = Self::constant_r2(&ys);

        // O(log n): x = ln(n), y = y
        {
            let xs: Vec<f64> = ns.iter().map(|&n| n.ln()).collect();
            let (_, _, r2) = Self::linear_regression(&xs, &ys);
            if r2 > best_r2 {
                best_r2 = r2;
                best_class = ComplexityClass::Logarithmic;
            }
        }

        // O(n): x = n, y = y
        {
            let (_, _, r2) = Self::linear_regression(&ns, &ys);
            if r2 > best_r2 {
                best_r2 = r2;
                best_class = ComplexityClass::Linear;
            }
        }

        // O(n log n): x = n * ln(n), y = y
        {
            let xs: Vec<f64> = ns.iter().map(|&n| n * n.ln()).collect();
            let (_, _, r2) = Self::linear_regression(&xs, &ys);
            if r2 > best_r2 {
                best_r2 = r2;
                best_class = ComplexityClass::LinearLogarithmic;
            }
        }

        // O(n²): x = n², y = y
        {
            let xs: Vec<f64> = ns.iter().map(|&n| n * n).collect();
            let (_, _, r2) = Self::linear_regression(&xs, &ys);
            if r2 > best_r2 {
                best_r2 = r2;
                best_class = ComplexityClass::Quadratic;
            }
        }

        // O(n³): x = n³, y = y
        {
            let xs: Vec<f64> = ns.iter().map(|&n| n * n * n).collect();
            let (_, _, r2) = Self::linear_regression(&xs, &ys);
            if r2 > best_r2 {
                best_r2 = r2;
                best_class = ComplexityClass::Cubic;
            }
        }

        // O(2^n): x = n, y = ln(y)  (only valid when all y > 0)
        if ys.iter().all(|&y| y > 0.0) {
            let log_ys: Vec<f64> = ys.iter().map(|&y| y.ln()).collect();
            let (_, _, r2) = Self::linear_regression(&ns, &log_ys);
            if r2 > best_r2 {
                best_r2 = r2;
                best_class = ComplexityClass::Exponential;
            }
        }

        ComplexityEstimate::new(best_class, best_r2.clamp(0.0, 1.0), n)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Compute R² for the O(1) model: how well a constant (the mean) fits the data.
    ///
    /// R² = 1 - SS_res/SS_tot.  For a constant model SS_res = SS_tot (total variance),
    /// so a perfect constant gives R² = 1 and noisy data gives R² < 1.
    ///
    /// Equivalent to: R² = 1 - Var(y)/Var(y) if all values equal mean… but we compute
    /// it properly as R²_constant = 0 because the null model has SS_res = SS_tot.
    ///
    /// A better framing: the constant model predicts ŷ = ȳ for all points, which is
    /// identical to the baseline in R², so R²_constant = 0 always — BUT we want to
    /// distinguish truly-constant data.  We use 1 - (variance / mean²) clamped to [0,1]
    /// as a practical signal.
    fn constant_r2(ys: &[f64]) -> f64 {
        if ys.is_empty() {
            return 0.0;
        }
        let mean = ys.iter().sum::<f64>() / ys.len() as f64;
        if mean.abs() < f64::EPSILON {
            // All values are near zero — treat as constant.
            let max_dev = ys.iter().map(|&y| (y - mean).abs()).fold(0.0_f64, f64::max);
            return if max_dev < f64::EPSILON { 1.0 } else { 0.0 };
        }
        let ss_res: f64 = ys.iter().map(|&y| (y - mean).powi(2)).sum();
        let ss_tot: f64 = mean * mean * ys.len() as f64; // scale by mean²
        (1.0 - ss_res / ss_tot).clamp(0.0, 1.0)
    }

    /// Ordinary least-squares linear regression of `ys` on `xs`.
    ///
    /// Returns `(slope, intercept, r_squared)`.
    ///
    /// - slope     = Σ((x - x̄)(y - ȳ)) / Σ((x - x̄)²)
    /// - intercept = ȳ - slope · x̄
    /// - r²        = 1 - SS_res / SS_tot
    ///
    /// Returns `(0.0, mean(y), 0.0)` when there is only one sample or when
    /// all xs are identical (zero variance in x).
    pub fn linear_regression(xs: &[f64], ys: &[f64]) -> (f64, f64, f64) {
        debug_assert_eq!(xs.len(), ys.len(), "xs and ys must have equal length");

        let n = xs.len();
        if n == 0 {
            return (0.0, 0.0, 0.0);
        }

        let x_mean = xs.iter().sum::<f64>() / n as f64;
        let y_mean = ys.iter().sum::<f64>() / n as f64;

        let ss_xx: f64 = xs.iter().map(|&x| (x - x_mean).powi(2)).sum();
        let ss_xy: f64 = xs
            .iter()
            .zip(ys.iter())
            .map(|(&x, &y)| (x - x_mean) * (y - y_mean))
            .sum();
        let ss_tot: f64 = ys.iter().map(|&y| (y - y_mean).powi(2)).sum();

        if ss_xx.abs() < f64::EPSILON {
            // Degenerate: all x values are the same — no regression possible.
            return (0.0, y_mean, 0.0);
        }

        let slope = ss_xy / ss_xx;
        let intercept = y_mean - slope * x_mean;

        let ss_res: f64 = xs
            .iter()
            .zip(ys.iter())
            .map(|(&x, &y)| {
                let y_hat = slope * x + intercept;
                (y - y_hat).powi(2)
            })
            .sum();

        let r2 = if ss_tot.abs() < f64::EPSILON {
            // All y values are the same — perfect fit by any model.
            1.0
        } else {
            1.0 - ss_res / ss_tot
        };

        (slope, intercept, r2)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::ComplexityClass;

    // Helper: assert that the best-fit class matches and R² exceeds threshold.
    fn check(samples: &[(usize, f64)], expected: ComplexityClass, min_r2: f64) {
        let est = ComplexityEstimator::estimate_from_samples(samples);
        assert_eq!(
            est.complexity, expected,
            "expected {expected:?} but got {:?} (confidence={:.4})",
            est.complexity, est.confidence
        );
        assert!(
            est.confidence >= min_r2,
            "R² {:.4} < threshold {min_r2} for {expected:?}",
            est.confidence
        );
    }

    #[test]
    fn linear_data_detects_linear() {
        // y = n exactly — O(n)
        let samples = vec![(10, 10.0), (100, 100.0), (1000, 1000.0)];
        check(&samples, ComplexityClass::Linear, 0.99);
    }

    #[test]
    fn quadratic_data_detects_quadratic() {
        // y = n² exactly — O(n²)
        let samples = vec![(10, 100.0), (100, 10_000.0), (1000, 1_000_000.0)];
        check(&samples, ComplexityClass::Quadratic, 0.99);
    }

    #[test]
    fn constant_data_detects_constant() {
        // y = 5 for all n — O(1)
        let samples = vec![(10, 5.0), (100, 5.0), (1000, 5.0)];
        check(&samples, ComplexityClass::Constant, 0.99);
    }

    #[test]
    fn exponential_data_detects_exponential() {
        // y = 2^n — O(2^n)
        let samples = vec![(1, 2.0), (2, 4.0), (3, 8.0), (4, 16.0)];
        check(&samples, ComplexityClass::Exponential, 0.99);
    }

    #[test]
    fn noisy_random_data_has_low_confidence() {
        // Deliberately uncorrelated data should yield low R² for the winning model.
        let samples = vec![
            (10, 42.0),
            (100, 7.0),
            (1000, 200.0),
            (500, 3.0),
            (50, 99.0),
        ];
        let est = ComplexityEstimator::estimate_from_samples(&samples);
        // We do not assert the class (it can be anything), only that confidence is low.
        assert!(
            est.confidence < 0.8,
            "expected low confidence for noisy data, got {:.4}",
            est.confidence
        );
    }

    #[test]
    fn empty_samples_returns_constant_with_zero_confidence() {
        let est = ComplexityEstimator::estimate_from_samples(&[]);
        assert_eq!(est.complexity, ComplexityClass::Constant);
        assert_eq!(est.confidence, 0.0);
        assert_eq!(est.sample_count, 0);
    }

    #[test]
    fn linear_regression_perfect_fit() {
        // y = 2x + 1
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys: Vec<f64> = xs.iter().map(|&x| 2.0 * x + 1.0).collect();
        let (slope, intercept, r2) = ComplexityEstimator::linear_regression(&xs, &ys);
        assert!((slope - 2.0).abs() < 1e-9, "slope {slope}");
        assert!((intercept - 1.0).abs() < 1e-9, "intercept {intercept}");
        assert!((r2 - 1.0).abs() < 1e-9, "r2 {r2}");
    }

    #[test]
    fn linear_regression_degenerate_single_x_value() {
        let xs = vec![5.0, 5.0, 5.0];
        let ys = vec![1.0, 2.0, 3.0];
        let (slope, _intercept, r2) = ComplexityEstimator::linear_regression(&xs, &ys);
        assert_eq!(slope, 0.0);
        assert_eq!(r2, 0.0);
    }

    #[test]
    fn complexity_estimator_has_default_sizes() {
        let est = ComplexityEstimator::new();
        assert_eq!(est.input_sizes, vec![10, 50, 100, 500, 1000, 5000, 10000]);
        assert_eq!(est.iterations_per_size, 3);
    }

    #[test]
    fn sample_count_matches_input() {
        let samples = vec![(1, 1.0), (2, 4.0), (3, 9.0)];
        let est = ComplexityEstimator::estimate_from_samples(&samples);
        assert_eq!(est.sample_count, 3);
    }
}
