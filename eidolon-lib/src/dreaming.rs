// dreaming.rs - offline consolidation engine
// Performs one dream cycle: replay, merge, prune, discover, resolve.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ndarray::Array1;

use crate::graph::ConnectionGraph;
use crate::substrate::HopfieldSubstrate;
use crate::types::{BrainMemory, EdgeType};

// ---- Constants ----

pub const REPLAY_BOOST: f32 = 0.05;
pub const REPLAY_EDGE_BOOST: f32 = 0.01;
pub const REPLAY_TOP_N: usize = 20;

pub const MERGE_SIMILARITY_THRESHOLD: f32 = 0.92;
pub const MERGE_CONTENT_RATIO: f32 = 0.70;

pub const PRUNE_DECAY_THRESHOLD: f32 = 0.08;
pub const PRUNE_EDGE_THRESHOLD: f32 = 0.02;

pub const DISCOVERY_SIM_THRESHOLD: f32 = 0.35;
pub const DISCOVERY_SAMPLE_SIZE: usize = 50;

// ---- Result struct ----

#[derive(Debug, Clone, serde::Serialize)]
pub struct DreamCycleResult {
    pub replayed: usize,
    pub merged: usize,
    pub pruned_patterns: usize,
    pub pruned_edges: usize,
    pub discovered: usize,
    pub resolved: usize,
    pub cycle_time_ms: u64,
}

// ---- Utility: cosine similarity between two ndarray vectors ----

