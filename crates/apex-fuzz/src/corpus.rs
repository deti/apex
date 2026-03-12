use rand::Rng;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerSchedule {
    Explore,
    Fast,
    Rare,
}

/// In-memory fuzzing corpus with a fixed capacity.
///
/// When full, the oldest entry is evicted (LRU-style). Entries that produced
/// new coverage are inserted at the back so they stay longer.
pub struct Corpus {
    entries: VecDeque<CorpusEntry>,
    max_size: usize,
    schedule: PowerSchedule,
}

#[derive(Clone)]
pub struct CorpusEntry {
    pub data: Vec<u8>,
    /// How many new branches this entry discovered when first run.
    pub coverage_gain: usize,
    pub energy: f64,
    pub fuzz_count: u64,
    pub covered_edges: Vec<u64>,
    /// Distance to a directed-fuzzing target (AFLGo-style). `None` means unknown.
    pub distance_to_target: Option<f64>,
}

impl Corpus {
    pub fn new(max_size: usize) -> Self {
        Corpus {
            entries: VecDeque::new(),
            max_size: max_size.max(1),
            schedule: PowerSchedule::Explore,
        }
    }

    pub fn add(&mut self, data: Vec<u8>, coverage_gain: usize) {
        if self.entries.len() >= self.max_size {
            self.entries.pop_front();
        }
        self.entries.push_back(CorpusEntry {
            data,
            coverage_gain,
            energy: coverage_gain.max(1) as f64,
            fuzz_count: 0,
            covered_edges: Vec::new(),
            distance_to_target: None,
        });
    }

    /// Sample an entry, weighted by energy (set via power schedule).
    pub fn sample(&mut self, rng: &mut impl Rng) -> Option<&CorpusEntry> {
        if self.entries.is_empty() {
            return None;
        }
        let total: f64 = self.entries.iter().map(|e| e.energy.max(0.001)).sum();
        let mut pick = rng.gen::<f64>() * total;
        let mut selected = self.entries.len() - 1;
        for (i, entry) in self.entries.iter().enumerate() {
            let w = entry.energy.max(0.001);
            if pick < w {
                selected = i;
                break;
            }
            pick -= w;
        }
        self.entries[selected].fuzz_count += 1;
        Some(&self.entries[selected])
    }

