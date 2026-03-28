use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::time::Instant;

use ndarray::Array1;
use rusqlite::Connection;
use serde_json::json;

use engram_brain::absorb::absorb_memory;
use engram_brain::decay::{
    apply_recall_boost, classify_health, compute_pattern_decay, is_dead, EDGE_DECAY_RATE,
};
use engram_brain::graph::ConnectionGraph;
use engram_brain::interference::{now_unix, parse_datetime_approx, resolve_interference};
use engram_brain::pca::PcaTransform;
use engram_brain::persistence::{load_edges, load_memories, save_edge, save_pca_state, load_pca_state};
use engram_brain::substrate::{HopfieldSubstrate, DEFAULT_BETA};
use engram_brain::types::{
    ActivatedMemory, BrainEdge, BrainMemory, Command, ContradictionPair, QueryResult, Response,
    StatsResult, BRAIN_DIM, RAW_DIM,
};

struct Brain {
    memories: Vec<BrainMemory>,
    memory_index: HashMap<i64, usize>,
    pca: PcaTransform,
    substrate: HopfieldSubstrate,
    graph: ConnectionGraph,
    conn: Option<Connection>,
}

impl Brain {
    fn new() -> Self {
        Brain {
            memories: Vec::new(),
            memory_index: HashMap::new(),
            pca: PcaTransform::new_empty(),
            substrate: HopfieldSubstrate::new(),
            graph: ConnectionGraph::new(),
            conn: None,
        }
    }

