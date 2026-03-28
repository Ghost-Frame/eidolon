// ============================================================================
// Brain process manager
// Spawns the native brain subprocess (Rust or C++), manages JSON-over-stdio
// IPC, and handles crash recovery with auto-restart.
// ============================================================================

import { spawn, type ChildProcess } from "child_process";
import { createInterface } from "readline";
import { resolve } from "path";
import { existsSync } from "fs";
import { log } from "../../config/logger.ts";
import { embed } from "../../embeddings/index.ts";
import { DATA_DIR } from "../../config/index.ts";
import type { BrainCommand, BrainResponse, BrainQueryResult, BrainStats } from "./types.ts";

// ---- Config ----

const BRAIN_BACKEND: string = process.env.ENGRAM_BRAIN_BACKEND || "rust";
const EXE_SUFFIX = process.platform === "win32" ? ".exe" : "";

const RUST_BINARY = resolve(process.cwd(), "eidolon", "rust", "target", "release", `eidolon${EXE_SUFFIX}`);
const CPP_BINARY = resolve(process.cwd(), "eidolon", "cpp", "build", `eidolon${EXE_SUFFIX}`);

const REQUEST_TIMEOUT_MS = 30000;
const MAX_RESTART_ATTEMPTS = 3;
const RESTART_BACKOFF_MS = 2000;

// ---- State ----

let child: ChildProcess | null = null;
let ready = false;
let seq = 0;
let restartAttempts = 0;
let restartTimer: ReturnType<typeof setTimeout> | null = null;

interface PendingRequest {
  resolve: (value: BrainResponse) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const pending = new Map<number, PendingRequest>();

// ---- Helpers ----

function binaryPath(): string {
  return BRAIN_BACKEND === "cpp" ? CPP_BINARY : RUST_BINARY;
}

function rejectAllPending(reason: string): void {
  for (const [s, p] of pending) {
    clearTimeout(p.timer);
    p.reject(new Error(reason));
    pending.delete(s);
  }
}

// ---- IPC: send a command, return a Promise for the response ----

function sendCommand(cmd: BrainCommand): Promise<BrainResponse> {
  return new Promise((resolve, reject) => {
    if (!child || !ready) {
      reject(new Error("brain_not_ready"));
      return;
    }

    const thisSeq = ++seq;
    const payload = JSON.stringify({ ...cmd, seq: thisSeq }) + "\n";

    const timer = setTimeout(() => {
      pending.delete(thisSeq);
      reject(new Error(`brain_timeout seq=${thisSeq} cmd=${cmd.cmd}`));
    }, REQUEST_TIMEOUT_MS);

    pending.set(thisSeq, { resolve, reject, timer });

    try {
      child.stdin!.write(payload);
    } catch (e: any) {
      clearTimeout(timer);
      pending.delete(thisSeq);
      reject(new Error(`brain_write_failed: ${e.message}`));
    }
  });
}

// ---- Spawn the subprocess ----

function spawnBrain(): void {
  const bin = binaryPath();

  if (!existsSync(bin)) {
    log.warn({ msg: "brain_binary_not_found", path: bin, backend: BRAIN_BACKEND });
    return;
  }

  log.info({ msg: "brain_spawn", binary: bin, backend: BRAIN_BACKEND });

  child = spawn(bin, [], {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env },
  });

  // Route stderr to Engram log with [brain] prefix
  const stderrRl = createInterface({ input: child.stderr! });
  stderrRl.on("line", line => {
    log.info({ msg: "[brain] " + line });
  });

  // Parse stdout as JSON lines and resolve pending requests
  const stdoutRl = createInterface({ input: child.stdout! });
  stdoutRl.on("line", (line: string) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    try {
      const resp: BrainResponse = JSON.parse(trimmed);
      const s = resp.seq;
      if (s != null && pending.has(s)) {
        const p = pending.get(s)!;
        clearTimeout(p.timer);
        pending.delete(s);
        p.resolve(resp);
      }
    } catch (e: any) {
      log.warn({ msg: "brain_parse_error", line: trimmed.substring(0, 200), error: e.message });
    }
  });

  child.on("exit", (code, signal) => {
    log.warn({ msg: "brain_exited", code, signal, attempts: restartAttempts });
    ready = false;
    rejectAllPending("brain_subprocess_exited");
    child = null;

    // Auto-restart with backoff
    if (restartAttempts < MAX_RESTART_ATTEMPTS) {
      restartAttempts++;
      const backoff = RESTART_BACKOFF_MS * restartAttempts;
      log.info({ msg: "brain_restart_scheduled", attempt: restartAttempts, backoff_ms: backoff });
      restartTimer = setTimeout(async () => {
        spawnBrain();
        await initBrain();
      }, backoff);
    } else {
      log.warn({ msg: "brain_restart_exhausted", max_attempts: MAX_RESTART_ATTEMPTS });
    }
  });

  child.on("error", (err) => {
    log.error({ msg: "brain_spawn_error", error: err.message });
  });
}

