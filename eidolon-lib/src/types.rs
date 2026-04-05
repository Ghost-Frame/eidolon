use ndarray::Array1;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BRAIN_DIM: usize = 512;
pub const RAW_DIM: usize = 1024;

#[derive(Debug, Clone)]
pub struct BrainMemory {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub source: String,
    pub importance: i32,
    pub created_at: String,
    pub embedding: Vec<f32>,
    pub pattern: Array1<f32>,
    pub activation: f32,
    pub last_activated: f64,
    pub access_count: u32,
    pub decay_factor: f32,
    pub tags: Vec<String>,
}

impl BrainMemory {
    pub fn content_preview(&self, max_len: usize) -> String {
        if self.content.len() <= max_len {
            self.content.clone()
        } else {
            let boundary = self.content.char_indices()
                .nth(max_len)
                .map(|(i, _)| i)
                .unwrap_or(self.content.len());
            format!("{}...", &self.content[..boundary])
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EdgeType {
    Association,
    Temporal,
    Contradiction,
    Causal,
    Resolves,
}

impl EdgeType {
    pub fn as_str(&self) -> &str {
        match self {
            EdgeType::Association => "association",
            EdgeType::Temporal => "temporal",
            EdgeType::Contradiction => "contradiction",
            EdgeType::Causal => "causal",
            EdgeType::Resolves => "resolves",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "temporal" => EdgeType::Temporal,
            "contradiction" => EdgeType::Contradiction,
            "causal" => EdgeType::Causal,
            "resolves" => EdgeType::Resolves,
            _ => EdgeType::Association,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrainEdge {
    pub source_id: i64,
    pub target_id: i64,
    pub weight: f32,
    pub edge_type: EdgeType,
    pub created_at: String,
}

// JSON protocol types

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Init {
        seq: Option<u64>,
        db_path: String,
        data_dir: Option<String>,
    },
    Query {
        seq: Option<u64>,
        embedding: Vec<f32>,
        top_k: Option<usize>,
        beta: Option<f32>,
        spread_hops: Option<usize>,
    },
    Absorb {
        seq: Option<u64>,
        id: i64,
        content: String,
        category: String,
        source: String,
        importance: i32,
        created_at: String,
        embedding: Vec<f32>,
        tags: Option<Vec<String>>,
    },
    DecayTick {
        seq: Option<u64>,
        ticks: Option<u32>,
    },
    GetStats {
        seq: Option<u64>,
    },
    DreamCycle {
        seq: Option<u64>,
    },
    // Evolution commands - only meaningful when compiled with --features evolution
    // When evolution is disabled, these return { ok: false, error: "evolution not enabled" }
    FeedbackSignal {
        seq: Option<u64>,
        memory_ids: Vec<i64>,
        edge_pairs: Vec<[i64; 2]>,
        useful: bool,
    },
    EvolutionTrain {
        seq: Option<u64>,
    },
    EvolutionStats {
        seq: Option<u64>,
    },
    Shutdown {
        seq: Option<u64>,
    },
    GenerateInstincts {
        seq: Option<u64>,
        output_path: String,
    },
}

impl Command {
    pub fn seq(&self) -> Option<u64> {
        match self {
            Command::Init { seq, .. } => *seq,
            Command::Query { seq, .. } => *seq,
            Command::Absorb { seq, .. } => *seq,
            Command::DecayTick { seq, .. } => *seq,
            Command::GetStats { seq, .. } => *seq,
            Command::DreamCycle { seq, .. } => *seq,
            Command::FeedbackSignal { seq, .. } => *seq,
            Command::EvolutionTrain { seq, .. } => *seq,
            Command::EvolutionStats { seq, .. } => *seq,
            Command::Shutdown { seq, .. } => *seq,
            Command::GenerateInstincts { seq, .. } => *seq,
        }
    }

    pub fn cmd_name(&self) -> &str {
        match self {
            Command::Init { .. } => "init",
            Command::Query { .. } => "query",
            Command::Absorb { .. } => "absorb",
            Command::DecayTick { .. } => "decay_tick",
            Command::GetStats { .. } => "get_stats",
            Command::DreamCycle { .. } => "dream_cycle",
            Command::FeedbackSignal { .. } => "feedback_signal",
            Command::EvolutionTrain { .. } => "evolution_train",
            Command::EvolutionStats { .. } => "evolution_stats",
            Command::Shutdown { .. } => "shutdown",
            Command::GenerateInstincts { .. } => "generate_instincts",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Response {
    pub fn ok(cmd: &str, seq: Option<u64>, data: Value) -> Self {
        Response {
            ok: true,
            cmd: cmd.to_string(),
            seq,
            error: None,
            data: Some(data),
        }
    }

    pub fn err(cmd: &str, seq: Option<u64>, error: String) -> Self {
        Response {
            ok: false,
            cmd: cmd.to_string(),
            seq,
            error: Some(error),
            data: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ActivatedMemory {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub activation: f32,
    pub source: String,
    pub hops: usize,
    pub decay_factor: f32,
    pub importance: i32,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ContradictionPair {
    pub winner_id: i64,
    pub loser_id: i64,
    pub winner_activation: f32,
    pub loser_activation: f32,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub activated: Vec<ActivatedMemory>,
    pub contradictions: Vec<ContradictionPair>,
    #[cfg(feature = "reasoning")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inferences: Vec<Inference>,
    pub total_patterns: usize,
    pub query_time_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct StatsResult {
    pub total_patterns: usize,
    pub total_edges: usize,
    pub avg_activation: f32,
    pub avg_decay_factor: f32,
    pub health_distribution: std::collections::HashMap<String, usize>,
    pub top_activated: Vec<serde_json::Value>,
    pub bottom_activated: Vec<serde_json::Value>,
}

// ---- Evolution stats result (serialized in JSON responses) ----

#[derive(Debug, Serialize)]
pub struct EvolutionStatsResult {
    pub generation: u32,
    pub num_node_weights: usize,
    pub num_edge_weights: usize,
    pub learning_rate: f32,
}

// ---- Reasoning types (feature-gated) ----

#[cfg(feature = "reasoning")]
#[derive(Debug, Clone, Serialize)]
pub struct Inference {
    pub id: String,
    pub kind: InferenceKind,
    pub content: String,
    pub confidence: f32,
    pub source_ids: Vec<i64>,
    pub source_edges: Vec<(i64, i64)>,
}

#[cfg(feature = "reasoning")]
#[derive(Debug, Clone, Serialize)]
pub enum InferenceKind {
    Abductive,
    Predictive,
    Synthesis,
    Rule,
    Analogical,
}

#[cfg(feature = "reasoning")]
#[derive(Debug, Clone)]
pub struct ReasoningConfig {
    pub abductive: bool,
    pub predictive: bool,
    pub synthesis: bool,
    pub rule_extraction: bool,
    pub analogical: bool,
    pub max_inferences: usize,
    pub min_confidence: f32,
}

#[cfg(feature = "reasoning")]
impl Default for ReasoningConfig {
    fn default() -> Self {
        ReasoningConfig {
            abductive: true,
            predictive: true,
            synthesis: true,
            rule_extraction: true,
            analogical: false, // Most expensive, disabled by default
            max_inferences: 5,
            min_confidence: 0.3,
        }
    }
}
