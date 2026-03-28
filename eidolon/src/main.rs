use std::io::{self, BufRead, Write};

use serde_json::json;

use eidolon_lib::brain::Brain;
use eidolon_lib::instincts::{generate_instincts, save_instincts};
use eidolon_lib::substrate::DEFAULT_BETA;
use eidolon_lib::types::{
    BrainMemory, Command, Response,
    BRAIN_DIM,
};

#[cfg(feature = "evolution")]
use eidolon_lib::types::EvolutionStatsResult;

#[cfg(feature = "evolution")]
use eidolon_lib::evolution::FeedbackSignal;

fn write_response(resp: &Response) {
    let line = serde_json::to_string(resp).unwrap_or_else(|_| r#"{"ok":false,"cmd":"error","error":"serialization failed"}"#.to_string());
    println!("{}", line);
    let _ = io::stdout().flush();
}

fn main() {
    let stdin = io::stdin();
    let mut brain = Brain::new();

    eprintln!("[brain] eidolon started, waiting for commands");

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
            Command::Init { db_path, data_dir, .. } => {
                match brain.init(&db_path, data_dir.as_deref()) {
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

            Command::DreamCycle { .. } => {
                let result = brain.run_dream_cycle();
                let resp = Response::ok("dream_cycle", seq, serde_json::to_value(result).unwrap_or(json!({})));
                write_response(&resp);
            }

            Command::FeedbackSignal { memory_ids, edge_pairs, useful, .. } => {
                #[cfg(feature = "evolution")]
                {
                    brain.evolution_feedback(memory_ids, edge_pairs, useful);
                    let resp = Response::ok("feedback_signal", seq, json!({ "recorded": true }));
                    write_response(&resp);
                }
                #[cfg(not(feature = "evolution"))]
                {
                    let _ = (memory_ids, edge_pairs, useful);
                    let resp = Response::err("feedback_signal", seq, "evolution not enabled".to_string());
                    write_response(&resp);
                }
            }

            Command::EvolutionTrain { .. } => {
                #[cfg(feature = "evolution")]
                {
                    let generation = brain.evolution_train();
                    let resp = Response::ok("evolution_train", seq, json!({ "generation": generation }));
                    write_response(&resp);
                }
                #[cfg(not(feature = "evolution"))]
                {
                    let resp = Response::err("evolution_train", seq, "evolution not enabled".to_string());
                    write_response(&resp);
                }
            }

            Command::EvolutionStats { .. } => {
                #[cfg(feature = "evolution")]
                {
                    let stats = brain.evolution_stats();
                    let resp = Response::ok("evolution_stats", seq, serde_json::to_value(stats).unwrap_or(json!({})));
                    write_response(&resp);
                }
                #[cfg(not(feature = "evolution"))]
                {
                    let resp = Response::err("evolution_stats", seq, "evolution not enabled".to_string());
                    write_response(&resp);
                }
            }

            Command::GenerateInstincts { output_path, .. } => {
                let corpus = generate_instincts();
                let n_memories = corpus.memories.len();
                let n_edges = corpus.edges.len();
                match save_instincts(&corpus, &output_path) {
                    Ok(()) => {
                        let file_size = std::fs::metadata(&output_path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        let resp = Response::ok("generate_instincts", seq, json!({
                            "memories": n_memories,
                            "edges": n_edges,
                            "output_path": output_path,
                            "file_size_bytes": file_size,
                        }));
                        write_response(&resp);
                        eprintln!("[brain] generated instincts: {} memories, {} edges, {} bytes",
                            n_memories, n_edges, file_size);
                    }
                    Err(e) => {
                        let resp = Response::err("generate_instincts", seq, e);
                        write_response(&resp);
                    }
                }
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
