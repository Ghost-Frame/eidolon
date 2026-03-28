// ============================================================================
// Brain types -- shared TypeScript types for the brain JSON protocol
// and brain.db schema constants.
// ============================================================================

// ---- JSON Protocol: Commands (TypeScript -> Native) ----

export type BrainCommand =
  | { cmd: "init"; seq?: number; db_path: string; data_dir: string }
  | { cmd: "query"; seq?: number; embedding: number[]; top_k?: number; beta?: number; spread_hops?: number }
  | { cmd: "absorb"; seq?: number; id: number; content: string; category: string; source: string; importance: number; created_at: string; embedding: number[]; tags?: string[] }
  | { cmd: "decay_tick"; seq?: number; ticks?: number }
  | { cmd: "get_stats"; seq?: number }
  | { cmd: "shutdown"; seq?: number };

// ---- JSON Protocol: Responses (Native -> TypeScript) ----

export interface BrainResponse {
  ok: boolean;
  cmd: string;
  seq?: number;
  error?: string;
  data?: unknown;
}

// ---- Activated memory returned in query results ----

export interface ActivatedMemory {
  id: number;
  content: string;
  category: string;
  activation: number;
  source: "hopfield" | "spread" | "both";
  hops: number;
  decay_factor: number;
  importance: number;
  created_at: string;
}

// ---- Contradiction pair detected during interference resolution ----

export interface ContradictionPair {
  winner_id: number;
  loser_id: number;
  winner_activation: number;
  loser_activation: number;
  reason: string;
}

// ---- Full query result ----

export interface BrainQueryResult {
  activated: ActivatedMemory[];
  contradictions: ContradictionPair[];
  total_patterns: number;
  query_time_ms: number;
}

// ---- Brain substrate statistics ----

export interface BrainStats {
  total_patterns: number;
  total_edges: number;
  avg_activation: number;
  avg_decay_factor: number;
  health_distribution: Record<string, number>;
  top_activated: Array<{ id: number; content_preview: string; activation: number }>;
  bottom_activated: Array<{ id: number; content_preview: string; decay_factor: number }>;
}

// ---- Stats produced by the curation pipeline ----

export interface CurationStats {
  total_source: number;
  passed_filter: number;
  deduped: number;
  noise_removed: number;
  curated: number;
  edges_seeded: number;
}

// ---- brain.db schema ----

export const BRAIN_SCHEMA = `
CREATE TABLE IF NOT EXISTS brain_memories (
  id         INTEGER PRIMARY KEY,
  content    TEXT NOT NULL,
  category   TEXT NOT NULL DEFAULT 'general',
  source     TEXT NOT NULL DEFAULT 'unknown',
  importance INTEGER NOT NULL DEFAULT 5,
  created_at TEXT NOT NULL,
  embedding  BLOB,
  tags       TEXT,
  curated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS brain_edges (
  source_id  INTEGER NOT NULL,
  target_id  INTEGER NOT NULL,
  weight     REAL NOT NULL DEFAULT 1.0,
  edge_type  TEXT NOT NULL DEFAULT 'association',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(source_id, target_id)
);

CREATE TABLE IF NOT EXISTS brain_meta (
  key        TEXT PRIMARY KEY,
  value      BLOB,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_brain_edges_source ON brain_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_brain_edges_target ON brain_edges(target_id);
`;
