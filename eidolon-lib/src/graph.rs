use std::collections::{HashMap, HashSet};
use crate::types::EdgeType;

pub const SPREAD_DECAY_PER_HOP: f32 = 0.5;
pub const MIN_SPREAD_ACTIVATION: f32 = 0.005;
pub const MIN_EDGE_WEIGHT: f32 = 0.01;

#[derive(Debug)]
pub struct ConnectionGraph {
    pub adjacency: HashMap<i64, Vec<(i64, f32, EdgeType)>>,
    pub nodes: HashSet<i64>,
}

impl ConnectionGraph {
    pub fn new() -> Self {
        ConnectionGraph {
            adjacency: HashMap::new(),
            nodes: HashSet::new(),
        }
    }

    pub fn add_node(&mut self, id: i64) {
        self.nodes.insert(id);
        self.adjacency.entry(id).or_default();
    }

    pub fn add_edge(&mut self, source: i64, target: i64, weight: f32, edge_type: EdgeType) {
        self.nodes.insert(source);
        self.nodes.insert(target);
        let edges = self.adjacency.entry(source).or_default();
        // Update if edge exists, otherwise append
        if let Some(e) = edges.iter_mut().find(|(t, _, _)| *t == target) {
            e.1 = weight;
            e.2 = edge_type;
        } else {
            edges.push((target, weight, edge_type));
        }
    }

    pub fn total_edges(&self) -> usize {
        self.adjacency.values().map(|v| v.len()).sum()
    }

    /// Spread activation from seeds over max_hops.
    /// Returns HashMap<id, (activation, hops_from_seed)>.
    /// Uses max-merge (not sum) to avoid unbounded growth.
    pub fn spread(
        &self,
        seeds: &HashMap<i64, f32>,
        max_hops: usize,
    ) -> HashMap<i64, (f32, usize)> {
        let mut activations: HashMap<i64, (f32, usize)> = HashMap::new();

        // Initialize with seeds at hop 0
        for (&id, &act) in seeds {
            activations.insert(id, (act, 0));
        }

        let mut frontier: HashMap<i64, f32> = seeds.clone();

        for hop in 0..max_hops {
            if frontier.is_empty() {
                break;
            }
            let mut next_frontier: HashMap<i64, f32> = HashMap::new();

            for (&from_id, &from_act) in &frontier {
                let hop_alpha = from_act * SPREAD_DECAY_PER_HOP;
                if hop_alpha < MIN_SPREAD_ACTIVATION {
                    continue;
                }
                if let Some(edges) = self.adjacency.get(&from_id) {
                    for &(to_id, weight, _) in edges {
                        let spread_act = hop_alpha * weight;
                        if spread_act < MIN_SPREAD_ACTIVATION {
                            continue;
                        }
                        let current_hop = hop + 1;
                        let entry = activations.entry(to_id).or_insert((0.0, current_hop));
                        // Max-merge
                        if spread_act > entry.0 {
                            entry.0 = spread_act;
                            entry.1 = current_hop;
                        }
                        // Add to next frontier (max-merge)
                        let fe = next_frontier.entry(to_id).or_insert(0.0);
                        if spread_act > *fe {
                            *fe = spread_act;
                        }
                    }
                }
            }
            frontier = next_frontier;
        }

        activations
    }

    /// Find contradiction pairs among active node IDs.
    /// Returns (id_a, id_b) where there is a Contradiction edge.
    pub fn contradiction_pairs(&self, active_ids: &[i64]) -> Vec<(i64, i64)> {
        let active_set: HashSet<i64> = active_ids.iter().cloned().collect();
        let mut pairs = Vec::new();

        for &id in &active_set {
            if let Some(edges) = self.adjacency.get(&id) {
                for &(target, _, ref etype) in edges {
                    if *etype == EdgeType::Contradiction && active_set.contains(&target) && id < target {
                        pairs.push((id, target));
                    }
                }
            }
        }
        pairs
    }

    /// Hebbian strengthening: boost edge weight between co-activated nodes.
    pub fn strengthen_edge(&mut self, source: i64, target: i64, boost: f32) {
        if let Some(edges) = self.adjacency.get_mut(&source) {
            if let Some(e) = edges.iter_mut().find(|(t, _, _)| *t == target) {
                e.1 = (e.1 + boost).min(1.0);
            }
        }
    }

    /// Decay all edge weights by multiplying by rate. Remove edges below min_weight.
    pub fn decay_edges(&mut self, rate: f32, min_weight: f32) {
        for edges in self.adjacency.values_mut() {
            edges.iter_mut().for_each(|e| e.1 *= rate);
            edges.retain(|(_, w, _)| *w >= min_weight);
        }
    }
}

impl Default for ConnectionGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreading_basic() {
        let mut g = ConnectionGraph::new();
        g.add_node(1);
        g.add_node(2);
        g.add_edge(1, 2, 1.0, EdgeType::Association);

        let mut seeds = HashMap::new();
        seeds.insert(1i64, 1.0f32);

        let result = g.spread(&seeds, 2);
        assert!(result.contains_key(&1));
        assert!(result.contains_key(&2));
        assert!(result[&2].0 > 0.0, "activation should spread to node 2");
    }

    #[test]
    fn multi_hop() {
        let mut g = ConnectionGraph::new();
        g.add_edge(1, 2, 1.0, EdgeType::Association);
        g.add_edge(2, 3, 1.0, EdgeType::Association);
        g.add_edge(3, 4, 1.0, EdgeType::Association);

        let mut seeds = HashMap::new();
        seeds.insert(1i64, 1.0f32);

        let result = g.spread(&seeds, 3);
        assert!(result.contains_key(&4), "activation should reach node 4 in 3 hops");
        assert!(result[&4].0 > 0.0);
        // Each hop decays by 0.5 * weight
        // hop1: 1.0 * 0.5 * 1.0 = 0.5
        // hop2: 0.5 * 0.5 * 1.0 = 0.25
        // hop3: 0.25 * 0.5 * 1.0 = 0.125
        assert!(result[&4].0 > MIN_SPREAD_ACTIVATION);
        assert_eq!(result[&4].1, 3, "should be 3 hops away");
    }

    #[test]
    fn strengthen_and_decay() {
        let mut g = ConnectionGraph::new();
        g.add_edge(1, 2, 0.5, EdgeType::Association);

        g.strengthen_edge(1, 2, 0.2);
        let w = g.adjacency[&1].iter().find(|(t, _, _)| *t == 2).unwrap().1;
        assert!((w - 0.7).abs() < 1e-5, "weight after boost: {}", w);

        g.decay_edges(0.5, 0.01);
        let w2 = g.adjacency[&1].iter().find(|(t, _, _)| *t == 2).unwrap().1;
        assert!((w2 - 0.35).abs() < 1e-5, "weight after decay: {}", w2);
    }

    #[test]
    fn contradiction_pairs() {
        let mut g = ConnectionGraph::new();
        g.add_edge(1, 2, 0.9, EdgeType::Contradiction);
        g.add_edge(3, 4, 0.8, EdgeType::Association);

        let active = vec![1i64, 2i64, 3i64, 4i64];
        let pairs = g.contradiction_pairs(&active);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], (1, 2));
    }
}
