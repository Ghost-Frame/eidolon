//! Neural reasoning engine -- substrate-native inference generation.
//! Feature-gated behind `--features reasoning`.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::ConnectionGraph;
use crate::substrate::HopfieldSubstrate;
use crate::types::{
    BrainMemory, ContradictionPair, EdgeType,
    Inference, InferenceKind, ReasoningConfig,
};

// ---- Mode 1: Abductive Reasoning (Backward Causal) ----

/// "Why did X happen?" -- traverse Causal edges backward.
pub fn abductive_reason(
    graph: &ConnectionGraph,
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
    activated: &HashMap<i64, f32>,
    config: &ReasoningConfig,
) -> Vec<Inference> {
    if !config.abductive {
        return vec![];
    }
    let max_depth = 3;
    let mut inferences: Vec<Inference> = Vec::new();
    let mut counter = 0;

    for (&mem_id, &activation) in activated {
        if activation < 0.5 {
            continue;
        }

        // Backward BFS on Causal edges
        let mut queue: VecDeque<(i64, Vec<i64>, f32)> = VecDeque::new();
        queue.push_back((mem_id, vec![mem_id], 1.0));
        let mut visited: HashSet<i64> = HashSet::new();
        visited.insert(mem_id);
        let mut chains: Vec<(Vec<i64>, f32)> = Vec::new();

        while let Some((current, path, confidence)) = queue.pop_front() {
            if path.len() > max_depth + 1 {
                continue;
            }
            // Get predecessors (causal parents)
            let predecessors = graph.predecessors(current, &EdgeType::Causal);
            for (pred_id, weight) in predecessors {
                if visited.contains(&pred_id) {
                    continue;
                }
                visited.insert(pred_id);
                let chain_conf = confidence * weight;
                if chain_conf < config.min_confidence {
                    continue;
                }
                let mut new_path = path.clone();
                new_path.push(pred_id);
                chains.push((new_path.clone(), chain_conf));
                if new_path.len() <= max_depth + 1 {
                    queue.push_back((pred_id, new_path, chain_conf));
                }
            }
        }

        // Merge chains sharing same root cause
        let mut root_groups: HashMap<i64, Vec<f32>> = HashMap::new();
        let mut root_chains: HashMap<i64, Vec<i64>> = HashMap::new();
        for (chain, conf) in &chains {
            let root = *chain.last().unwrap_or(&mem_id);
            root_groups.entry(root).or_default().push(*conf);
            root_chains.entry(root).or_insert_with(|| chain.clone());
        }

        for (root_id, confidences) in root_groups {
            let combined = 1.0 - confidences.iter().fold(1.0_f32, |acc, c| acc * (1.0 - c));
            if combined < config.min_confidence {
                continue;
            }

            let chain = root_chains.get(&root_id).cloned().unwrap_or_default();
            let content = build_chain_description(&chain, memories, memory_index, "caused");
            let source_edges: Vec<(i64, i64)> = chain.windows(2).map(|w| (w[1], w[0])).collect();

            // Apply root memory's decay_factor
            let root_decay = memory_index.get(&root_id)
                .map(|&idx| memories[idx].decay_factor)
                .unwrap_or(1.0);

            counter += 1;
            inferences.push(Inference {
                id: format!("inf-abductive-{}", counter),
                kind: InferenceKind::Abductive,
                content,
                confidence: combined * root_decay,
                source_ids: chain.clone(),
                source_edges,
            });
        }
    }

    inferences.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    inferences.truncate(config.max_inferences);
    inferences
}

// ---- Mode 2: Predictive Reasoning (Forward Causal) ----