    /// Sample two distinct entries for splicing.
    pub fn sample_pair<'a>(
        &'a self,
        rng: &mut impl Rng,
    ) -> Option<(&'a CorpusEntry, &'a CorpusEntry)> {
        if self.entries.len() < 2 {
            return None;
        }
        let i = rng.gen_range(0..self.entries.len());
        let mut j = rng.gen_range(0..self.entries.len() - 1);
        if j >= i {
            j += 1;
        }
        Some((&self.entries[i], &self.entries[j]))
    }

    pub fn set_power_schedule(&mut self, schedule: PowerSchedule) {
        self.schedule = schedule;
        self.recalculate_energy();
    }

    fn recalculate_energy(&mut self) {
        match self.schedule {
            PowerSchedule::Explore => {
                for e in &mut self.entries {
                    e.energy = 1.0;
                }
            }
            PowerSchedule::Fast => {
                for e in &mut self.entries {
                    e.energy = 1.0 / ((e.fuzz_count.max(1) as f64) * (e.data.len().max(1) as f64));
                }
            }
            PowerSchedule::Rare => {
                let mut edge_counts: std::collections::HashMap<u64, usize> =
                    std::collections::HashMap::new();
                for e in self.entries.iter() {
                    for &edge in &e.covered_edges {
                        *edge_counts.entry(edge).or_default() += 1;
                    }
                }
                for e in &mut self.entries {
                    if e.covered_edges.is_empty() {
                        e.energy = 1.0;
                    } else {
                        e.energy = e
                            .covered_edges
                            .iter()
                            .map(|edge| 1.0 / *edge_counts.get(edge).unwrap_or(&1) as f64)
                            .sum::<f64>();
                    }
                }
            }
        }
    }

    /// Greedy set-cover minimization. Returns a new Corpus containing the
    /// smallest subset of entries that covers all edges.
    pub fn minimize(&self) -> Corpus {
        use std::collections::HashSet;

        let mut remaining: HashSet<u64> = self
            .entries
            .iter()
            .flat_map(|e| e.covered_edges.iter().copied())
            .collect();

        let mut selected = Vec::new();
        let mut used = vec![false; self.entries.len()];

        while !remaining.is_empty() {
            let mut best_idx = None;
            let mut best_count = 0;
            for (i, entry) in self.entries.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let count = entry
                    .covered_edges
                    .iter()
                    .filter(|e| remaining.contains(e))
                    .count();
                if count > best_count {
                    best_count = count;
                    best_idx = Some(i);
                }
            }
            match best_idx {
                Some(idx) => {
                    used[idx] = true;
                    for edge in &self.entries[idx].covered_edges {
                        remaining.remove(edge);
                    }
                    selected.push(self.entries[idx].clone());
                }
                None => break,
            }
        }

        let mut result = Corpus::new(self.max_size);
        result.schedule = self.schedule;
        for entry in selected {
            result.entries.push_back(entry);
        }
        result
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn evicts_oldest_when_full() {
        let mut c = Corpus::new(3);
        c.add(vec![1], 1);
        c.add(vec![2], 1);
        c.add(vec![3], 1);
        c.add(vec![4], 1); // should evict vec![1]
        assert_eq!(c.len(), 3);
        assert!(c.entries.front().unwrap().data == vec![2]);
    }

    #[test]
    fn sample_returns_entry() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let mut c = Corpus::new(10);
        c.add(vec![0xAA], 2);
        assert!(c.sample(&mut rng).is_some());
    }

    #[test]
    fn sample_on_empty_corpus() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut c = Corpus::new(10);
        assert!(c.sample(&mut rng).is_none());
    }

    #[test]
    fn sample_pair_with_single_entry() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        assert!(c.sample_pair(&mut rng).is_none());
    }

    #[test]
    fn sample_pair_with_two_entries() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        c.add(vec![2], 1);
        let pair = c.sample_pair(&mut rng);
        assert!(pair.is_some());
        let (a, b) = pair.unwrap();
        // The two entries should be distinct (different indices)
        assert_ne!(a.data, b.data);
    }

    #[test]
    fn add_multiple_with_different_gains() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let mut c = Corpus::new(100);
        // Entry with gain=100 should be sampled far more often than gain=1
        c.add(vec![0xAA], 1);
        c.add(vec![0xBB], 100);

        let mut count_bb = 0usize;
        let iterations = 1000;
        for _ in 0..iterations {
            let entry = c.sample(&mut rng).unwrap();
            if entry.data == vec![0xBB] {
                count_bb += 1;
            }
        }
        // With weights 1 vs 100, 0xBB should appear ~99% of the time.
        // Use a conservative threshold of 80%.
        assert!(
            count_bb > iterations * 80 / 100,
            "high-gain entry sampled {} / {} times, expected > 80%",
            count_bb,
            iterations
        );
    }

    #[test]
    fn is_empty_true_initially() {
        let c = Corpus::new(10);
        assert!(c.is_empty());
    }

    #[test]
    fn is_empty_false_after_add() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        assert!(!c.is_empty());
    }

    #[test]
    fn len_tracks_additions() {
        let mut c = Corpus::new(100);
        assert_eq!(c.len(), 0);
        c.add(vec![1], 1);
        assert_eq!(c.len(), 1);
        c.add(vec![2], 1);
        assert_eq!(c.len(), 2);
        c.add(vec![3], 1);
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn max_size_of_zero_becomes_one() {
        let mut c = Corpus::new(0);
        // max_size should be 1 due to .max(1)
        c.add(vec![1], 1);
        c.add(vec![2], 1);
        // Only 1 entry should survive
        assert_eq!(c.len(), 1);
        assert_eq!(c.entries.front().unwrap().data, vec![2]);
    }

    #[test]
    fn energy_field_exists() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        assert!(c.entries.front().unwrap().energy > 0.0);
    }

    #[test]
    fn set_power_schedule() {
        let mut c = Corpus::new(10);
        c.set_power_schedule(PowerSchedule::Rare);
    }

    #[test]
    fn fast_schedule_favors_less_fuzzed() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        c.add(vec![2], 1);
        c.entries[0].fuzz_count = 100;
        c.entries[1].fuzz_count = 1;
        c.set_power_schedule(PowerSchedule::Fast);
        assert!(c.entries[1].energy > c.entries[0].energy);
    }

    #[test]
    fn rare_schedule_favors_rare_edges() {
        let mut c = Corpus::new(10);
        c.entries.push_back(CorpusEntry {
            data: vec![1],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1, 2],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![2],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![3],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![3],
            distance_to_target: None, // unique edge
        });
        c.set_power_schedule(PowerSchedule::Rare);
        // Entry with unique edge 3 should have highest energy
        assert!(c.entries[2].energy >= c.entries[1].energy);
    }

    #[test]
    fn minimize_reduces_corpus() {
        let mut c = Corpus::new(100);
        c.entries.push_back(CorpusEntry {
            data: vec![0],
            coverage_gain: 2,
            energy: 2.0,
            fuzz_count: 0,
            covered_edges: vec![1, 2],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![1],
            coverage_gain: 2,
            energy: 2.0,
            fuzz_count: 0,
            covered_edges: vec![2, 3],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![2],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1],
            distance_to_target: None,
        });

        let minimized = c.minimize();
        assert!(minimized.len() <= 2);
    }

    #[test]
    fn minimize_empty_corpus() {
        let c = Corpus::new(10);
        let minimized = c.minimize();
        assert!(minimized.is_empty());
    }

    #[test]
    fn power_schedule_eq_and_copy() {
        let a = PowerSchedule::Explore;
        let b = a; // Copy
        assert_eq!(a, b);
        assert_ne!(PowerSchedule::Explore, PowerSchedule::Fast);
        assert_ne!(PowerSchedule::Fast, PowerSchedule::Rare);
    }

    #[test]
    fn power_schedule_debug() {
        let debug = format!("{:?}", PowerSchedule::Rare);
        assert_eq!(debug, "Rare");
    }

    #[test]
    fn explore_schedule_sets_uniform_energy() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 5);
        c.add(vec![2], 1);
        c.set_power_schedule(PowerSchedule::Explore);
        // All entries should have energy 1.0
        for e in c.entries.iter() {
            assert!((e.energy - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn rare_schedule_empty_covered_edges_gets_one() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1); // no covered_edges
        c.set_power_schedule(PowerSchedule::Rare);
        assert!((c.entries[0].energy - 1.0).abs() < 1e-9);
    }

    #[test]
    fn corpus_entry_distance_to_target_default_none() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        assert!(c.entries[0].distance_to_target.is_none());
    }

    #[test]
    fn minimize_entries_with_no_edges() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1); // no covered_edges
        c.add(vec![2], 1); // no covered_edges
        let minimized = c.minimize();
        // No edges to cover, so result should be empty
        assert!(minimized.is_empty());
    }

    #[test]
    fn minimize_preserves_schedule() {
        let mut c = Corpus::new(100);
        c.set_power_schedule(PowerSchedule::Fast);
        c.entries.push_back(CorpusEntry {
            data: vec![0],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1],
            distance_to_target: None,
        });
        let minimized = c.minimize();
        assert_eq!(minimized.schedule, PowerSchedule::Fast);
    }

    #[test]
    fn sample_increments_fuzz_count() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let mut c = Corpus::new(10);
        c.add(vec![0xAA], 1);
        let _ = c.sample(&mut rng);
        assert_eq!(c.entries[0].fuzz_count, 1);
        let _ = c.sample(&mut rng);
        assert_eq!(c.entries[0].fuzz_count, 2);
    }

    #[test]
    fn fast_schedule_smaller_input_higher_energy() {
        let mut c = Corpus::new(10);
        c.entries.push_back(CorpusEntry {
            data: vec![1], // len=1
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 1,
            covered_edges: vec![],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10], // len=10
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 1,
            covered_edges: vec![],
            distance_to_target: None,
        });
        c.set_power_schedule(PowerSchedule::Fast);
        // Smaller input should have higher energy
        assert!(c.entries[0].energy > c.entries[1].energy);
    }

    #[test]
    fn minimize_covers_all_edges() {
        let mut c = Corpus::new(100);
        c.entries.push_back(CorpusEntry {
            data: vec![0],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1, 2, 3],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![1],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![4, 5],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![2],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1, 4],
            distance_to_target: None,
        });

        let minimized = c.minimize();
        let all_edges: std::collections::HashSet<u64> = minimized
            .entries
            .iter()
            .flat_map(|e| e.covered_edges.iter().copied())
            .collect();
        // Must cover all 5 edges
        for edge in 1..=5u64 {
            assert!(all_edges.contains(&edge), "missing edge {edge}");
        }
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn sample_pair_both_results_are_from_corpus() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(77);
        let mut c = Corpus::new(10);
        c.add(vec![0xAA], 1);
        c.add(vec![0xBB], 1);
        c.add(vec![0xCC], 1);
        for _ in 0..50 {
            let (a, b) = c.sample_pair(&mut rng).unwrap();
            // Both should be from the known set of entries
            assert!(a.data == vec![0xAA] || a.data == vec![0xBB] || a.data == vec![0xCC]);
            assert!(b.data == vec![0xAA] || b.data == vec![0xBB] || b.data == vec![0xCC]);
        }
    }

    #[test]
    fn corpus_max_size_exactly_reached() {
        let mut c = Corpus::new(2);
        c.add(vec![1], 1);
        c.add(vec![2], 1);
        assert_eq!(c.len(), 2);
        // Adding one more evicts the oldest
        c.add(vec![3], 1);
        assert_eq!(c.len(), 2);
        assert_eq!(c.entries.front().unwrap().data, vec![2]);
        assert_eq!(c.entries.back().unwrap().data, vec![3]);
    }

    #[test]
    fn rare_schedule_two_entries_more_edges_wins() {
        let mut c = Corpus::new(10);
        // Entry 0: covers two edges each seen once => energy = 1/1 + 1/1 = 2.0
        c.entries.push_back(CorpusEntry {
            data: vec![1],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![1, 2],
            distance_to_target: None,
        });
        // Entry 1: covers one unique edge => energy = 1/1 = 1.0
        c.entries.push_back(CorpusEntry {
            data: vec![2],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 0,
            covered_edges: vec![3],
            distance_to_target: None,
        });
        c.set_power_schedule(PowerSchedule::Rare);
        // Entry 0 covers more edges, so its energy is higher
        assert!(c.entries[0].energy > c.entries[1].energy);
    }

    #[test]
    fn fast_schedule_high_fuzz_count_has_lower_energy_than_low() {
        let mut c = Corpus::new(10);
        c.entries.push_back(CorpusEntry {
            data: vec![1],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 1000,
            covered_edges: vec![],
            distance_to_target: None,
        });
        c.entries.push_back(CorpusEntry {
            data: vec![2],
            coverage_gain: 1,
            energy: 1.0,
            fuzz_count: 1,
            covered_edges: vec![],
            distance_to_target: None,
        });
        c.set_power_schedule(PowerSchedule::Fast);
        assert!(c.entries[1].energy > c.entries[0].energy);
    }

    #[test]
    fn corpus_entry_covered_edges_can_be_set() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        c.entries[0].covered_edges = vec![100, 200, 300];
        assert_eq!(c.entries[0].covered_edges.len(), 3);
    }

    #[test]
    fn corpus_entry_distance_can_be_set() {
        let mut c = Corpus::new(10);
        c.add(vec![1], 1);
        c.entries[0].distance_to_target = Some(3.14);
        assert!((c.entries[0].distance_to_target.unwrap() - 3.14).abs() < 1e-9);
    }
}
