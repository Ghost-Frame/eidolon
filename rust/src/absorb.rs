use ndarray::Array1;
use crate::types::{BrainMemory, EdgeType};
use crate::pca::PcaTransform;
use crate::substrate::HopfieldSubstrate;
use crate::graph::ConnectionGraph;
use crate::interference::parse_datetime_approx;

pub const ASSOCIATION_THRESHOLD: f32 = 0.4;
pub const TEMPORAL_WINDOW_SECS: f64 = 86400.0;
pub const MAX_EDGES_PER_MEMORY: usize = 15;
pub const CONTRADICTION_SIM_THRESHOLD: f32 = 0.75;

/// Cosine similarity between two vectors.
pub fn cosine_sim(a: &Array1<f32>, b: &Array1<f32>) -> f32 {
    let dot = a.dot(b);
    let na = a.dot(a).sqrt();
    let nb = b.dot(b).sqrt();
    if na < 1e-10 || nb < 1e-10 {
        return 0.0;
    }
    dot / (na * nb)
}

/// Absorb a new memory into all substrate components.
/// 1. PCA project raw embedding to pattern
/// 2. Store in Hopfield substrate
/// 3. Add as graph node
/// 4. Cosine similarity edges (>0.4, top 15)
/// 5. Temporal edges (within 24h window)
/// 6. Contradiction detection (sim >0.75, same category)
pub fn absorb_memory(
    memory: &mut BrainMemory,
    existing_memories: &[BrainMemory],
    pca: &PcaTransform,
    substrate: &mut HopfieldSubstrate,
    graph: &mut ConnectionGraph,
) {
    // Step 1: PCA project
    let raw = Array1::from(memory.embedding.clone());
    memory.pattern = pca.project(&raw);

    // Step 2: Hopfield store with strength = decay_factor
    substrate.store(memory.id, &memory.pattern, memory.decay_factor);

    // Step 3: Graph node
    graph.add_node(memory.id);

    let new_created = parse_datetime_approx(&memory.created_at);

    // Step 4 + 5 + 6: Edges to existing memories
    let mut association_candidates: Vec<(i64, f32)> = Vec::new();

    for existing in existing_memories {
        if existing.id == memory.id {
            continue;
        }
        if existing.pattern.len() == 0 {
            continue;
        }
        let sim = cosine_sim(&memory.pattern, &existing.pattern);

        // Temporal edge
        let existing_created = parse_datetime_approx(&existing.created_at);
        let time_diff = (new_created - existing_created).abs();
        if time_diff <= TEMPORAL_WINDOW_SECS && time_diff >= 0.0 {
            graph.add_edge(memory.id, existing.id, sim.max(0.1), EdgeType::Temporal);
            graph.add_edge(existing.id, memory.id, sim.max(0.1), EdgeType::Temporal);
        }

        // Contradiction detection
        if sim > CONTRADICTION_SIM_THRESHOLD
            && memory.category == existing.category
            && memory.content != existing.content
        {
            graph.add_edge(memory.id, existing.id, sim, EdgeType::Contradiction);
            graph.add_edge(existing.id, memory.id, sim, EdgeType::Contradiction);
        } else if sim > ASSOCIATION_THRESHOLD {
            association_candidates.push((existing.id, sim));
        }
    }

    // Add top MAX_EDGES_PER_MEMORY association edges
    association_candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    association_candidates.truncate(MAX_EDGES_PER_MEMORY);

    for (target_id, weight) in association_candidates {
        graph.add_edge(memory.id, target_id, weight, EdgeType::Association);
        graph.add_edge(target_id, memory.id, weight, EdgeType::Association);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array1;

    #[test]
    fn cosine_sim_correct() {
        let a = Array1::from(vec![1.0_f32, 0.0, 0.0]);
        let b = Array1::from(vec![0.0_f32, 1.0, 0.0]);
        assert!((cosine_sim(&a, &b)).abs() < 1e-5, "orthogonal vectors: sim should be 0");

        let c = Array1::from(vec![1.0_f32, 0.0, 0.0]);
        assert!((cosine_sim(&a, &c) - 1.0).abs() < 1e-5, "identical vectors: sim should be 1");

        let d = Array1::from(vec![-1.0_f32, 0.0, 0.0]);
        assert!((cosine_sim(&a, &d) + 1.0).abs() < 1e-5, "opposite vectors: sim should be -1");
    }

    #[test]
    fn absorb_adds_node_and_edges() {
        use crate::pca::PcaTransform;
        use crate::substrate::HopfieldSubstrate;
        use crate::graph::ConnectionGraph;
        use ndarray::Array2;

        // Build a small PCA (1 component from 4-dim data)
        let d = 4;
        let data = Array2::from_shape_fn((8, d), |(i, j)| {
            ((i as f32 * 0.5 + j as f32 * 0.3) * std::f32::consts::PI).sin()
        });
        let pca = PcaTransform::fit(&data);

        let mut substrate = HopfieldSubstrate::new();
        let mut graph = ConnectionGraph::new();

        // Create an existing memory
        let existing_raw: Vec<f32> = (0..d).map(|i| (i as f32 * 0.1).sin()).collect();
        let existing_pattern = pca.project(&Array1::from(existing_raw.clone()));
        let existing = BrainMemory {
            id: 1,
            content: "existing memory".to_string(),
            category: "test".to_string(),
            source: "test".to_string(),
            importance: 5,
            created_at: "2026-03-27T00:00:00Z".to_string(),
            embedding: existing_raw,
            pattern: existing_pattern.clone(),
            activation: 0.5,
            last_activated: 0.0,
            access_count: 0,
            decay_factor: 1.0,
            tags: vec![],
        };
        substrate.store(existing.id, &existing_pattern, 1.0);
        graph.add_node(existing.id);

        // Absorb a new memory
        let new_raw: Vec<f32> = (0..d).map(|i| ((i as f32 + 0.1) * 0.1).sin()).collect();
        let mut new_mem = BrainMemory {
            id: 2,
            content: "new memory".to_string(),
            category: "test".to_string(),
            source: "test".to_string(),
            importance: 5,
            created_at: "2026-03-27T00:01:00Z".to_string(),
            embedding: new_raw,
            pattern: Array1::zeros(pca.n_components),
            activation: 0.5,
            last_activated: 0.0,
            access_count: 0,
            decay_factor: 1.0,
            tags: vec![],
        };

        let existing_list = vec![existing];
        absorb_memory(&mut new_mem, &existing_list, &pca, &mut substrate, &mut graph);

        assert!(graph.nodes.contains(&2), "node 2 should be added");
        assert_eq!(substrate.n_patterns(), 2, "should have 2 patterns");
    }
}