// ---- Send init command after spawn ----

async function initBrain(): Promise<boolean> {
  if (!child) return false;

  // Give process a moment to start up
  await new Promise(r => setTimeout(r, 200));

  if (!child) return false;

  // Mark ready temporarily to allow sendCommand to work
  ready = true;

  try {
    const brainDbPath = resolve(DATA_DIR, "brain.db");
    const resp = await sendCommand({
      cmd: "init",
      db_path: brainDbPath,
      data_dir: DATA_DIR,
    });

    if (!resp.ok) {
      log.warn({ msg: "brain_init_failed", error: resp.error });
      ready = false;
      return false;
    }

    restartAttempts = 0; // Successful init resets the counter
    log.info({ msg: "brain_ready", backend: BRAIN_BACKEND });
    return true;
  } catch (e: any) {
    log.warn({ msg: "brain_init_error", error: e.message });
    ready = false;
    return false;
  }
}

// ---- Public API ----

export async function startBrain(): Promise<boolean> {
  const bin = binaryPath();
  if (!existsSync(bin)) {
    log.info({ msg: "brain_start_skipped", reason: "binary_not_found", path: bin });
    return false;
  }

  spawnBrain();
  return initBrain();
}

export async function stopBrain(): Promise<void> {
  if (!child) return;

  // Clear any pending restart timer
  if (restartTimer) {
    clearTimeout(restartTimer);
    restartTimer = null;
  }

  // Clear the exit handler by setting attempts to max so it won't restart
  restartAttempts = MAX_RESTART_ATTEMPTS;

  try {
    if (ready) {
      await Promise.race([
        sendCommand({ cmd: "shutdown" }),
        new Promise(r => setTimeout(r, 2000)),
      ]);
    }
  } catch {
    // Ignore shutdown errors
  }

  ready = false;

  if (child) {
    // Wait 2s for graceful exit, then SIGKILL
    await new Promise<void>(resolve => {
      const killTimer = setTimeout(() => {
        try { child?.kill("SIGKILL"); } catch {}
        resolve();
      }, 2000);

      child!.once("exit", () => {
        clearTimeout(killTimer);
        resolve();
      });
    });
  }

  rejectAllPending("brain_shutdown");
  child = null;
  log.info({ msg: "brain_stopped" });
}

export function isBrainReady(): boolean {
  return ready && child !== null;
}

export async function queryBrain(
  text: string,
  options?: { top_k?: number; beta?: number; spread_hops?: number },
): Promise<BrainQueryResult> {
  const embedding = await embed(text);
  const resp = await sendCommand({
    cmd: "query",
    embedding: Array.from(embedding),
    top_k: options?.top_k,
    beta: options?.beta,
    spread_hops: options?.spread_hops,
  });

  if (!resp.ok) throw new Error(resp.error || "brain_query_failed");
  return resp.data as BrainQueryResult;
}

export async function absorbMemory(memory: {
  id: number;
  content: string;
  category: string;
  source: string;
  importance: number;
  created_at: string;
  tags?: string[];
}): Promise<void> {
  if (!isBrainReady()) return;

  try {
    const embedding = await embed(memory.content);
    await sendCommand({
      cmd: "absorb",
      id: memory.id,
      content: memory.content,
      category: memory.category,
      source: memory.source,
      importance: memory.importance,
      created_at: memory.created_at,
      embedding: Array.from(embedding),
      tags: memory.tags,
    });
  } catch (e: any) {
    log.warn({ msg: "brain_absorb_failed", id: memory.id, error: e.message });
  }
}

export async function brainDecayTick(ticks = 1): Promise<void> {
  if (!isBrainReady()) return;

  try {
    await sendCommand({ cmd: "decay_tick", ticks });
  } catch (e: any) {
    log.warn({ msg: "brain_decay_tick_failed", error: e.message });
  }
}

export async function brainStats(): Promise<BrainStats> {
  const resp = await sendCommand({ cmd: "get_stats" });
  if (!resp.ok) throw new Error(resp.error || "brain_stats_failed");
  return resp.data as BrainStats;
}
