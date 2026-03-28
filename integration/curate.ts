#!/usr/bin/env node
// ============================================================================
// Curation pipeline: memory.db -> brain.db
// Multi-pass: noise filter -> SimHash dedup -> insert survivors -> seed edges
//
// Run: cd ~/engram && node --experimental-strip-types src/services/brain/curate.ts
// ============================================================================

import { resolve } from "path";
import { existsSync, unlinkSync } from "fs";
import Database from "libsql";
import { BRAIN_SCHEMA, type CurationStats } from "./types.ts";

// ---- Constants ----

const MIN_CONTENT_LENGTH = 20;
const MAX_CONTENT_LENGTH = 10000;
const SIMHASH_HAMMING_THRESHOLD = 5;

// Noise patterns: deployment logs, debug output, raw commands, stack traces,
// session chatter. Order doesn't matter -- all are checked.
const NOISE_PATTERNS: RegExp[] = [
  // Stack traces
  /^\s+at\s+\w/m,
  /Error:\s+.*\n\s+at/m,
  // Deployment / CI logs
  /\[?(?:INFO|WARN|ERROR|DEBUG|TRACE)\]?\s+\d{4}-\d{2}-\d{2}/i,
  /\b(?:npm (?:install|run|build|test)|yarn (?:install|build)|pnpm install)\b/i,
  /\b(?:Downloading|Installing|Resolving|Fetching)\s+packages?\b/i,
  // Raw shell commands (lines that are mostly shell syntax)
  /^(?:sudo |root@|deploy@|\$\s+|#\s+)[a-z].*(?:&&|\|\||;)/m,
  /^\s*(?:docker|kubectl|systemctl|journalctl|nginx|apt|dnf|yum)\s+/m,
  // Git noise
  /^(?:commit [0-9a-f]{40}|diff --git|index [0-9a-f]+|---\s+a\/|@@ -\d)/m,
  // HTTP access logs
  /\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b.*"(?:GET|POST|PUT|DELETE|HEAD)/,
  // Session/debug chatter
  /^(?:ok:|fail:|pass:|skip:|todo:)\s+/im,
  /\[brain\]|\[engram\]|\[thymus\]|\[chiasm\]/i,
  // JSON dumps (raw JSON blobs stored as memories)
  /^\s*\{"\w+":\s*(?:"|{|\[|\d)/,
  // Binary/hash noise
  /^[0-9a-f]{32,}$/i,
  // Repeated punctuation or whitespace-only
  /^[\s\-=_*#]{5,}$/,
];

// ---- SimHash (self-contained, no dependency on src/memory/simhash.ts) ----

function fnv1a(str: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < str.length; i++) {
    hash ^= str.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

function computeSimHash(text: string): string {
  const tokens = text
    .toLowerCase()
    .replace(/[^a-z0-9\s]/g, " ")
    .split(/\s+/)
    .filter(t => t.length >= 3);
  const unique = [...new Set(tokens)];

  if (unique.length === 0) return "0".repeat(16);

  const vec = new Int32Array(64);

  for (const token of unique) {
    const h1 = fnv1a(token);
    const h2 = fnv1a(token + "\x00");
    for (let i = 0; i < 32; i++) {
      vec[i] += (h1 & (1 << i)) ? 1 : -1;
      vec[32 + i] += (h2 & (1 << i)) ? 1 : -1;
    }
  }

  let hex = "";
  for (let nibbleIdx = 0; nibbleIdx < 16; nibbleIdx++) {
    let nibble = 0;
    for (let bit = 0; bit < 4; bit++) {
      const vecIdx = nibbleIdx * 4 + bit;
      if (vec[vecIdx] > 0) nibble |= (1 << bit);
    }
    hex += nibble.toString(16);
  }
  return hex;
}

function hammingDistance(a: string, b: string): number {
  if (a.length !== b.length) return 64;
  let dist = 0;
  for (let i = 0; i < a.length; i++) {
    const xor = parseInt(a[i], 16) ^ parseInt(b[i], 16);
    dist += ((xor & 1) + ((xor >> 1) & 1) + ((xor >> 2) & 1) + ((xor >> 3) & 1));
  }
  return dist;
}

// ---- Noise filter ----

function isNoise(content: string): boolean {
  if (content.length < MIN_CONTENT_LENGTH) return true;
  if (content.length > MAX_CONTENT_LENGTH) return true;
  for (const pattern of NOISE_PATTERNS) {
    if (pattern.test(content)) return true;
  }
  return false;
}

// ---- Main pipeline ----

async function curate(): Promise<void> {
  const dataDir = process.env.ENGRAM_DATA_DIR
    ? resolve(process.env.ENGRAM_DATA_DIR)
    : resolve(process.cwd(), "data");

  const memoryDbPath = resolve(dataDir, "memory.db");
  const brainDbPath = resolve(dataDir, "brain.db");

  if (!existsSync(memoryDbPath)) {
    console.error(`memory.db not found at ${memoryDbPath}`);
    process.exit(1);
  }

  // Delete any existing brain.db to start fresh (curation is idempotent)
  if (existsSync(brainDbPath)) {
    unlinkSync(brainDbPath);
    console.log("Deleted existing brain.db");
  }

  // Open memory.db read-only, create fresh brain.db
  const mem = new Database(memoryDbPath, { flags: 0x00000001 }); // SQLITE_OPEN_READONLY
  const brain = new Database(brainDbPath);

  // Apply brain schema
  brain.exec(BRAIN_SCHEMA);

  const stats: CurationStats = {
    total_source: 0,
    passed_filter: 0,
    deduped: 0,
    noise_removed: 0,
    curated: 0,
    edges_seeded: 0,
  };

  // ---- Pass 1 + 2: Load memories, noise filter, SimHash dedup ----

  const rows = mem.prepare(
    `SELECT id, content, category, source, importance, created_at, embedding, tags
     FROM memories
     WHERE is_forgotten = 0
       AND content IS NOT NULL
       AND content != ''
     ORDER BY created_at ASC`
  ).all() as Array<{
    id: number;
    content: string;
    category: string;
    source: string;
    importance: number;
    created_at: string;
    embedding: Buffer | null;
    tags: string | null;
  }>;

  stats.total_source = rows.length;
  console.log(`Source memories: ${stats.total_source}`);

  const survivors: typeof rows = [];
  const hashes: string[] = [];

  for (const row of rows) {
    // Noise filter
    if (isNoise(row.content)) {
      stats.noise_removed++;
      continue;
    }
    stats.passed_filter++;

    // SimHash dedup
    const hash = computeSimHash(row.content);
    let isDuplicate = false;
    for (const existing of hashes) {
      if (hammingDistance(hash, existing) <= SIMHASH_HAMMING_THRESHOLD) {
        isDuplicate = true;
        break;
      }
    }
    if (isDuplicate) {
      stats.deduped++;
      continue;
    }

    hashes.push(hash);
    survivors.push(row);
  }

  console.log(`After noise filter: ${stats.passed_filter} (removed ${stats.noise_removed})`);
  console.log(`After SimHash dedup: ${survivors.length} (removed ${stats.deduped})`);

  // ---- Pass 3: Insert survivors into brain_memories ----

  const insertMemory = brain.prepare(
    `INSERT INTO brain_memories (id, content, category, source, importance, created_at, embedding, tags, curated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))`
  );

  const survivorIds = new Set<number>();
  const insertTxn = brain.transaction(() => {
    for (const row of survivors) {
      // libsql returns ArrayBuffer for BLOBs; convert to Buffer for brain.db insertion
      let embeddingBuf: Buffer | null = null;
      if (row.embedding) {
        if (Buffer.isBuffer(row.embedding)) {
          embeddingBuf = row.embedding;
        } else if (row.embedding instanceof ArrayBuffer) {
          embeddingBuf = Buffer.from(row.embedding);
        } else if (row.embedding instanceof Uint8Array) {
          embeddingBuf = Buffer.from(row.embedding.buffer, row.embedding.byteOffset, row.embedding.byteLength);
        }
      }
      insertMemory.run(
        row.id,
        row.content,
        row.category || "general",
        row.source || "unknown",
        row.importance ?? 5,
        row.created_at,
        embeddingBuf,
        row.tags,
      );
      survivorIds.add(row.id);
      stats.curated++;
    }
  });
  insertTxn();

  console.log(`Inserted ${stats.curated} memories into brain.db`);

  // ---- Pass 4: Seed edges from memory_links ----

  // Check if memory_links table exists
  const linksTableExists = mem.prepare(
    "SELECT name FROM sqlite_master WHERE type='table' AND name='memory_links'"
  ).get();

  if (linksTableExists) {
    const links = mem.prepare(
      `SELECT source_id, target_id, similarity, type FROM memory_links`
    ).all() as Array<{
      source_id: number;
      target_id: number;
      similarity: number;
      type: string;
    }>;

    const insertEdge = brain.prepare(
      `INSERT OR IGNORE INTO brain_edges (source_id, target_id, weight, edge_type, created_at)
       VALUES (?, ?, ?, ?, datetime('now'))`
    );

    const edgeTxn = brain.transaction(() => {
      for (const link of links) {
        // Only seed edges where both endpoints survived curation
        if (!survivorIds.has(link.source_id) || !survivorIds.has(link.target_id)) continue;

        const edgeType = link.type === "contradiction" ? "contradiction" : "association";
        const weight = Math.max(0.1, Math.min(1.0, link.similarity || 0.5));

        // Bidirectional edges
        insertEdge.run(link.source_id, link.target_id, weight, edgeType);
        insertEdge.run(link.target_id, link.source_id, weight, edgeType);
        stats.edges_seeded += 2;
      }
    });
    edgeTxn();
  }

  console.log(`Seeded ${stats.edges_seeded} edges`);

  // ---- Pass 5: Write stats to brain_meta ----

  const now = new Date().toISOString();
  brain.prepare(
    `INSERT OR REPLACE INTO brain_meta (key, value, updated_at) VALUES ('curation_stats', ?, ?)`
  ).run(JSON.stringify(stats), now);
  brain.prepare(
    `INSERT OR REPLACE INTO brain_meta (key, value, updated_at) VALUES ('curated_at', ?, ?)`
  ).run(now, now);

  mem.close();
  brain.close();

  console.log("\nCuration complete:");
  console.log(`  total_source:   ${stats.total_source}`);
  console.log(`  noise_removed:  ${stats.noise_removed}`);
  console.log(`  deduped:        ${stats.deduped}`);
  console.log(`  curated:        ${stats.curated}`);
  console.log(`  edges_seeded:   ${stats.edges_seeded}`);
  console.log(`\nbrain.db written to: ${brainDbPath}`);
}

curate().catch(e => {
  console.error("Curation failed:", e);
  process.exit(1);
});