/// "What will X cause?" -- traverse Causal edges forward.
pub fn predictive_reason(
    graph: &ConnectionGraph,
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
    activated: &HashMap<i64, f32>,
    config: &ReasoningConfig,
) -> Vec<Inference> {
    if !config.predictive {
        return vec![];
    }
    let max_depth = 3;
    let spread_decay: f32 = 0.5;
    let mut inferences: Vec<Inference> = Vec::new();
    let mut counter = 0;
    let activated_set: HashSet<i64> = activated.keys().cloned().collect();

    for (&mem_id, &activation) in activated {
        if activation < 0.5 {
            continue;
        }

        // Forward BFS on Causal edges
        let mut queue: VecDeque<(i64, Vec<i64>, f32, usize)> = VecDeque::new();
        queue.push_back((mem_id, vec![mem_id], 1.0, 0));
        let mut visited: HashSet<i64> = HashSet::new();
        visited.insert(mem_id);

        while let Some((current, path, confidence, hops)) = queue.pop_front() {
            if hops >= max_depth {
                continue;
            }
            let successors = graph.successors(current, &EdgeType::Causal);
            for (succ_id, weight) in successors {
                if visited.contains(&succ_id) {
                    continue;
                }
                visited.insert(succ_id);
                let chain_conf = confidence * weight * spread_decay.powi((hops + 1) as i32);
                if chain_conf < config.min_confidence {
                    continue;
                }
                let mut new_path = path.clone();
                new_path.push(succ_id);

                // Exclude consequences already activated (not predictions)
                if !activated_set.contains(&succ_id) {
                    let content = build_prediction_description(&new_path, memories, memory_index);
                    let source_edges: Vec<(i64, i64)> = new_path.windows(2).map(|w| (w[0], w[1])).collect();

                    counter += 1;
                    inferences.push(Inference {
                        id: format!("inf-predictive-{}", counter),
                        kind: InferenceKind::Predictive,
                        content,
                        confidence: chain_conf,
                        source_ids: new_path.clone(),
                        source_edges,
                    });
                }

                if hops + 1 < max_depth {
                    queue.push_back((succ_id, new_path, confidence * weight, hops + 1));
                }
            }
        }
    }

    inferences.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    inferences.truncate(config.max_inferences);
    inferences
}

// ---- Mode 3: Contradiction Synthesis ----

/// Produce temporal understanding from contradiction pairs.
pub fn synthesize_contradictions(
    contradictions: &[ContradictionPair],
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
    config: &ReasoningConfig,
) -> Vec<Inference> {
    if !config.synthesis {
        return vec![];
    }
    let mut inferences: Vec<Inference> = Vec::new();
    let mut counter = 0;

    for pair in contradictions {
        let winner_idx = match memory_index.get(&pair.winner_id) {
            Some(&idx) => idx,
            None => continue,
        };
        let loser_idx = match memory_index.get(&pair.loser_id) {
            Some(&idx) => idx,
            None => continue,
        };

        let winner = &memories[winner_idx];
        let loser = &memories[loser_idx];

        let winner_preview = winner.content_preview(80);
        let loser_preview = loser.content_preview(80);

        let content = if loser.created_at < winner.created_at {
            format!(
                "'{}' was the case until {}, superseded by: '{}'",
                loser_preview, winner.created_at, winner_preview
            )
        } else {
            format!(
                "'{}' replaced earlier understanding: '{}'",
                winner_preview, loser_preview
            )
        };

        let confidence = (pair.winner_activation - pair.loser_activation).abs().min(1.0);
        if confidence < config.min_confidence {
            continue;
        }

        counter += 1;
        inferences.push(Inference {
            id: format!("inf-synthesis-{}", counter),
            kind: InferenceKind::Synthesis,
            content,
            confidence,
            source_ids: vec![pair.winner_id, pair.loser_id],
            source_edges: vec![(pair.winner_id, pair.loser_id)],
        });
    }

    inferences.truncate(config.max_inferences);
    inferences
}

// ---- Mode 4: Rule Extraction (Dreaming-Phase) ----

