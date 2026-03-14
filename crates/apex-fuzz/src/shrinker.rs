//! Delta-debugging style input shrinker.

/// Binary shrinker — reduces failing inputs to minimal reproducer.
pub struct BinaryShrinker {
    /// Minimum input size to try.
    min_size: usize,
}

impl BinaryShrinker {
    pub fn new(min_size: usize) -> Self {
        Self { min_size }
    }

    /// Shrink `input` using delta-debugging. `check_fn` returns true if
    /// the input still triggers the property violation.
    ///
    /// Algorithm:
    /// 1. Start with granularity = 2
    /// 2. Split input into `granularity` chunks
    /// 3. Try removing each chunk — if check_fn still passes, keep the reduced input
    /// 4. If no chunk could be removed, double granularity
    /// 5. Stop when granularity >= input length or input <= min_size
    pub fn shrink<F>(&self, input: &[u8], check_fn: F) -> Vec<u8>
    where
        F: Fn(&[u8]) -> bool,
    {
        if input.is_empty() || !check_fn(input) {
            return input.to_vec();
        }

        let mut current = input.to_vec();
        let mut granularity = 2usize;

        loop {
            if current.len() <= self.min_size {
                break;
            }

            if granularity > current.len() {
                break;
            }

            let chunk_size = current.len().div_ceil(granularity);
            let mut reduced = false;

            // Try removing each chunk.
            let mut i = 0;
            while i < granularity && i * chunk_size < current.len() {
                let start = i * chunk_size;
                let end = (start + chunk_size).min(current.len());

                // Build candidate without this chunk.
                let mut candidate = Vec::with_capacity(current.len() - (end - start));
                candidate.extend_from_slice(&current[..start]);
                candidate.extend_from_slice(&current[end..]);

                if candidate.len() >= self.min_size && !candidate.is_empty() && check_fn(&candidate)
                {
                    current = candidate;
                    reduced = true;
                    // Recompute — don't advance i since indices shifted.
                    // But reduce granularity back toward 2 since we made progress.
                    granularity = 2.max(granularity - 1);
                    break;
                }

                i += 1;
            }

            if !reduced {
                granularity *= 2;
            }
        }

        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_empty_input() {
        let shrinker = BinaryShrinker::new(0);
        let result = shrinker.shrink(&[], |_| true);
        assert!(result.is_empty());
    }

    #[test]
    fn shrink_single_byte() {
        let shrinker = BinaryShrinker::new(0);
        let result = shrinker.shrink(&[0x42], |_| true);
        assert_eq!(result, vec![0x42]);
    }

    #[test]
    fn shrink_removes_unnecessary_prefix() {
        let shrinker = BinaryShrinker::new(0);
        let input: Vec<u8> = (0..10).collect();
        // Only the last byte (9) matters.
        let result = shrinker.shrink(&input, |data| data.contains(&9));
        assert!(result.len() <= 3, "got len {}", result.len());
        assert!(result.contains(&9));
    }

    #[test]
    fn shrink_removes_unnecessary_suffix() {
        let shrinker = BinaryShrinker::new(0);
        let input: Vec<u8> = (0..10).collect();
        // Only the first byte (0) matters.
        let result = shrinker.shrink(&input, |data| data.contains(&0));
        assert!(result.len() <= 3, "got len {}", result.len());
        assert!(result.contains(&0));
    }

    #[test]
    fn shrink_preserves_all_when_all_needed() {
        let shrinker = BinaryShrinker::new(0);
        let input = vec![1, 2, 3, 4];
        // All bytes must be present.
        let result = shrinker.shrink(&input, |data| {
            data.contains(&1) && data.contains(&2) && data.contains(&3) && data.contains(&4)
        });
        assert_eq!(result, vec![1, 2, 3, 4]);
    }

    #[test]
    fn shrink_binary_reduction() {
        let shrinker = BinaryShrinker::new(0);
        let input: Vec<u8> = (0..200).cycle().take(1000).collect();
        // Only the first 10 bytes matter (values 0..10).
        let result = shrinker.shrink(&input, |data| {
            (0u8..10).all(|b| data.contains(&b))
        });
        assert!(
            result.len() <= 20,
            "expected <=20, got {}",
            result.len()
        );
        for b in 0u8..10 {
            assert!(result.contains(&b), "missing byte {b}");
        }
    }

    #[test]
    fn shrink_min_size_respected() {
        let shrinker = BinaryShrinker::new(4);
        let input = vec![0, 1, 2, 3, 4, 5, 6, 7];
        // Only byte 0 matters, but min_size is 4.
        let result = shrinker.shrink(&input, |data| data.contains(&0));
        assert!(result.len() >= 4, "got len {}", result.len());
    }

    #[test]
    fn shrink_check_fn_always_false() {
        let shrinker = BinaryShrinker::new(0);
        let input = vec![1, 2, 3, 4, 5];
        // Nothing triggers — should return original.
        let result = shrinker.shrink(&input, |_| false);
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }
}
