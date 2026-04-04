// brain-benchmark.ts
// Benchmarks both Rust and C++ eidolon backends against real brain.db
// Run: cd ~/engram && node --experimental-strip-types scripts/brain-benchmark.ts

import { spawn, ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import * as fs from "node:fs";

const DB_PATH = process.env.EIDOLON_DB_PATH || "./data/brain.db";
const DATA_DIR = process.env.EIDOLON_DATA_DIR || "./data";

const BACKENDS = [
  {
    name: "Rust",
    bin: process.env.EIDOLON_RUST_BIN || "./rust/target/release/eidolon",
  },
  {
    name: "C++",
    bin: process.env.EIDOLON_CPP_BIN || "./cpp/build/eidolon",
  },
];

const QUERY_COUNT = 100;

// Generate deterministic 1024-dim normalized embedding using sin pattern
function makeQueryEmbedding(i: number): number[] {
  const vec: number[] = new Array(1024);
  for (let d = 0; d < 1024; d++) {
    vec[d] = Math.sin(i * 0.1 + d * 0.01);
  }
  // L2 normalize
  let norm = 0;
  for (const v of vec) norm += v * v;
  norm = Math.sqrt(norm);
  for (let d = 0; d < 1024; d++) vec[d] /= norm;
  return vec;
}

// Read /proc/PID/status VmRSS in kB
function readVmRSS(pid: number): number {
  try {
    const status = fs.readFileSync(`/proc/${pid}/status`, "utf8");
    const match = status.match(/VmRSS:\s+(\d+)/);
    return match ? parseInt(match[1], 10) : 0;
  } catch {
    return 0;
  }
}

interface BenchResult {
  backend: string;
  initTimeMs: number;
  totalPatterns: number;
  queryLatencies: number[];
  avgLatencyMs: number;
  p50Ms: number;
  p95Ms: number;
  p99Ms: number;
  peakRssKb: number;
  errors: number;
}

async function runBackend(name: string, bin: string): Promise<BenchResult> {
  console.log(`\n[${name}] Starting benchmark...`);

  const proc: ChildProcess = spawn(bin, [], {
    stdio: ["pipe", "pipe", "pipe"],
  });

  const pid = proc.pid!;
  let seq = 0;

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

  // Capture stderr for diagnostics
  let stderrBuf = "";
  proc.stderr?.on("data", (d: Buffer) => {
    stderrBuf += d.toString();
  });

  function nextLine(): Promise<string> {
    if (lineQueue.length > 0) {
      return Promise.resolve(lineQueue.shift()!);
    }
    return new Promise<string>((resolve) => {
      pendingResolve = resolve;
    });
  }

  function sendCmd(cmd: unknown): void {
    const line = JSON.stringify(cmd) + "\n";
    proc.stdin!.write(line);
  }

  async function sendAndWait(cmd: unknown): Promise<{ ok: boolean; data: unknown; error?: string }> {
    sendCmd(cmd);
    const line = await nextLine();
    try {
      return JSON.parse(line);
    } catch {
      console.error(`[${name}] Failed to parse response: ${line}`);
      return { ok: false, data: null, error: "parse error" };
    }
  }

  let errorCount = 0;

  // -- Init --
  const initStart = Date.now();
  const initSeq = ++seq;
  const initResp = await sendAndWait({ cmd: "init", seq: initSeq, db_path: DB_PATH, data_dir: DATA_DIR });
  const initTimeMs = Date.now() - initStart;

  if (!initResp.ok) {
    console.error(`[${name}] Init failed:`, initResp.error);
    errorCount++;
  }
  console.log(`[${name}] Init done in ${initTimeMs}ms`);

  // -- Get stats --
  const statsSeq = ++seq;
  const statsResp = await sendAndWait({ cmd: "get_stats", seq: statsSeq });
  const stats = statsResp.data as { total_patterns: number };
  const totalPatterns = stats?.total_patterns ?? 0;
  console.log(`[${name}] Patterns loaded: ${totalPatterns}`);

  // -- Query loop --
  const queryLatencies: number[] = [];
  let peakRssKb = 0;

  for (let i = 0; i < QUERY_COUNT; i++) {
    const embedding = makeQueryEmbedding(i);
    const qSeq = ++seq;

    const rss = readVmRSS(pid);
    if (rss > peakRssKb) peakRssKb = rss;

    const qStart = Date.now();
    const qResp = await sendAndWait({ cmd: "query", seq: qSeq, embedding, top_k: 10 });
    const qTime = Date.now() - qStart;

    queryLatencies.push(qTime);

    if (!qResp.ok) {
      errorCount++;
    }

    if ((i + 1) % 25 === 0) {
      process.stdout.write(`[${name}] Query ${i + 1}/${QUERY_COUNT} -- last latency: ${qTime}ms\n`);
    }
  }

  // Final RSS check
  const finalRss = readVmRSS(pid);
  if (finalRss > peakRssKb) peakRssKb = finalRss;

  // -- Shutdown --
  const shutSeq = ++seq;
  sendCmd({ cmd: "shutdown", seq: shutSeq });

  // Wait for process to exit
  await new Promise<void>((resolve) => {
    proc.on("close", () => resolve());
    setTimeout(() => {
      proc.kill();
      resolve();
    }, 5000);
  });

  // Compute percentiles
  const sorted = [...queryLatencies].sort((a, b) => a - b);
  const avg = queryLatencies.reduce((a, b) => a + b, 0) / queryLatencies.length;
  const p50 = sorted[Math.floor(sorted.length * 0.50)];
  const p95 = sorted[Math.floor(sorted.length * 0.95)];
  const p99 = sorted[Math.floor(sorted.length * 0.99)];

  return {
    backend: name,
    initTimeMs,
    totalPatterns,
    queryLatencies,
    avgLatencyMs: Math.round(avg * 10) / 10,
    p50Ms: p50,
    p95Ms: p95,
    p99Ms: p99,
    peakRssKb,
    errors: errorCount,
  };
}

function printTable(results: BenchResult[]): void {
  console.log("\n");
  console.log("=".repeat(72));
  console.log("  ENGRAM BRAIN BACKEND BENCHMARK RESULTS");
  console.log("  Database: brain.db -- 1630 memories, 6632 edges");
  console.log(`  Queries per backend: ${QUERY_COUNT}`);
  console.log("=".repeat(72));
  console.log(
    `${"Metric".padEnd(28)} ${"Rust".padStart(18)} ${"C++".padStart(18)}`
  );
  console.log("-".repeat(66));

  const r = results.find((x) => x.backend === "Rust");
  const c = results.find((x) => x.backend === "C++");

  if (!r || !c) {
    console.log("One or both backends failed. Partial results:");
    for (const res of results) {
      console.log(`  ${res.backend}: init=${res.initTimeMs}ms, avg=${res.avgLatencyMs}ms, errors=${res.errors}`);
    }
    return;
  }

  function row(label: string, rv: string | number, cv: string | number): void {
    console.log(
      `${label.padEnd(28)} ${String(rv).padStart(18)} ${String(cv).padStart(18)}`
    );
  }

  row("Patterns loaded", r.totalPatterns, c.totalPatterns);
  row("Init time (ms)", r.initTimeMs, c.initTimeMs);
  row("Query avg (ms)", r.avgLatencyMs, c.avgLatencyMs);
  row("Query p50 (ms)", r.p50Ms, c.p50Ms);
  row("Query p95 (ms)", r.p95Ms, c.p95Ms);
  row("Query p99 (ms)", r.p99Ms, c.p99Ms);
  row("Peak RSS (MB)", (r.peakRssKb / 1024).toFixed(1), (c.peakRssKb / 1024).toFixed(1));
  row("Errors", r.errors, c.errors);

  console.log("-".repeat(66));
  console.log("\nWinner summary:");

  const initWinner = r.initTimeMs <= c.initTimeMs ? "Rust" : "C++";
  const queryWinner = r.avgLatencyMs <= c.avgLatencyMs ? "Rust" : "C++";
  const memWinner = r.peakRssKb <= c.peakRssKb ? "Rust" : "C++";

  console.log(`  Init speed:   ${initWinner} is faster by ${Math.round(Math.abs(r.initTimeMs - c.initTimeMs))}ms`);
  console.log(`  Query speed:  ${queryWinner} is faster by ${Math.abs(r.avgLatencyMs - c.avgLatencyMs).toFixed(1)}ms avg`);
  console.log(`  Memory usage: ${memWinner} uses ${((Math.abs(r.peakRssKb - c.peakRssKb)) / 1024).toFixed(1)}MB less`);
  console.log("=".repeat(72));
}

async function main(): Promise<void> {
  console.log("Engram Brain Backend Benchmark");
  console.log(`Database: ${DB_PATH}`);
  console.log(`Query count: ${QUERY_COUNT}`);

  const results: BenchResult[] = [];

  for (const backend of BACKENDS) {
    try {
      const result = await runBackend(backend.name, backend.bin);
      results.push(result);
    } catch (err) {
      console.error(`Backend ${backend.name} failed:`, err);
    }
  }

  printTable(results);
}

main().catch(console.error);
