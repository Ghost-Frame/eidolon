// ============================================================================
// Brain routes
// Prefix: /brain/*
// ============================================================================

import { log } from "../../config/logger.ts";
import { json, errorResponse } from "../../helpers/index.ts";
import { isBrainReady, queryBrain, absorbMemory, brainStats, brainDecayTick } from "./manager.ts";

const BRAIN_BACKEND: string = process.env.ENGRAM_BRAIN_BACKEND || "rust";

export async function handleBrainRoutes(
  method: string,
  url: URL,
  req: Request,
  requestId: string,
): Promise<Response | null> {
  const path = url.pathname;

  if (!path.startsWith("/brain/") && path !== "/brain") return null;

  const sub = path.slice("/brain".length);

  // GET /brain/health
  if (sub === "/health" && method === "GET") {
    return json({
      ok: true,
      ready: isBrainReady(),
      backend: BRAIN_BACKEND,
    });
  }

  // GET /brain/stats
  if (sub === "/stats" && method === "GET") {
    if (!isBrainReady()) {
      return errorResponse("brain_not_ready", 503, requestId);
    }
    try {
      const stats = await brainStats();
      return json({ ok: true, stats });
    } catch (e: any) {
      log.error({ msg: "brain_stats_error", error: e.message });
      return errorResponse(e.message, 503, requestId);
    }
  }

  // POST /brain/query
  if (sub === "/query" && method === "POST") {
    if (!isBrainReady()) {
      return errorResponse("brain_not_ready", 503, requestId);
    }
    const body = await req.json().catch(() => ({})) as Record<string, unknown>;
    const { query, top_k, beta, spread_hops } = body;
    if (!query || typeof query !== "string") {
      return errorResponse("query string required", 400, requestId);
    }
    try {
      const result = await queryBrain(query, {
        top_k: typeof top_k === "number" ? top_k : undefined,
        beta: typeof beta === "number" ? beta : undefined,
        spread_hops: typeof spread_hops === "number" ? spread_hops : undefined,
      });
      return json({ ok: true, ...result });
    } catch (e: any) {
      log.error({ msg: "brain_query_error", error: e.message });
      return errorResponse(e.message, 503, requestId);
    }
  }

  // POST /brain/absorb
  if (sub === "/absorb" && method === "POST") {
    const body = await req.json().catch(() => ({})) as Record<string, unknown>;
    const { id, content, category, source, importance, created_at, tags } = body;

    if (!id || !content || typeof content !== "string") {
      return errorResponse("id and content required", 400, requestId);
    }

    try {
      await absorbMemory({
        id: Number(id),
        content: String(content),
        category: typeof category === "string" ? category : "general",
        source: typeof source === "string" ? source : "unknown",
        importance: typeof importance === "number" ? importance : 5,
        created_at: typeof created_at === "string" ? created_at : new Date().toISOString(),
        tags: Array.isArray(tags) ? tags.map(String) : undefined,
      });
      return json({ ok: true });
    } catch (e: any) {
      log.error({ msg: "brain_absorb_error", error: e.message });
      return errorResponse(e.message, 500, requestId);
    }
  }

  // POST /brain/decay
  if (sub === "/decay" && method === "POST") {
    const body = await req.json().catch(() => ({})) as Record<string, unknown>;
    const ticks = typeof body.ticks === "number" ? body.ticks : 1;
    try {
      await brainDecayTick(ticks);
      return json({ ok: true, ticks });
    } catch (e: any) {
      log.error({ msg: "brain_decay_error", error: e.message });
      return errorResponse(e.message, 500, requestId);
    }
  }

  return null;
}
