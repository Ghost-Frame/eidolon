use ndarray::{Array1, Array2};
use std::collections::HashMap;

pub const DEFAULT_BETA: f32 = 8.0;
pub const ACTIVATION_THRESHOLD: f32 = 0.01;

#[derive(Debug)]
pub struct HopfieldSubstrate {
    /// Pattern matrix: n_patterns x BRAIN_DIM
    pub patterns: Array2<f32>,
    pub strengths: Vec<f32>,
    pub pattern_ids: Vec<i64>,
    pub id_to_index: HashMap<i64, usize>,
}

impl HopfieldSubstrate {
    pub fn new() -> Self {
        HopfieldSubstrate {
            patterns: Array2::zeros((0, 0)),
            strengths: Vec::new(),
            pattern_ids: Vec::new(),
            id_to_index: HashMap::new(),
        }
    }

    pub fn n_patterns(&self) -> usize {
        self.pattern_ids.len()
    }

    /// Store or update a pattern. If id already exists, update in place.
    pub fn store(&mut self, id: i64, pattern: &Array1<f32>, strength: f32) {
        let d = pattern.len();
        if let Some(&idx) = self.id_to_index.get(&id) {
            // Update existing
            self.patterns.row_mut(idx).assign(pattern);
            self.strengths[idx] = strength;
            return;
        }

        // Append new row
        if self.patterns.nrows() == 0 {
            self.patterns = Array2::zeros((1, d));
            self.patterns.row_mut(0).assign(pattern);
        } else {
            // Grow by 1 row
            let n = self.patterns.nrows();
            let mut new_patterns = Array2::zeros((n + 1, d));
            new_patterns.slice_mut(ndarray::s![..n, ..]).assign(&self.patterns);
            new_patterns.row_mut(n).assign(pattern);
            self.patterns = new_patterns;
        }

        let idx = self.pattern_ids.len();
        self.pattern_ids.push(id);
        self.strengths.push(strength);
        self.id_to_index.insert(id, idx);
    }

    /// Remove a pattern by id. Rebuilds index.
    pub fn remove(&mut self, id: i64) {
        if let Some(&idx) = self.id_to_index.get(&id) {
            let n = self.patterns.nrows();
            if n == 0 {
                return;
            }
            // Build new pattern matrix without this row
            let d = self.patterns.ncols();
            if n == 1 {
                self.patterns = Array2::zeros((0, d));
                self.pattern_ids.clear();
                self.strengths.clear();
                self.id_to_index.clear();
                return;
            }
            let mut new_patterns = Array2::zeros((n - 1, d));
            let mut new_ids = Vec::with_capacity(n - 1);
            let mut new_strengths = Vec::with_capacity(n - 1);
            let mut new_map = HashMap::new();
            let mut new_idx = 0;
            for i in 0..n {
                if i != idx {
                    new_patterns.row_mut(new_idx).assign(&self.patterns.row(i));
                    new_ids.push(self.pattern_ids[i]);
                    new_strengths.push(self.strengths[i]);
                    new_map.insert(self.pattern_ids[i], new_idx);
                    new_idx += 1;
                }
            }
            self.patterns = new_patterns;
            self.pattern_ids = new_ids;
            self.strengths = new_strengths;
            self.id_to_index = new_map;
        }
    }

    /// Retrieve top_k patterns by softmax attention over similarities.
    /// Returns (id, activation_score) pairs sorted descending.
    pub fn retrieve(&self, query: &Array1<f32>, top_k: usize, beta: f32) -> Vec<(i64, f32)> {
        if self.patterns.nrows() == 0 {
            return vec![];
        }
        let sims = self.patterns.dot(query); // n_patterns
        // logits = beta * sim * strength
        let logits: Vec<f32> = sims.iter().zip(self.strengths.iter())
            .map(|(&s, &w)| beta * s * w)
            .collect();

        // Softmax
        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = logits.iter().map(|&l| (l - max_logit).exp()).collect();
        let sum_exp: f32 = exps.iter().sum();
        let activations: Vec<f32> = exps.iter().map(|&e| e / sum_exp.max(1e-10)).collect();

        // Collect and sort
        let mut results: Vec<(i64, f32)> = self.pattern_ids.iter()
            .zip(activations.iter())
            .filter(|(_, &a)| a >= ACTIVATION_THRESHOLD)
            .map(|(&id, &a)| (id, a))
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Pattern completion via iterative attention refinement.
    /// Takes a query, returns a refined query vector (the memory state).
    pub fn complete(&self, query: &Array1<f32>, iterations: usize, beta: f32) -> Array1<f32> {
        if self.patterns.nrows() == 0 {
            return query.clone();
        }
        let mut state = query.clone();
        let d = self.patterns.ncols();

        for _ in 0..iterations {
            let sims = self.patterns.dot(&state);
            // Weighted attention: softmax(beta * sims * strengths)
            let logits: Vec<f32> = sims.iter().zip(self.strengths.iter())
                .map(|(&s, &w)| beta * s * w)
                .collect();
            let max_l = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let exps: Vec<f32> = logits.iter().map(|&l| (l - max_l).exp()).collect();
            let sum_e: f32 = exps.iter().sum();
            let weights: Vec<f32> = exps.iter().map(|&e| e / sum_e.max(1e-10)).collect();

            // New state = weighted sum of patterns
            let mut new_state: Array1<f32> = Array1::zeros(d);
            for (i, &w) in weights.iter().enumerate() {
                new_state = new_state + &self.patterns.row(i).to_owned() * w;
            }

            // L2 normalize
            let norm: f32 = new_state.dot(&new_state).sqrt();
            if norm > 1e-10 {
                new_state /= norm;
            }
            state = new_state;
        }
        state
    }
}

impl Default for HopfieldSubstrate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array1;