/// Extract implicit rules from strongly co-activated memory clusters.
/// Called during dream phase, results cached on Brain.
pub fn extract_rules(
    graph: &ConnectionGraph,
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
    min_edge_weight: f32,
) -> Vec<Inference> {
    let mut inferences: Vec<Inference> = Vec::new();
    let mut counter = 0;

    // Find nodes with 2+ outgoing strong edges
    for (&node_id, edges) in &graph.adjacency {
        let strong_neighbors: Vec<(i64, f32)> = edges.iter()
            .filter(|(_, w, _)| *w >= min_edge_weight)
            .map(|(t, w, _)| (*t, *w))
            .collect();

        if strong_neighbors.len() < 2 {
            continue;
        }

        // Check bidirectional strength (clique detection)
        let mut clique_ids: Vec<i64> = vec![node_id];
        for &(neighbor_id, _) in &strong_neighbors {
            // Check if neighbor also has strong edge back
            let has_reverse = graph.adjacency.get(&neighbor_id)
                .map(|e| e.iter().any(|(t, w, _)| *t == node_id && *w >= min_edge_weight))
                .unwrap_or(false);
            if has_reverse {
                clique_ids.push(neighbor_id);
            }
        }

        if clique_ids.len() < 3 {
            continue;
        }

        // Collect categories and build rule
        let mut categories: HashMap<String, Vec<String>> = HashMap::new();
        let mut mean_weight: f32 = 0.0;
        let mut weight_count: usize = 0;

        for &cid in &clique_ids {
            if let Some(&idx) = memory_index.get(&cid) {
                let mem = &memories[idx];
                categories.entry(mem.category.clone())
                    .or_default()
                    .push(mem.content_preview(60));
            }
            // Sum edge weights
            for &(neighbor, _) in &strong_neighbors {
                if clique_ids.contains(&neighbor) {
                    if let Some(edges) = graph.adjacency.get(&cid) {
                        if let Some((_, w, _)) = edges.iter().find(|(t, _, _)| *t == neighbor) {
                            mean_weight += w;
                            weight_count += 1;
                        }
                    }
                }
            }
        }

        if weight_count > 0 {
            mean_weight /= weight_count as f32;
        }

        let content = if categories.len() > 1 {
            let cats: Vec<String> = categories.iter()
                .map(|(cat, summaries)| format!("[{}]: {}", cat, summaries.join("; ")))
                .collect();
            format!("Cross-category pattern: {}", cats.join(" <-> "))
        } else {
            let (cat, summaries) = categories.iter().next().unwrap();
            format!("[{}] recurring pattern: {}", cat, summaries.join("; "))
        };

        let confidence = mean_weight * (clique_ids.len() as f32).sqrt() / (memories.len() as f32).sqrt().max(1.0);
        let confidence = confidence.min(1.0);

        counter += 1;
        inferences.push(Inference {
            id: format!("inf-rule-{}", counter),
            kind: InferenceKind::Rule,
            content,
            confidence,
            source_ids: clique_ids,
            source_edges: vec![],
        });
    }

    inferences.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    inferences.truncate(10); // Cache up to 10 rules
    inferences
}

/// Filter cached rules to those relevant to current activation.
pub fn filter_cached_rules(
    cached_rules: &[Inference],
    activated: &HashMap<i64, f32>,
    config: &ReasoningConfig,
) -> Vec<Inference> {
    if !config.rule_extraction {
        return vec![];
    }
    let activated_set: HashSet<i64> = activated.keys().cloned().collect();
    let mut relevant: Vec<Inference> = cached_rules.iter()
        .filter(|rule| {
            rule.source_ids.iter().any(|id| activated_set.contains(id))
        })
        .cloned()
        .collect();
    relevant.truncate(config.max_inferences);
    relevant
}

// ---- Mode 5: Analogical Reasoning (Structural Pattern Matching) ----