    fn init(&mut self, db_path: &str) -> Result<String, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("failed to open {}: {}", db_path, e))?;

        // Load memories
        let mut memories = load_memories(&conn)
            .map_err(|e| format!("failed to load memories: {}", e))?;

        if memories.is_empty() {
            self.conn = Some(conn);
            return Err("brain.db has no memories".to_string());
        }

        // Try loading saved PCA state
        let pca = match load_pca_state(&conn) {
            Ok(Some(p)) => p,
            _ => {
                // Fit PCA on all embeddings
                eprintln!("[brain] fitting PCA on {} memories", memories.len());
                let valid: Vec<&Vec<f32>> = memories.iter()
                    .filter(|m| m.embedding.len() == RAW_DIM)
                    .map(|m| &m.embedding)
                    .collect();

                if valid.is_empty() {
                    return Err("no valid embeddings (expected 1024-dim)".to_string());
                }

                let n = valid.len();
                let mut data = ndarray::Array2::<f32>::zeros((n, RAW_DIM));
                for (i, emb) in valid.iter().enumerate() {
                    for (j, &v) in emb.iter().enumerate() {
                        data[[i, j]] = v;
                    }
                }

                let pca = PcaTransform::fit(&data);
                // Save PCA for future use
                let _ = save_pca_state(&conn, &pca);
                pca
            }
        };

        // Project all memories to brain space
        for mem in &mut memories {
            if mem.embedding.len() == RAW_DIM {
                let raw = Array1::from(mem.embedding.clone());
                mem.pattern = pca.project(&raw);
            } else {
                mem.pattern = Array1::zeros(pca.n_components.max(1));
            }
            mem.decay_factor = 1.0;
            mem.activation = 0.5;
        }

        // Build memory index
        let mut memory_index = HashMap::new();
        for (i, m) in memories.iter().enumerate() {
            memory_index.insert(m.id, i);
        }

        // Build Hopfield substrate
        let mut substrate = HopfieldSubstrate::new();
        for mem in &memories {
            if mem.pattern.len() > 0 {
                substrate.store(mem.id, &mem.pattern, mem.decay_factor);
            }
        }

        // Build graph
        let mut graph = ConnectionGraph::new();
        for mem in &memories {
            graph.add_node(mem.id);
        }
        let edges = load_edges(&conn)
            .map_err(|e| format!("failed to load edges: {}", e))?;
        for edge in &edges {
            graph.add_edge(edge.source_id, edge.target_id, edge.weight, edge.edge_type.clone());
        }

        let n_patterns = memories.len();
        let n_edges = graph.total_edges();

        self.memories = memories;
        self.memory_index = memory_index;
        self.pca = pca;
        self.substrate = substrate;
        self.graph = graph;
        self.conn = Some(conn);

        Ok(format!("initialized: {} patterns, {} edges", n_patterns, n_edges))
    }

    fn query(
        &mut self,
        embedding: &[f32],
        top_k: usize,
        beta: f32,
        spread_hops: usize,
    ) -> QueryResult {
        let t0 = Instant::now();

        if self.memories.is_empty() {
            return QueryResult {
                activated: vec![],
                contradictions: vec![],
                total_patterns: 0,
                query_time_ms: t0.elapsed().as_secs_f64() * 1000.0,
            };
        }

        let raw = Array1::from(embedding.to_vec());
        let query_pattern = self.pca.project(&raw);

        // Hopfield retrieve: 2x candidates
        let hopfield_results = self.substrate.retrieve(&query_pattern, top_k * 2, beta);

        // Build seed map for graph spreading
        let mut seeds: HashMap<i64, f32> = HashMap::new();
        for (id, activation) in &hopfield_results {
            seeds.insert(*id, *activation);
        }

        // Graph spread
        let spread_result = self.graph.spread(&seeds, spread_hops);

        // Merge: max-merge hopfield + spread activations
        let mut merged: HashMap<i64, (f32, &str, usize)> = HashMap::new();
        for (id, activation) in &hopfield_results {
            merged.insert(*id, (*activation, "hopfield", 0));
        }
        for (id, (activation, hops)) in &spread_result {
            let entry = merged.entry(*id).or_insert((0.0, "spread", *hops));
            if *activation > entry.0 {
                *entry = (*activation, "spread", *hops);
            } else if entry.1 == "hopfield" && *hops > 0 {
                *entry = (entry.0, "both", entry.2);
            }
        }

        // Get the now timestamp for recency
        let now = now_unix();

        // Resolve interference for contradiction pairs
        let active_ids: Vec<i64> = merged.keys().cloned().collect();
        let contradiction_edges = self.graph.contradiction_pairs(&active_ids);

        let mut contradiction_pairs: Vec<ContradictionPair> = Vec::new();
        for (a_id, b_id) in &contradiction_edges {
            if let (Some(&a_idx), Some(&b_idx)) = (
                self.memory_index.get(a_id),
                self.memory_index.get(b_id),
            ) {
                let a = &self.memories[a_idx];
                let b = &self.memories[b_idx];
                let a_act = merged.get(a_id).map(|e| e.0).unwrap_or(0.0);
                let b_act = merged.get(b_id).map(|e| e.0).unwrap_or(0.0);
                let a_age = ((now - parse_datetime_approx(&a.created_at)) / 86400.0) as f32;
                let b_age = ((now - parse_datetime_approx(&b.created_at)) / 86400.0) as f32;

                let (a_new, b_new, a_won) = resolve_interference(
                    a_act, a.decay_factor, a.importance, a_age,
                    b_act, b.decay_factor, b.importance, b_age,
                );

                if let Some(e) = merged.get_mut(a_id) {
                    e.0 = a_new;
                }
                if let Some(e) = merged.get_mut(b_id) {
                    e.0 = b_new;
                }

                let (winner_id, loser_id, winner_act, loser_act) = if a_won {
                    (*a_id, *b_id, a_new, b_new)
                } else {
                    (*b_id, *a_id, b_new, a_new)
                };

                contradiction_pairs.push(ContradictionPair {
                    winner_id,
                    loser_id,
                    winner_activation: winner_act,
                    loser_activation: loser_act,
                    reason: "contradiction_edge".to_string(),
                });
            }
        }

        // Hebbian: strengthen edges between co-activated top memories
        let mut top_ids: Vec<(i64, f32)> = merged.iter()
            .map(|(&id, &(act, _, _))| (id, act))
            .collect();
        top_ids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top_5: Vec<i64> = top_ids.iter().take(5).map(|(id, _)| *id).collect();
        for i in 0..top_5.len() {
            for j in (i + 1)..top_5.len() {
                self.graph.strengthen_edge(top_5[i], top_5[j], 0.01);
                self.graph.strengthen_edge(top_5[j], top_5[i], 0.01);
            }
        }

        // Recall boost + update activations
        for (id, (activation, _, _)) in &mut merged {
            if let Some(&idx) = self.memory_index.get(id) {
                self.memories[idx].activation = *activation;
                self.memories[idx].decay_factor = apply_recall_boost(self.memories[idx].decay_factor);
                self.memories[idx].access_count += 1;
                self.memories[idx].last_activated = now;
                // Boost the substrate strength too
                self.substrate.store(*id, &self.memories[idx].pattern.clone(), self.memories[idx].decay_factor);
            }
        }

        // Build response: top_k activated memories
        let mut activated: Vec<ActivatedMemory> = merged.iter()
            .filter_map(|(&id, &(activation, source, hops))| {
                self.memory_index.get(&id).map(|&idx| {
                    let m = &self.memories[idx];
                    ActivatedMemory {
                        id,
                        content: m.content.clone(),
                        category: m.category.clone(),
                        activation,
                        source: source.to_string(),
                        hops,
                        decay_factor: m.decay_factor,
                        importance: m.importance,
                        created_at: m.created_at.clone(),
                    }
                })
            })
            .collect();

        activated.sort_by(|a, b| b.activation.partial_cmp(&a.activation).unwrap_or(std::cmp::Ordering::Equal));
        activated.truncate(top_k);

        QueryResult {
            activated,
            contradictions: contradiction_pairs,
            total_patterns: self.memories.len(),
            query_time_ms: t0.elapsed().as_secs_f64() * 1000.0,
        }
    }

    fn absorb_new(&mut self, mut memory: BrainMemory) {
        // Don't absorb if already known
        if self.memory_index.contains_key(&memory.id) {
            return;
        }

        let existing_snapshot: Vec<BrainMemory> = self.memories.iter().cloned().collect();

        absorb_memory(
            &mut memory,
            &existing_snapshot,
            &self.pca,
            &mut self.substrate,
            &mut self.graph,
        );

        let idx = self.memories.len();
        self.memory_index.insert(memory.id, idx);

        // Persist new edges if we have a DB connection
        if let Some(ref conn) = self.conn {
            if let Some(edges) = self.graph.adjacency.get(&memory.id) {
                for &(target_id, weight, ref etype) in edges {
                    let edge = BrainEdge {
                        source_id: memory.id,
                        target_id,
                        weight,
                        edge_type: etype.clone(),
                        created_at: memory.created_at.clone(),
                    };
                    let _ = save_edge(conn, &edge);
                }
            }
        }

        self.memories.push(memory);
    }

    fn decay_tick(&mut self, ticks: u32) {
        let dead_ids: Vec<i64> = self.memories.iter()
            .filter(|m| {
                let new_decay = compute_pattern_decay(m.decay_factor, ticks, m.importance);
                is_dead(new_decay)
            })
            .map(|m| m.id)
            .collect();

        // Apply decay
        for mem in &mut self.memories {
            mem.decay_factor = compute_pattern_decay(mem.decay_factor, ticks, mem.importance);
        }

        // Remove dead patterns
        for id in &dead_ids {
            self.substrate.remove(*id);
        }

        // Decay graph edges
        for _ in 0..ticks {
            self.graph.decay_edges(EDGE_DECAY_RATE, 0.01);
        }

        // Rebuild memory_index (remove dead)
        if !dead_ids.is_empty() {
            let dead_set: std::collections::HashSet<i64> = dead_ids.into_iter().collect();
            self.memories.retain(|m| !dead_set.contains(&m.id));
            self.memory_index.clear();
            for (i, m) in self.memories.iter().enumerate() {
                self.memory_index.insert(m.id, i);
            }
        }
    }

    fn get_stats(&self) -> StatsResult {
        let total_patterns = self.memories.len();
        let total_edges = self.graph.total_edges();

        let avg_activation = if total_patterns > 0 {
            self.memories.iter().map(|m| m.activation).sum::<f32>() / total_patterns as f32
        } else {
            0.0
        };

        let avg_decay_factor = if total_patterns > 0 {
            self.memories.iter().map(|m| m.decay_factor).sum::<f32>() / total_patterns as f32
        } else {
            0.0
        };

        let mut health_dist: HashMap<String, usize> = HashMap::new();
        for m in &self.memories {
            let h = classify_health(m.decay_factor).to_string();
            *health_dist.entry(h).or_insert(0) += 1;
        }

        let mut sorted_by_act = self.memories.iter().collect::<Vec<_>>();
        sorted_by_act.sort_by(|a, b| b.activation.partial_cmp(&a.activation).unwrap_or(std::cmp::Ordering::Equal));

        let top_activated: Vec<serde_json::Value> = sorted_by_act.iter().take(10).map(|m| {
            json!({
                "id": m.id,
                "content_preview": m.content_preview(80),
                "activation": m.activation,
            })
        }).collect();

        let mut sorted_by_decay = self.memories.iter().collect::<Vec<_>>();
        sorted_by_decay.sort_by(|a, b| a.decay_factor.partial_cmp(&b.decay_factor).unwrap_or(std::cmp::Ordering::Equal));

        let bottom_activated: Vec<serde_json::Value> = sorted_by_decay.iter().take(10).map(|m| {
            json!({
                "id": m.id,
                "content_preview": m.content_preview(80),
                "decay_factor": m.decay_factor,
            })
        }).collect();

        StatsResult {
            total_patterns,
            total_edges,
            avg_activation,
            avg_decay_factor,
            health_distribution: health_dist,
            top_activated,
            bottom_activated,
        }
    }
}