    fn unit_vec(dim: usize, idx: usize) -> Array1<f32> {
        let mut v = Array1::zeros(dim);
        v[idx] = 1.0;
        v
    }

    fn norm_vec(v: Array1<f32>) -> Array1<f32> {
        let norm: f32 = v.dot(&v).sqrt();
        if norm > 1e-10 { v / norm } else { v }
    }

    #[test]
    fn store_and_retrieve() {
        let dim = 16;
        let mut sub = HopfieldSubstrate::new();
        let p = unit_vec(dim, 0);
        sub.store(1, &p, 1.0);

        let results = sub.retrieve(&p, 5, DEFAULT_BETA);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, 1);
        assert!(results[0].1 > 0.5, "activation: {}", results[0].1);
    }

    #[test]
    fn multiple_patterns() {
        let dim = 16;
        let mut sub = HopfieldSubstrate::new();
        // Three orthogonal patterns
        sub.store(1, &unit_vec(dim, 0), 1.0);
        sub.store(2, &unit_vec(dim, 1), 1.0);
        sub.store(3, &unit_vec(dim, 2), 1.0);

        // Query for pattern 3
        let q = unit_vec(dim, 2);
        let results = sub.retrieve(&q, 3, DEFAULT_BETA);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, 3, "pattern 3 should win");
    }

    #[test]
    fn strength_affects_retrieval() {
        let dim = 16;
        let mut sub = HopfieldSubstrate::new();
        let p1 = unit_vec(dim, 0);
        let p2 = unit_vec(dim, 1);
        // Both equally similar to query, but p2 is stronger
        sub.store(1, &p1, 0.5);
        sub.store(2, &p2, 2.0);

        // Query midway between p1 and p2
        let q = norm_vec(&p1 + &p2);
        let results = sub.retrieve(&q, 2, DEFAULT_BETA);
        assert!(!results.is_empty());
        // p2 (stronger) should rank higher
        assert_eq!(results[0].0, 2, "stronger pattern should rank higher");
    }

    #[test]
    fn completion_improves_noisy_query() {
        let dim = 32;
        let mut sub = HopfieldSubstrate::new();
        // Store a clear pattern
        let p = Array1::from_shape_fn(dim, |i| if i < 16 { 1.0_f32 } else { 0.0_f32 });
        let p_norm = norm_vec(p.clone());
        sub.store(1, &p_norm, 1.0);

        // Noisy query: mostly the pattern but with noise
        let noisy = Array1::from_shape_fn(dim, |i| {
            let base = if i < 16 { 1.0_f32 } else { 0.0_f32 };
            base + ((i as f32 * 0.7).sin() * 0.3)
        });
        let noisy_norm = norm_vec(noisy);

        let sim_before: f32 = noisy_norm.dot(&p_norm);
        let completed = sub.complete(&noisy_norm, 5, DEFAULT_BETA);
        let sim_after: f32 = completed.dot(&p_norm);

        assert!(sim_after >= sim_before, "completion should improve similarity: before={} after={}", sim_before, sim_after);
    }

    #[test]
    fn store_update_no_duplicate() {
        let dim = 8;
        let mut sub = HopfieldSubstrate::new();
        let p = unit_vec(dim, 0);
        sub.store(42, &p, 1.0);
        sub.store(42, &p, 2.0); // Update

        assert_eq!(sub.n_patterns(), 1);
        assert_eq!(sub.strengths[0], 2.0);
    }

    #[test]
    fn remove_pattern() {
        let dim = 8;
        let mut sub = HopfieldSubstrate::new();
        sub.store(1, &unit_vec(dim, 0), 1.0);
        sub.store(2, &unit_vec(dim, 1), 1.0);
        assert_eq!(sub.n_patterns(), 2);

        sub.remove(1);
        assert_eq!(sub.n_patterns(), 1);
        assert!(!sub.id_to_index.contains_key(&1));
        assert!(sub.id_to_index.contains_key(&2));
    }
}