fn cosine_sim(a: &Array1<f32>, b: &Array1<f32>) -> f32 {
    if a.len() != b.len() || a.len() == 0 {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na < 1e-10 || nb < 1e-10 {
        return 0.0;
    }
    (dot / (na * nb)).max(-1.0).min(1.0)
}

// ---- Utility: Jaccard word overlap ----

fn word_overlap(a: &str, b: &str) -> f32 {
    let a_words: HashSet<&str> = a.split_whitespace().collect();
    let b_words: HashSet<&str> = b.split_whitespace().collect();
    if a_words.is_empty() && b_words.is_empty() {
        return 1.0;
    }
    let intersection = a_words.intersection(&b_words).count();
    let union = a_words.union(&b_words).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

// ---- Operation 1: Replay recent memories ----

fn replay_recent(
    memories: &mut Vec<BrainMemory>,
    _memory_index: &HashMap<i64, usize>,
    graph: &mut ConnectionGraph,
    substrate: &mut HopfieldSubstrate,
) -> usize {
    if memories.is_empty() {
        return 0;
    }

    // Sort indices by last_activated descending, pick top N
    let mut indexed: Vec<(usize, f64)> = memories
        .iter()
        .enumerate()
        .map(|(i, m)| (i, m.last_activated))
        .collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let top_n = indexed.len().min(REPLAY_TOP_N);
    let top_indices: Vec<usize> = indexed[..top_n].iter().map(|(i, _)| *i).collect();
    let top_ids: Vec<i64> = top_indices.iter().map(|&i| memories[i].id).collect();

    // Boost decay_factor for replayed memories
    for &idx in &top_indices {
        memories[idx].decay_factor = (memories[idx].decay_factor + REPLAY_BOOST).min(1.0);
        let id = memories[idx].id;
        let strength = memories[idx].decay_factor;
        let pattern = memories[idx].pattern.clone();
        substrate.store(id, &pattern, strength);
    }

    // Strengthen edges between co-activated top patterns (Hebbian)
    for i in 0..top_ids.len() {
        for j in (i + 1)..top_ids.len() {
            graph.strengthen_edge(top_ids[i], top_ids[j], REPLAY_EDGE_BOOST);
            graph.strengthen_edge(top_ids[j], top_ids[i], REPLAY_EDGE_BOOST);
        }
    }

    top_n
}

// ---- Operation 2: Merge redundant patterns ----

fn merge_redundant(
    memories: &mut Vec<BrainMemory>,
    memory_index: &mut HashMap<i64, usize>,
    graph: &mut ConnectionGraph,
    substrate: &mut HopfieldSubstrate,
) -> usize {
    let n = memories.len();
    if n < 2 {
        return 0;
    }

    let mut to_remove: HashSet<i64> = HashSet::new();

    // Check top pairs by cosine similarity (limit to avoid O(n^2) on large sets)
    let check_limit = n.min(50);

    for i in 0..check_limit {
        let id_i = memories[i].id;
        if to_remove.contains(&id_i) {
            continue;
        }
        for j in (i + 1)..check_limit {
            let id_j = memories[j].id;
            if to_remove.contains(&id_j) {
                continue;
            }

            let sim = cosine_sim(&memories[i].pattern, &memories[j].pattern);
            if sim < MERGE_SIMILARITY_THRESHOLD {
                continue;
            }

            let overlap = word_overlap(&memories[i].content, &memories[j].content);
            if overlap < MERGE_CONTENT_RATIO {
                continue;
            }

            // Winner = higher effective strength (decay_factor * importance)
            let eff_i = memories[i].decay_factor * memories[i].importance as f32;
            let eff_j = memories[j].decay_factor * memories[j].importance as f32;

            let (winner_idx, loser_idx) = if eff_i >= eff_j { (i, j) } else { (j, i) };
            let winner_id = memories[winner_idx].id;
            let loser_id = memories[loser_idx].id;

            // Transfer: winner absorbs loser's importance (max)
            let loser_importance = memories[loser_idx].importance;
            if loser_importance > memories[winner_idx].importance {
                memories[winner_idx].importance = loser_importance;
            }

            // Redirect loser's edges to winner in graph
            let loser_edges: Vec<(i64, f32, EdgeType)> = if let Some(edges) = graph.adjacency.get(&loser_id) {
                edges.iter().map(|(t, w, et)| (*t, *w, et.clone())).collect()
            } else {
                vec![]
            };

            for (target, weight, etype) in loser_edges {
                if target != winner_id {
                    graph.add_edge(winner_id, target, weight, etype.clone());
                    graph.add_edge(target, winner_id, weight, etype);
                }
            }

            to_remove.insert(loser_id);
        }
    }

    let removed_count = to_remove.len();

    if removed_count > 0 {
        // Remove from substrate
        for &id in &to_remove {
            substrate.remove(id);
        }

        // Remove from memories list and rebuild index
        memories.retain(|m| !to_remove.contains(&m.id));
        memory_index.clear();
        for (i, m) in memories.iter().enumerate() {
            memory_index.insert(m.id, i);
        }

        // Remove nodes from graph
        for &id in &to_remove {
            graph.nodes.remove(&id);
            graph.adjacency.remove(&id);
            graph.reverse_adjacency.remove(&id);
            // Remove all edges pointing to this id
            for edges in graph.adjacency.values_mut() {
                edges.retain(|(t, _, _)| *t != id);
            }
            // Remove all reverse edges pointing to this id
            for edges in graph.reverse_adjacency.values_mut() {
                edges.retain(|(s, _, _)| *s != id);
            }
        }
    }

    removed_count
}

// ---- Operation 3: Prune dead patterns and weak edges ----

fn prune_dead(
    memories: &mut Vec<BrainMemory>,
    memory_index: &mut HashMap<i64, usize>,
    graph: &mut ConnectionGraph,
    substrate: &mut HopfieldSubstrate,
) -> (usize, usize) {
    // Find dead patterns
    let dead_ids: Vec<i64> = memories
        .iter()
        .filter(|m| m.decay_factor < PRUNE_DECAY_THRESHOLD)
        .map(|m| m.id)
        .collect();

    let pruned_patterns = dead_ids.len();

    for &id in &dead_ids {
        substrate.remove(id);
    }

    // Remove weak edges and count them
    let mut pruned_edges = 0usize;
    for edges in graph.adjacency.values_mut() {
        let before = edges.len();
        edges.retain(|(_, w, _)| *w >= PRUNE_EDGE_THRESHOLD);
        pruned_edges += before - edges.len();
    }

    // Remove dead memories
    if pruned_patterns > 0 {
        let dead_set: HashSet<i64> = dead_ids.iter().cloned().collect();

        // Also clean up graph nodes and adjacency for dead memories
        for &id in &dead_set {
            graph.nodes.remove(&id);
            graph.adjacency.remove(&id);
            graph.reverse_adjacency.remove(&id);
            for edges in graph.adjacency.values_mut() {
                edges.retain(|(t, _, _)| *t != id);
            }
            for edges in graph.reverse_adjacency.values_mut() {
                edges.retain(|(s, _, _)| *s != id);
            }
        }

        memories.retain(|m| !dead_set.contains(&m.id));
        memory_index.clear();
        for (i, m) in memories.iter().enumerate() {
            memory_index.insert(m.id, i);
        }
    }

    (pruned_patterns, pruned_edges)
}

// ---- Operation 4: Discover new connections ----

fn discover_connections(
    memories: &[BrainMemory],
    graph: &mut ConnectionGraph,
    cycle_number: u64,
) -> usize {
    let n = memories.len();
    if n < 2 {
        return 0;
    }

    let mut discovered = 0usize;

    // Use top 20 strongest patterns as anchors
    let mut by_strength: Vec<usize> = (0..n).collect();
    by_strength.sort_by(|&a, &b| {
        let sa = memories[a].decay_factor * memories[a].importance as f32;
        let sb = memories[b].decay_factor * memories[b].importance as f32;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let anchor_count = n.min(20);
    let anchors = &by_strength[..anchor_count];

    // Check DISCOVERY_SAMPLE_SIZE random pairs using cycle_number as seed
    let sample_limit = DISCOVERY_SAMPLE_SIZE.min(n);

    let mut checked = 0usize;
    for &ai in anchors {
        if checked >= DISCOVERY_SAMPLE_SIZE {
            break;
        }
        let id_a = memories[ai].id;

        // Walk through other memories using a deterministic pseudo-random offset
        let offset = ((cycle_number as usize).wrapping_mul(2654435761) ^ ai) % n.max(1);
        for step in 0..sample_limit {
            let bi = (offset + step) % n;
            if bi == ai {
                continue;
            }
            checked += 1;
            if checked > DISCOVERY_SAMPLE_SIZE {
                break;
            }

            let id_b = memories[bi].id;

            // Check if edge already exists between a and b
            let already_connected = graph.adjacency
                .get(&id_a)
                .map(|edges| edges.iter().any(|(t, _, _)| *t == id_b))
                .unwrap_or(false);

            if already_connected {
                continue;
            }

            let sim = cosine_sim(&memories[ai].pattern, &memories[bi].pattern);
            if sim >= DISCOVERY_SIM_THRESHOLD {
                graph.add_edge(id_a, id_b, sim * 0.5, EdgeType::Association);
                graph.add_edge(id_b, id_a, sim * 0.5, EdgeType::Association);
                discovered += 1;
            }
        }
    }

    discovered
}

// ---- Operation 5: Resolve lingering contradictions ----

fn resolve_lingering(
    memories: &mut Vec<BrainMemory>,
    memory_index: &HashMap<i64, usize>,
    graph: &mut ConnectionGraph,
) -> usize {
    let mut resolved = 0usize;
    let mut edges_to_remove: Vec<(i64, i64)> = Vec::new();
    let mut loser_ids: Vec<i64> = Vec::new();

    // Find contradiction edges where one side has decayed significantly
    for (&src_id, edges) in &graph.adjacency {
        let src_idx = match memory_index.get(&src_id) {
            Some(&idx) => idx,
            None => continue,
        };
        let src_decay = memories[src_idx].decay_factor;

        for &(tgt_id, _, ref etype) in edges {
            if *etype != EdgeType::Contradiction {
                continue;
            }
            let tgt_idx = match memory_index.get(&tgt_id) {
                Some(&idx) => idx,
                None => continue,
            };
            let tgt_decay = memories[tgt_idx].decay_factor;

            // One side weak (< 0.2), the other strong (> 0.6): loser has lost
            let (loser_id, _winner_id) = if src_decay < 0.2 && tgt_decay > 0.6 {
                (src_id, tgt_id)
            } else if tgt_decay < 0.2 && src_decay > 0.6 {
                (tgt_id, src_id)
            } else {
                continue;
            };

            edges_to_remove.push((src_id, tgt_id));
            edges_to_remove.push((tgt_id, src_id));
            loser_ids.push(loser_id);
            resolved += 1;
        }
    }

    // Remove contradiction edges
    for (src, tgt) in &edges_to_remove {
        if let Some(edges) = graph.adjacency.get_mut(src) {
            edges.retain(|(t, _, et)| !(*t == *tgt && *et == EdgeType::Contradiction));
        }
        if let Some(edges) = graph.reverse_adjacency.get_mut(tgt) {
            edges.retain(|(s, _, et)| !(*s == *src && *et == EdgeType::Contradiction));
        }
    }

    // Suppress losers further
    for loser_id in loser_ids {
        if let Some(&idx) = memory_index.get(&loser_id) {
            memories[idx].decay_factor *= 0.5;
        }
    }

    resolved
}

// ---- Main: run one dream cycle ----

pub fn dream_cycle(
    substrate: &mut HopfieldSubstrate,
    graph: &mut ConnectionGraph,
    memories: &mut Vec<BrainMemory>,
    memory_index: &mut HashMap<i64, usize>,
    cycle_number: u64,
) -> DreamCycleResult {
    let t0 = Instant::now();

    // Guard: nothing to do with fewer than 2 memories
    if memories.len() < 2 {
        return DreamCycleResult {
            replayed: 0,
            merged: 0,
            pruned_patterns: 0,
            pruned_edges: 0,
            discovered: 0,
            resolved: 0,
            cycle_time_ms: t0.elapsed().as_millis() as u64,
        };
    }

    let replayed = replay_recent(memories, memory_index, graph, substrate);
    let merged = merge_redundant(memories, memory_index, graph, substrate);
    let (pruned_patterns, pruned_edges) = prune_dead(memories, memory_index, graph, substrate);
    let discovered = discover_connections(memories, graph, cycle_number);
    let resolved = resolve_lingering(memories, memory_index, graph);

    DreamCycleResult {
        replayed,
        merged,
        pruned_patterns,
        pruned_edges,
        discovered,
        resolved,
        cycle_time_ms: t0.elapsed().as_millis() as u64,
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array1;
    use std::collections::HashMap;

    fn make_pattern(dim: usize, val: f32) -> Array1<f32> {
        let mut v = Array1::<f32>::zeros(dim);
        for x in v.iter_mut() {
            *x = val;
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-10 {
            v / norm
        } else {
            v
        }
    }

    fn make_memory(id: i64, pattern_val: f32, decay: f32, last_activated: f64) -> BrainMemory {
        BrainMemory {
            id,
            content: format!("memory {}", id),
            category: "test".to_string(),
            source: "test".to_string(),
            importance: 5,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            embedding: vec![],
            pattern: make_pattern(16, pattern_val),
            activation: 0.5,
            last_activated,
            access_count: 0,
            decay_factor: decay,
            tags: vec![],
        }
    }

    fn build_substrate_and_graph(
        memories: &[BrainMemory],
    ) -> (HopfieldSubstrate, ConnectionGraph, HashMap<i64, usize>) {
        let mut substrate = HopfieldSubstrate::new();
        let mut graph = ConnectionGraph::new();
        let mut index = HashMap::new();
        for (i, m) in memories.iter().enumerate() {
            substrate.store(m.id, &m.pattern, m.decay_factor);
            graph.add_node(m.id);
            index.insert(m.id, i);
        }
        (substrate, graph, index)
    }

    #[test]
    fn test_replay() {
        let mut memories: Vec<BrainMemory> = (1..=5)
            .map(|i| make_memory(i, i as f32 * 0.1, 0.7, i as f64 * 100.0))
            .collect();
        let (mut substrate, mut graph, mut index) = build_substrate_and_graph(&memories);

        let before_decay: Vec<f32> = memories.iter().map(|m| m.decay_factor).collect();
        let replayed = replay_recent(&mut memories, &index, &mut graph, &mut substrate);

        assert!(replayed > 0, "should have replayed at least one memory");
        let after_decay: Vec<f32> = memories.iter().map(|m| m.decay_factor).collect();
        let boosted = after_decay.iter().zip(before_decay.iter()).filter(|(a, b)| *a > *b).count();
        assert!(
            boosted > 0,
            "at least one memory should have been boosted, before: {:?}, after: {:?}",
            before_decay,
            after_decay
        );
    }

    #[test]
    fn test_merge() {
        let dim = 16;
        let mut p1 = Array1::<f32>::zeros(dim);
        p1[0] = 1.0;
        let mut p2 = Array1::<f32>::zeros(dim);
        p2[0] = 1.0;
        p2[1] = 0.001;

        let norm1: f32 = p1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm2: f32 = p2.iter().map(|x| x * x).sum::<f32>().sqrt();
        p1 /= norm1;
        p2 /= norm2;

        let shared_content =
            "alpha beta gamma delta epsilon zeta eta theta iota kappa".to_string();
        let mut m1 = make_memory(1, 0.0, 0.9, 100.0);
        let mut m2 = make_memory(2, 0.0, 0.6, 50.0);
        m1.pattern = p1;
        m2.pattern = p2;
        m1.content = shared_content.clone();
        m2.content = shared_content;

        let mut memories = vec![m1, m2];
        let (mut substrate, mut graph, mut index) = build_substrate_and_graph(&memories);

        let merged = merge_redundant(&mut memories, &mut index, &mut graph, &mut substrate);
        assert_eq!(merged, 1, "one pattern should have been merged");
        assert_eq!(memories.len(), 1, "only one pattern should remain");
    }

    #[test]
    fn test_prune() {
        let dead = make_memory(1, 0.1, 0.01, 0.0);
        let alive = make_memory(2, 0.5, 0.9, 100.0);
        let mut memories = vec![dead, alive];
        let (mut substrate, mut graph, mut index) = build_substrate_and_graph(&memories);

        let (pruned_p, _pruned_e) = prune_dead(&mut memories, &mut index, &mut graph, &mut substrate);
        assert_eq!(pruned_p, 1, "one dead pattern should have been pruned");
        assert_eq!(memories.len(), 1, "one memory should remain");
        assert_eq!(memories[0].id, 2, "the alive memory should remain");
    }

    #[test]
    fn test_discover() {
        let dim = 16;
        let mut p1 = Array1::<f32>::zeros(dim);
        let mut p2 = Array1::<f32>::zeros(dim);
        p1[0] = 1.0;
        p1[1] = 0.8;
        p2[0] = 0.9;
        p2[1] = 0.7;
        let norm1: f32 = p1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm2: f32 = p2.iter().map(|x| x * x).sum::<f32>().sqrt();
        p1 /= norm1;
        p2 /= norm2;

        let mut m1 = make_memory(1, 0.0, 0.9, 100.0);
        let mut m2 = make_memory(2, 0.0, 0.9, 100.0);
        m1.pattern = p1;
        m2.pattern = p2;

        let mut memories = vec![m1, m2];
        let (mut substrate, mut graph, index) = build_substrate_and_graph(&memories);

        let initially_connected = graph
            .adjacency
            .get(&1)
            .map(|e| !e.is_empty())
            .unwrap_or(false);
        assert!(!initially_connected, "patterns should not be connected initially");

        let discovered = discover_connections(&memories, &mut graph, 1);
        assert!(discovered > 0, "should have discovered connections");
        let connected = graph
            .adjacency
            .get(&1)
            .map(|e| e.iter().any(|(t, _, _)| *t == 2))
            .unwrap_or(false);
        assert!(connected, "edge should exist between pattern 1 and 2 after discovery");
    }

    #[test]
    fn test_full_cycle() {
        let mut memories: Vec<BrainMemory> = (1..=5)
            .map(|i| make_memory(i, i as f32 * 0.2, 0.7 + i as f32 * 0.05, i as f64 * 50.0))
            .collect();
        let (mut substrate, mut graph, mut index) = build_substrate_and_graph(&memories);

        let result = dream_cycle(&mut substrate, &mut graph, &mut memories, &mut index, 1);

        assert!(
            result.cycle_time_ms < 10000,
            "cycle time should be reasonable: {}ms",
            result.cycle_time_ms
        );
    }

    #[test]
    fn test_empty_substrate() {
        let mut memories: Vec<BrainMemory> = vec![];
        let mut substrate = HopfieldSubstrate::new();
        let mut graph = ConnectionGraph::new();
        let mut index = HashMap::new();

        let result = dream_cycle(&mut substrate, &mut graph, &mut memories, &mut index, 1);
        assert_eq!(result.replayed, 0);
        assert_eq!(result.merged, 0);
        assert_eq!(result.pruned_patterns, 0);
    }
}