fn write_response(resp: &Response) {
    let line = serde_json::to_string(resp).unwrap_or_else(|_| r#"{"ok":false,"cmd":"error","error":"serialization failed"}"#.to_string());
    println!("{}", line);
    let _ = io::stdout().flush();
}

fn main() {
    let stdin = io::stdin();
    let mut brain = Brain::new();

    eprintln!("[brain] engram-brain started, waiting for commands");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let cmd: Command = match serde_json::from_str(&line) {
            Ok(c) => c,
            Err(e) => {
                let resp = Response::err("unknown", None, format!("parse error: {}", e));
                write_response(&resp);
                continue;
            }
        };

        let seq = cmd.seq();
        let _cmd_name = cmd.cmd_name().to_string();

        match cmd {
            Command::Init { db_path, .. } => {
                match brain.init(&db_path) {
                    Ok(msg) => {
                        eprintln!("[brain] {}", msg);
                        let resp = Response::ok("init", seq, json!({ "message": msg }));
                        write_response(&resp);
                    }
                    Err(e) => {
                        eprintln!("[brain] init error: {}", e);
                        let resp = Response::err("init", seq, e);
                        write_response(&resp);
                    }
                }
            }

            Command::Query { embedding, top_k, beta, spread_hops, .. } => {
                let top_k = top_k.unwrap_or(10);
                let beta = beta.unwrap_or(DEFAULT_BETA);
                let spread_hops = spread_hops.unwrap_or(2);

                let result = brain.query(&embedding, top_k, beta, spread_hops);
                let resp = Response::ok("query", seq, serde_json::to_value(result).unwrap_or(json!({})));
                write_response(&resp);
            }

            Command::Absorb { id, content, category, source, importance, created_at, embedding, tags, .. } => {
                let mem = BrainMemory {
                    id,
                    content,
                    category,
                    source,
                    importance,
                    created_at,
                    embedding,
                    pattern: ndarray::Array1::zeros(BRAIN_DIM),
                    activation: 0.5,
                    last_activated: 0.0,
                    access_count: 0,
                    decay_factor: 1.0,
                    tags: tags.unwrap_or_default(),
                };
                brain.absorb_new(mem);
                let resp = Response::ok("absorb", seq, json!({ "absorbed": true, "total": brain.memories.len() }));
                write_response(&resp);
            }

            Command::DecayTick { ticks, .. } => {
                let ticks = ticks.unwrap_or(1);
                let before = brain.memories.len();
                brain.decay_tick(ticks);
                let after = brain.memories.len();
                let resp = Response::ok("decay_tick", seq, json!({
                    "ticks": ticks,
                    "before": before,
                    "after": after,
                    "removed": before - after,
                }));
                write_response(&resp);
            }

            Command::GetStats { .. } => {
                let stats = brain.get_stats();
                let resp = Response::ok("get_stats", seq, serde_json::to_value(stats).unwrap_or(json!({})));
                write_response(&resp);
            }

            Command::Shutdown { .. } => {
                let resp = Response::ok("shutdown", seq, json!({ "message": "shutting down" }));
                write_response(&resp);
                eprintln!("[brain] shutdown command received");
                break;
            }
        }
    }

    eprintln!("[brain] exiting");
}
