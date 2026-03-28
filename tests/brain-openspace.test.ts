// brain-openspace.test.ts
// OpenSpace scenario test for eidolon backends
//
// Creates a SYNTHETIC brain.db with a controlled GraphView/Engram consolidation
// scenario, runs both Rust and C++ backends against it, and validates results.
//
// Run: cd ~/engram && node --experimental-strip-types tests/brain-openspace.test.ts

import { spawn, ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import * as fs from "node:fs";
import * as path from "node:path";
import { execSync } from "node:child_process";

// ============================================================================
// Constants
// ============================================================================

const BACKENDS = [
  {
    name: "Rust",
    bin: "/opt/eidolon/eidolon/rust/target/release/eidolon",
  },
  {
    name: "C++",
    bin: "/opt/eidolon/eidolon/cpp/build/eidolon",
  },
];

const SYNTHETIC_DB = "/tmp/brain-openspace-test.db";
const DIM = 1024;

// ============================================================================
// Embedding helpers
// ============================================================================

function l2normalize(vec: number[]): number[] {
  let norm = 0;
  for (const v of vec) norm += v * v;
  norm = Math.sqrt(norm);
  if (norm < 1e-10) return vec;
  return vec.map((v) => v / norm);
}

// Base "exists" cluster: sin starting at phase 0.0
function existsBase(): number[] {
  const v: number[] = [];
  for (let d = 0; d < DIM; d++) v.push(Math.sin(d * 0.02));
  return l2normalize(v);
}

// Base "archived" cluster: sin starting at phase 1.0
function archivedBase(): number[] {
  const v: number[] = [];
  for (let d = 0; d < DIM; d++) v.push(Math.sin(1.0 + d * 0.02));
  return l2normalize(v);
}

// Bridge: average of exists and archived bases
function bridgeBase(): number[] {
  const e = existsBase();
  const a = archivedBase();
  const avg = e.map((v, i) => (v + a[i]) / 2);
  return l2normalize(avg);
}

// Add small deterministic noise to a base vector
function withNoise(base: number[], seed: number): number[] {
  return l2normalize(base.map((v, i) => v + 0.05 * Math.sin(seed * 7.3 + i * 0.37)));
}

// Filler embedding: sin with different phase offset
function fillerEmbedding(idx: number): number[] {
  const phase = 2.0 + idx * 0.5;
  const v: number[] = [];
  for (let d = 0; d < DIM; d++) v.push(Math.sin(phase + d * 0.02));
  return l2normalize(v);
}

// Query embedding: close to archived cluster (slight perturbation)
function queryEmbedding(): number[] {
  return withNoise(archivedBase(), 99);
}

// Encode float array as little-endian binary blob (4096 bytes for 1024 floats)
function floatsToBlob(floats: number[]): Buffer {
  const buf = Buffer.allocUnsafe(floats.length * 4);
  for (let i = 0; i < floats.length; i++) {
    buf.writeFloatLE(floats[i], i * 4);
  }
  return buf;
}

// ============================================================================
// Scenario memories
// ============================================================================

interface ScenarioMemory {
  id: number;
  content: string;
  category: string;
  source: string;
  importance: number;
  created_at: string; // ISO string
  embedding: number[];
  tags: string;
}

function daysAgo(n: number): string {
  const d = new Date(Date.now() - n * 86400_000);
  return d.toISOString().replace("T", " ").replace(/\.\d+Z$/, "");
}

const SCENARIO_MEMORIES: ScenarioMemory[] = [
  {
    id: 1,
    content: "GraphView is a standalone graph visualization tool running at port 8080",
    category: "infrastructure",
    source: "test",
    importance: 7,
    created_at: daysAgo(30),
    embedding: withNoise(existsBase(), 1),
    tags: "graphview,standalone,port-8080",
  },
  {
    id: 2,
    content: "Engram includes a built-in graph visualization module at src/gui/",
    category: "infrastructure",
    source: "test",
    importance: 8,
    created_at: daysAgo(5),
    embedding: bridgeBase(),
    tags: "engram,graph,native,gui",
  },
  {
    id: 3,
    content: "GraphView features were merged into Engram's native graph module during the consolidation",
    category: "task",
    source: "test",
    importance: 9,
    created_at: daysAgo(5),
    embedding: withNoise(archivedBase(), 3),
    tags: "graphview,engram,merged,consolidation",
  },
  {
    id: 4,
    content: "The standalone GraphView repository has been archived and is no longer maintained",
    category: "reference",
    source: "test",
    importance: 8,
    created_at: daysAgo(4),
    embedding: withNoise(archivedBase(), 4),
    tags: "graphview,archived,deprecated",
  },
  {
    id: 5,
    content: "GraphView exists as an independent project with its own deployment",
    category: "infrastructure",
    source: "test",
    importance: 6,
    created_at: daysAgo(60),
    embedding: withNoise(existsBase(), 5),
    tags: "graphview,standalone,independent",
  },
];

// 25 filler memories (IDs 6..30)
const FILLER_MEMORIES: ScenarioMemory[] = Array.from({ length: 25 }, (_, i) => ({
  id: i + 6,
  content: `Filler memory ${i + 1} -- unrelated infrastructure note for context padding`,
  category: "general",
  source: "test",
  importance: 5,
  created_at: daysAgo(10 + i),
  embedding: fillerEmbedding(i),
  tags: `filler,idx-${i}`,
}));

const ALL_MEMORIES = [...SCENARIO_MEMORIES, ...FILLER_MEMORIES];

// ============================================================================
// Edge definitions
// ============================================================================

interface Edge {
  source_id: number;
  target_id: number;
  weight: number;
  edge_type: string;
}

const EDGES: Edge[] = [
  // Contradictions
  { source_id: 1, target_id: 4, weight: 1.0, edge_type: "contradiction" },
  { source_id: 4, target_id: 1, weight: 1.0, edge_type: "contradiction" },
  { source_id: 5, target_id: 4, weight: 1.0, edge_type: "contradiction" },
  { source_id: 4, target_id: 5, weight: 1.0, edge_type: "contradiction" },
  { source_id: 5, target_id: 3, weight: 1.0, edge_type: "contradiction" },
  { source_id: 3, target_id: 5, weight: 1.0, edge_type: "contradiction" },

  // Associations
  { source_id: 1, target_id: 2, weight: 0.8, edge_type: "association" },
  { source_id: 2, target_id: 1, weight: 0.8, edge_type: "association" },
  { source_id: 2, target_id: 3, weight: 0.9, edge_type: "association" },
  { source_id: 3, target_id: 2, weight: 0.9, edge_type: "association" },
  { source_id: 3, target_id: 4, weight: 0.9, edge_type: "association" },
  { source_id: 4, target_id: 3, weight: 0.9, edge_type: "association" },
  { source_id: 1, target_id: 5, weight: 0.7, edge_type: "association" },
  { source_id: 5, target_id: 1, weight: 0.7, edge_type: "association" },
];

// ============================================================================
// Create synthetic brain.db
// ============================================================================

function createSyntheticDB(): void {
  // Remove any existing test db
  if (fs.existsSync(SYNTHETIC_DB)) {
    fs.unlinkSync(SYNTHETIC_DB);
  }

  // Build SQL script
  const lines: string[] = [];

  lines.push(`
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
`);

  // Write SQL to temp file, then pipe blobs via node sqlite API
  // We'll use node's built-in sqlite (available in Node 22.5+) or fall back to CLI with blobs

  // Check if node:sqlite is available
  let useNodeSqlite = false;
  try {
    // Node 22.5+ has experimental sqlite
    execSync("node -e \"import('node:sqlite').then(() => process.exit(0)).catch(() => process.exit(1))\"", {
      timeout: 3000,
      stdio: "ignore",
    });
    useNodeSqlite = true;
  } catch {
    useNodeSqlite = false;
  }

  if (!useNodeSqlite) {
    createSyntheticDBViaCLI();
  } else {
    createSyntheticDBViaNodeSqlite();
  }
}

function createSyntheticDBViaCLI(): void {
  // Write memories as individual SQL inserts with hex-encoded blobs
  const sqlLines: string[] = [];
  sqlLines.push("BEGIN;");
  sqlLines.push(`
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
`);

  for (const m of ALL_MEMORIES) {
    const blob = floatsToBlob(m.embedding);
    const hexBlob = "X'" + blob.toString("hex") + "'";
    const content = m.content.replace(/'/g, "''");
    const tags = m.tags.replace(/'/g, "''");
    sqlLines.push(
      `INSERT INTO brain_memories (id, content, category, source, importance, created_at, embedding, tags) VALUES (${m.id}, '${content}', '${m.category}', '${m.source}', ${m.importance}, '${m.created_at}', ${hexBlob}, '${tags}');`
    );
  }

  for (const e of EDGES) {
    sqlLines.push(
      `INSERT OR IGNORE INTO brain_edges (source_id, target_id, weight, edge_type) VALUES (${e.source_id}, ${e.target_id}, ${e.weight}, '${e.edge_type}');`
    );
  }

  sqlLines.push("COMMIT;");

  const sqlScript = sqlLines.join("\n");
  const tmpSql = "/tmp/openspace-seed.sql";
  fs.writeFileSync(tmpSql, sqlScript, "utf8");

  execSync(`sqlite3 "${SYNTHETIC_DB}" < "${tmpSql}"`, { stdio: "inherit" });
  fs.unlinkSync(tmpSql);
}

function createSyntheticDBViaNodeSqlite(): void {
  // Dynamic import for Node 22.5+ experimental sqlite
  // This is a fallback path -- CLI method is preferred
  createSyntheticDBViaCLI();
}

// ============================================================================
// Brain process runner
// ============================================================================

async function runQuery(
  bin: string,
  dbPath: string,
  queryEmb: number[]
): Promise<{ activated: Array<{ id: number; activation: number; source: string }>; contradictions: unknown[] }> {
  const proc: ChildProcess = spawn(bin, [], { stdio: ["pipe", "pipe", "pipe"] });

  const lineQueue: string[] = [];
  let pendingResolve: ((line: string) => void) | null = null;

  const rl = createInterface({ input: proc.stdout! });
  rl.on("line", (line: string) => {
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = null;
      resolve(line);
    } else {
      lineQueue.push(line);
    }
  });

  proc.stderr?.on("data", () => {
    // suppress stderr
  });

  function nextLine(): Promise<string> {
    if (lineQueue.length > 0) return Promise.resolve(lineQueue.shift()!);
    return new Promise<string>((resolve) => { pendingResolve = resolve; });
  }

  function send(cmd: unknown): void {
    proc.stdin!.write(JSON.stringify(cmd) + "\n");
  }

  async function sendWait(cmd: unknown): Promise<{ ok: boolean; data: unknown; error?: string }> {
    send(cmd);
    const line = await nextLine();
    return JSON.parse(line);
  }

  // Init
  await sendWait({ cmd: "init", seq: 1, db_path: dbPath, data_dir: path.dirname(dbPath) });

  // Query
  const resp = await sendWait({ cmd: "query", seq: 2, embedding: queryEmb, top_k: 10, spread_hops: 2 });

  // Shutdown
  send({ cmd: "shutdown", seq: 3 });

  await new Promise<void>((resolve) => {
    proc.on("close", () => resolve());
    setTimeout(() => { proc.kill(); resolve(); }, 3000);
  });

  const data = resp.data as {
    activated?: Array<{ id: number; activation: number; source: string }>;
    contradictions?: unknown[];
  };

  return {
    activated: data?.activated ?? [],
    contradictions: data?.contradictions ?? [],
  };
}

// ============================================================================
// Validation
// ============================================================================

interface ValidationResult {
  pass: boolean;
  label: string;
  detail: string;
}

function validate(
  backendName: string,
  activated: Array<{ id: number; activation: number; source: string }>,
  contradictions: unknown[]
): ValidationResult[] {
  const results: ValidationResult[] = [];

  const topIds = activated.slice(0, 10).map((m) => m.id);
  const activationMap = new Map(activated.map((m) => [m.id, m.activation]));

  // Check 1: Archive memories (3 and 4) appear in top results
  const archiveIds = [3, 4];
  const archivePresent = archiveIds.some((id) => topIds.includes(id));
  results.push({
    pass: archivePresent,
    label: "Archive memories (3, 4) in top results",
    detail: archivePresent
      ? `Found IDs: ${archiveIds.filter((id) => topIds.includes(id)).join(", ")}`
      : `Top IDs were: ${topIds.join(", ")}`,
  });

  // Check 2: Archive memories rank higher than stale "exists" memory (5)
  // At least one archive memory has higher activation than memory 5
  const mem5act = activationMap.get(5) ?? 0;
  const archiveHigher = archiveIds.some((id) => (activationMap.get(id) ?? 0) > mem5act);
  results.push({
    pass: archiveHigher,
    label: "Archive memories rank > stale exists (id=5)",
    detail: `mem3=${(activationMap.get(3) ?? 0).toFixed(4)}, mem4=${(activationMap.get(4) ?? 0).toFixed(4)}, mem5=${mem5act.toFixed(4)}`,
  });

  // Check 3: Contradictions array is non-empty
  const hasContradictions = contradictions.length > 0;
  results.push({
    pass: hasContradictions,
    label: "Contradictions array is non-empty",
    detail: `Found ${contradictions.length} contradiction(s)`,
  });

  // Check 4: Memory 2 (bridge/Engram module) appears via spreading
  const mem2present = topIds.includes(2);
  results.push({
    pass: mem2present,
    label: "Memory 2 (Engram graph module) appears via spread",
    detail: mem2present
      ? `ID 2 found at position ${topIds.indexOf(2) + 1}, activation=${(activationMap.get(2) ?? 0).toFixed(4)}`
      : `ID 2 not in top results. Top IDs: ${topIds.join(", ")}`,
  });

  return results;
}

// ============================================================================
// Main
// ============================================================================

async function main(): Promise<void> {
  console.log("=".repeat(70));
  console.log("  ENGRAM BRAIN OPENSPACE TEST");
  console.log("  Scenario: GraphView consolidation into Engram native graph module");
  console.log("=".repeat(70));

  // Step 1: Create synthetic DB
  console.log("\n[Setup] Creating synthetic brain.db...");
  createSyntheticDB();

  // Verify
  const countOut = execSync(`sqlite3 "${SYNTHETIC_DB}" "SELECT COUNT(*) FROM brain_memories; SELECT COUNT(*) FROM brain_edges;"`, {
    encoding: "utf8",
  }).trim();
  const [memCount, edgeCount] = countOut.split("\n");
  console.log(`[Setup] Memories: ${memCount}, Edges: ${edgeCount}`);

  // Step 2: Generate query embedding (close to archived cluster)
  const queryEmb = queryEmbedding();
  console.log(`[Setup] Query embedding generated (archived cluster, dim=${queryEmb.length})`);

  // Step 3: Print scenario summary
  console.log("\nScenario memories:");
  for (const m of SCENARIO_MEMORIES) {
    const cluster = m.id === 2 ? "bridge" : [1, 5].includes(m.id) ? "exists" : "archived";
    console.log(`  ID ${m.id} [${cluster}, importance=${m.importance}, age=${m.id === 1 ? 30 : m.id === 2 ? 5 : m.id === 3 ? 5 : m.id === 4 ? 4 : 60}d]: ${m.content.substring(0, 60)}...`);
  }
  console.log(`  + 25 filler memories`);

  // Step 4: Run both backends
  const allPassed: Record<string, boolean> = {};

  for (const backend of BACKENDS) {
    console.log(`\n${"=".repeat(70)}`);
    console.log(`  Backend: ${backend.name}`);
    console.log("=".repeat(70));

    let activated: Array<{ id: number; activation: number; source: string }> = [];
    let contradictions: unknown[] = [];

    try {
      const result = await runQuery(backend.bin, SYNTHETIC_DB, queryEmb);
      activated = result.activated;
      contradictions = result.contradictions;
    } catch (err) {
      console.error(`  ERROR running ${backend.name}: ${err}`);
      allPassed[backend.name] = false;
      continue;
    }

    console.log(`\nTop activated memories (${activated.length} total):`);
    for (const m of activated.slice(0, 10)) {
      const scenario = SCENARIO_MEMORIES.find((s) => s.id === m.id);
      const label = scenario ? ` <- SCENARIO: ${scenario.content.substring(0, 45)}...` : "";
      console.log(`  ID ${String(m.id).padStart(3)}: activation=${m.activation.toFixed(4)}, source=${m.source}${label}`);
    }

    console.log(`\nContradictions detected: ${contradictions.length}`);
    for (const c of contradictions as Array<{ winner_id: number; loser_id: number; reason: string }>) {
      console.log(`  winner=${c.winner_id} vs loser=${c.loser_id}: ${c.reason}`);
    }

    // Validate
    const validations = validate(backend.name, activated, contradictions);
    let backendPassed = true;

    console.log("\nValidation results:");
    for (const v of validations) {
      const mark = v.pass ? "PASS" : "FAIL";
      console.log(`  [${mark}] ${v.label}`);
      console.log(`        ${v.detail}`);
      if (!v.pass) backendPassed = false;
    }

    allPassed[backend.name] = backendPassed;
    console.log(`\n  ${backend.name} overall: ${backendPassed ? "ALL PASS" : "SOME FAILURES"}`);
  }

  // Step 5: Summary
  console.log(`\n${"=".repeat(70)}`);
  console.log("  FINAL SUMMARY");
  console.log("=".repeat(70));
  let anyFail = false;
  for (const [name, passed] of Object.entries(allPassed)) {
    const mark = passed ? "PASS" : "FAIL";
    console.log(`  ${name.padEnd(10)} ${mark}`);
    if (!passed) anyFail = true;
  }
  console.log("=".repeat(70));

  // Cleanup
  if (fs.existsSync(SYNTHETIC_DB)) {
    fs.unlinkSync(SYNTHETIC_DB);
    console.log("\n[Cleanup] Removed synthetic brain.db");
  }

  process.exit(anyFail ? 1 : 0);
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