/// Find structural parallels between memory clusters.
pub fn analogical_reason(
    graph: &ConnectionGraph,
    substrate: &HopfieldSubstrate,
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
    activated: &HashMap<i64, f32>,
    query_pattern: &ndarray::Array1<f32>,
    config: &ReasoningConfig,
) -> Vec<Inference> {
    if !config.analogical {
        return vec![];
    }
    let mut inferences: Vec<Inference> = Vec::new();
    let mut counter = 0;
    let activated_set: HashSet<i64> = activated.keys().cloned().collect();

    // 1. Extract activated subgraph signature
    let activated_signature = subgraph_signature(graph, &activated_set);

    // 2. Pattern completion via Hopfield
    let completed = substrate.complete(query_pattern, 3, 8.0);

    // 3. Find non-activated memories similar to completed pattern
    for mem in memories {
        if activated_set.contains(&mem.id) {
            continue;
        }
        if mem.pattern.len() == 0 {
            continue;
        }
        let sim = crate::absorb::cosine_sim(&mem.pattern, &completed);
        if sim < 0.3 {
            continue;
        }

        // 4. Check structural similarity of neighborhoods
        let mem_neighbors: HashSet<i64> = graph.adjacency.get(&mem.id)
            .map(|edges| edges.iter().map(|(t, _, _)| *t).collect())
            .unwrap_or_default();

        let mem_signature = subgraph_signature(graph, &mem_neighbors);
        let structural_sim = jaccard_similarity(&activated_signature, &mem_signature);

        if structural_sim < 0.6 {
            continue;
        }

        let confidence = structural_sim * sim;
        if confidence < config.min_confidence.max(0.5) {
            continue;
        }

        let mem_preview = mem.content_preview(80);
        // Find a representative activated memory for the analogy
        let anchor_preview = activated.iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .and_then(|(id, _)| memory_index.get(id))
            .map(|&idx| memories[idx].content_preview(80))
            .unwrap_or_else(|| "current context".to_string());

        counter += 1;
        inferences.push(Inference {
            id: format!("inf-analogical-{}", counter),
            kind: InferenceKind::Analogical,
            content: format!("By analogy with '{}', consider: '{}'", anchor_preview, mem_preview),
            confidence,
            source_ids: vec![mem.id],
            source_edges: vec![],
        });

        if counter as usize >= config.max_inferences {
            break;
        }
    }

    inferences.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    inferences.truncate(config.max_inferences);
    inferences
}

// ---- Helpers ----

/// Compute a structural signature for a subgraph: set of (edge_type, degree) pairs.
fn subgraph_signature(graph: &ConnectionGraph, nodes: &HashSet<i64>) -> HashSet<String> {
    let mut sig: HashSet<String> = HashSet::new();
    for &node in nodes {
        if let Some(edges) = graph.adjacency.get(&node) {
            let degree = edges.len();
            for (_, _, et) in edges {
                sig.insert(format!("{}:{}", et.as_str(), degree.min(10)));
            }
        }
    }
    sig
}

/// Jaccard similarity between two sets of strings.
fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

fn build_chain_description(
    chain: &[i64],
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
    verb: &str,
) -> String {
    let previews: Vec<String> = chain.iter()
        .filter_map(|id| memory_index.get(id).map(|&idx| memories[idx].content_preview(60)))
        .collect();
    if previews.len() <= 1 {
        return previews.into_iter().next().unwrap_or_default();
    }
    let root = previews.last().unwrap();
    let effect = previews.first().unwrap();
    if previews.len() == 2 {
        format!("'{}' {} '{}'", root, verb, effect)
    } else {
        let intermediates: Vec<&str> = previews[1..previews.len()-1].iter().map(|s| s.as_str()).collect();
        format!("'{}' -> [{}] -> '{}'", root, intermediates.join(" -> "), effect)
    }
}

fn build_prediction_description(
    chain: &[i64],
    memories: &[BrainMemory],
    memory_index: &HashMap<i64, usize>,
) -> String {
    let previews: Vec<String> = chain.iter()
        .filter_map(|id| memory_index.get(id).map(|&idx| memories[idx].content_preview(60)))
        .collect();
    if previews.len() <= 1 {
        return previews.into_iter().next().unwrap_or_default();
    }
    let situation = previews.first().unwrap();
    let consequence = previews.last().unwrap();
    if previews.len() == 2 {
        format!("If '{}', then '{}'", situation, consequence)
    } else {
        let intermediates: Vec<&str> = previews[1..previews.len()-1].iter().map(|s| s.as_str()).collect();
        format!("If '{}', then '{}' (via {})", situation, consequence, intermediates.join(", "))
    }
}
