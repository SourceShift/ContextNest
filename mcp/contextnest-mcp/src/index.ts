#!/usr/bin/env tsx
/**
 * ContextNest MCP server — stdio transport.
 *
 * A thin, typed wrapper over the ContextNest HTTP API. It exists because the
 * equivalent `curl` recipes (documented in the `contextnest-memory` skill) are
 * easy to get wrong in three specific ways, each of which has already produced
 * a false "ContextNest is down" report in the wild:
 *
 *   1. **No timeout on an unscoped retrieve.** `POST /api/v1/tools/retrieve`
 *      without `session_id` scans the whole substrate: measured 9.3 s warm and
 *      89 s cold against 534k fragments / 12.4M edges. A default or short curl
 *      timeout reports `000`, and the caller concludes the service is dead.
 *      Here the timeout is per-call-shape: 120 s unscoped, 30 s scoped.
 *   2. **Port guessing.** The substrate answers on 127.0.0.1:28080 and nowhere
 *      else. 7048 is the z-dashboard daemon, 7130/17130 are InsForge backends,
 *      5057 is the dashboard UI. Every transport error below says so.
 *   3. **Silent fallback.** A client that "falls back to a local memory dir"
 *      hides the fact that memory was never queried. Every failure here names
 *      the URL it tried, the wall time, and what came back.
 *
 * Config (all optional):
 *   CONTEXTNEST_BASE_URL              default http://127.0.0.1:28080
 *   CONTEXTNEST_MCP_SCAN_TIMEOUT_MS   default 120000  (unscoped retrieve)
 *   CONTEXTNEST_MCP_TIMEOUT_MS        default 30000   (everything else)
 *   CONTEXTNEST_MCP_CONTENT_CHARS     default 320     (per-fragment clip)
 *   CONTEXTNEST_MCP_MAX_ROWS          default 25      (rows rendered per list)
 */
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

const DEFAULT_BASE = "http://127.0.0.1:28080";
const BASE = (
  process.env.CONTEXTNEST_BASE_URL ??
  process.env.CONTEXTNEST_BASE ??
  DEFAULT_BASE
).replace(/\/+$/, "");

const SCAN_TIMEOUT_MS = intEnv("CONTEXTNEST_MCP_SCAN_TIMEOUT_MS", 120_000);
const FAST_TIMEOUT_MS = intEnv("CONTEXTNEST_MCP_TIMEOUT_MS", 30_000);
const CONTENT_CHARS = intEnv("CONTEXTNEST_MCP_CONTENT_CHARS", 320);
const MAX_ROWS = intEnv("CONTEXTNEST_MCP_MAX_ROWS", 25);

function intEnv(name: string, fallback: number): number {
  const raw = process.env[name];
  if (!raw) return fallback;
  const n = Number.parseInt(raw, 10);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

// ─── transport ──────────────────────────────────────────────────────────────

type Ok<T> = { ok: true; data: T; ms: number };
type Err = { ok: false; message: string };

type Opts = {
  method?: "GET" | "POST";
  body?: unknown;
  timeoutMs?: number;
};

/** Ports that are NOT ContextNest — included in every transport failure so a
 *  caller never repeats the wrong-port diagnosis. */
const PORT_HINT =
  `ContextNest serves ${BASE} only (API is 28080).\n` +
  `  7048 = z-dashboard daemon · 7130/17130 = InsForge · 5057 = dashboard UI. None are the memory API.`;

async function request<T>(path: string, opts: Opts = {}): Promise<Ok<T> | Err> {
  const method = opts.method ?? "GET";
  const url = `${BASE}${path}`;
  const timeoutMs = opts.timeoutMs ?? FAST_TIMEOUT_MS;
  const started = Date.now();
  try {
    const res = await fetch(url, {
      method,
      headers: opts.body === undefined ? undefined : { "content-type": "application/json" },
      body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
      signal: AbortSignal.timeout(timeoutMs),
    });
    const ms = Date.now() - started;
    const raw = await res.text();
    if (!res.ok) {
      return {
        ok: false,
        message: `${method} ${url} → HTTP ${res.status} after ${ms} ms.\n  ${clip(raw, 300)}\n${PORT_HINT}`,
      };
    }
    try {
      return { ok: true, data: JSON.parse(raw) as T, ms };
    } catch {
      // /prompt-context/capsule returns text/markdown, not JSON.
      return { ok: true, data: raw as unknown as T, ms };
    }
  } catch (e) {
    const ms = Date.now() - started;
    const err = e as { name?: string; message?: string };
    const why =
      err?.name === "TimeoutError"
        ? `timed out after ${timeoutMs} ms (raise CONTEXTNEST_MCP_SCAN_TIMEOUT_MS)`
        : (err?.message ?? String(e));
    return {
      ok: false,
      message:
        `No answer from ${method} ${url} after ${ms} ms — ${why}.\n${PORT_HINT}\n` +
        `  Check the substrate:  curl -s --max-time 5 ${BASE}/api/v1/substrate/health`,
    };
  }
}

// ─── rendering helpers ──────────────────────────────────────────────────────

function text(s: string) {
  return { content: [{ type: "text" as const, text: s }] };
}
function fail(s: string) {
  return { content: [{ type: "text" as const, text: s }], isError: true };
}
function clip(s: unknown, n = CONTENT_CHARS): string {
  const t = typeof s === "string" ? s : JSON.stringify(s);
  if (t === undefined) return "";
  const flat = t.replace(/\s+/g, " ").trim();
  return flat.length <= n ? flat : `${flat.slice(0, n)}… (+${flat.length - n} chars)`;
}
function num(v: unknown, digits = 2): string {
  return typeof v === "number" ? v.toFixed(digits) : String(v);
}

/** Unwrap a JSON string field that the substrate sometimes stores as a
 *  serialized object (e.g. `metadata` and `summary` come back as strings). */
function asObject(v: unknown): Record<string, unknown> | undefined {
  if (v && typeof v === "object" && !Array.isArray(v)) return v as Record<string, unknown>;
  if (typeof v === "string" && v.trim().startsWith("{")) {
    try {
      const parsed = JSON.parse(v.replace(/'/g, '"'));
      if (parsed && typeof parsed === "object") return parsed as Record<string, unknown>;
    } catch {
      return undefined;
    }
  }
  return undefined;
}

const ARRAY_KEYS = [
  "hits", "items", "matches", "sessions", "features", "records", "phases",
  "atoms", "results", "clusters", "entries", "data",
];

/**
 * Generic fallback renderer: find the first array-valued property, print up to
 * MAX_ROWS rows as `key: value` lines. Used for endpoints whose shape is not
 * pinned by the client (attention, by-feature, by-intent) so an upstream
 * response-shape change degrades to "ugly but readable" instead of "empty".
 */
function renderGeneric(label: string, data: unknown): string {
  if (typeof data === "string") return data;
  const obj = data as Record<string, unknown>;
  const key =
    ARRAY_KEYS.find((k) => Array.isArray(obj?.[k])) ??
    Object.keys(obj ?? {}).find((k) => Array.isArray(obj[k]));
  if (!key) return `${label}\n${JSON.stringify(data, null, 2).slice(0, 4000)}`;
  const rows = obj[key] as unknown[];
  const head = Object.entries(obj)
    .filter(([k, v]) => k !== key && typeof v !== "object")
    .map(([k, v]) => `${k}=${v}`)
    .join(" · ");
  const body = rows.slice(0, MAX_ROWS).map((row, i) => {
    if (row && typeof row === "object") {
      const r = row as Record<string, unknown>;
      return `  ${i + 1}. ` + Object.entries(r)
        .filter(([, v]) => v !== null && v !== undefined && v !== "" && !(Array.isArray(v) && !v.length))
        .slice(0, 6)
        .map(([k, v]) => `${k}: ${clip(v, 90)}`)
        .join(" · ");
    }
    return `  ${i + 1}. ${clip(row, 120)}`;
  });
  const more = rows.length > MAX_ROWS ? `\n  … ${rows.length - MAX_ROWS} more` : "";
  return `${label}${head ? ` (${head})` : ""}\n${body.join("\n")}${more}`;
}

// ─── server ─────────────────────────────────────────────────────────────────

const server = new McpServer({ name: "contextnest", version: "0.1.0" });

server.registerTool(
  "cn_health",
  {
    title: "ContextNest substrate health",
    description:
      "Is the ContextNest memory substrate reachable, and how populated is it? " +
      "Call this FIRST whenever another cn_* tool errors — it separates " +
      "'substrate is down' from 'that particular query timed out'. Also the " +
      "fastest way to confirm the base URL is right.",
    inputSchema: {},
  },
  async () => {
    const r = await request<{
      fragments: { total: number; consolidated: number; lag: number };
      basins: { count: number; avg_mass: number; max_mass: number };
      connections: { edges: number; avg_degree: number; nodes: number };
      decay: { half_life_days: number; median_fragment_age_days: number };
    }>("/api/v1/substrate/health", { timeoutMs: 5_000 });
    if (!r.ok) return fail(r.message);
    const h = r.data;
    const lines = [
      `ContextNest is UP at ${BASE} (${r.ms} ms)`,
      `  fragments    ${h.fragments.total} (consolidated ${h.fragments.consolidated} · lag ${h.fragments.lag})`,
      `  basins       ${h.basins.count} · avg mass ${num(h.basins.avg_mass)} · max ${h.basins.max_mass}`,
      `  connections  ${h.connections.edges} edges · avg degree ${num(h.connections.avg_degree, 1)} · ${h.connections.nodes} nodes`,
      `  decay        half-life ${h.decay.half_life_days}d · median fragment age ${num(h.decay.median_fragment_age_days, 1)}d`,
    ];
    if (h.basins.count === 0 && h.fragments.total > 0) {
      lines.push(
        "  WARNING: basins.count == 0 on a populated substrate — the consolidation",
        "  worker or the embedder is down; basin/connection expansion will be empty.",
      );
    }
    if (h.fragments.lag > 0) {
      lines.push(`  note: ${h.fragments.lag} fragments not yet consolidated (sidecar-only until then).`);
    }
    return text(lines.join("\n"));
  },
);

server.registerTool(
  "cn_retrieve",
  {
    title: "Semantic recall of memory atoms",
    description:
      "Cosine-similarity search over stored memory fragments — decisions, failures, " +
      "verifications, features, files touched from past sessions. Use for " +
      "'what did we decide about X', 'what broke last time', 'anything similar to this'. " +
      "PASS session_id OR session_ids WHENEVER KNOWN: scoped is ~1.4 ms, unscoped is a " +
      "9-89 s full-substrate scan. Returns hits with ids usable in cn_outcome.",
    inputSchema: {
      query: z.string().min(1).describe("Natural-language question or topic."),
      top_k: z.number().int().min(1).max(50).default(5).describe("How many atoms to return."),
      session_id: z.string().optional().describe("Restrict to one session (fast path)."),
      session_ids: z.array(z.string()).optional().describe("Restrict to several sessions; each hit is tagged with its session."),
      metadata: z.record(z.string(), z.string()).optional().describe("Exact-match filter over fragment metadata keys."),
    },
  },
  async (args) => {
    const scoped = Boolean(args.session_id) || Boolean(args.session_ids?.length);
    const r = await request<{
      hits: Array<{
        id: string;
        content: string;
        similarity: number;
        importance?: number;
        session_id?: string;
        metadata?: unknown;
      }>;
    }>("/api/v1/tools/retrieve", {
      method: "POST",
      body: {
        query: args.query,
        top_k: args.top_k ?? 5,
        session_id: args.session_id,
        session_ids: args.session_ids,
        metadata: args.metadata,
      },
      timeoutMs: scoped ? FAST_TIMEOUT_MS : SCAN_TIMEOUT_MS,
    });
    if (!r.ok) return fail(r.message);
    const hits = r.data.hits ?? [];
    if (!hits.length) {
      return text(
        `No matches for "${args.query}"${scoped ? " in the scoped sessions" : ""} (${r.ms} ms).\n` +
          `  Try cn_prompt_context_capsule for a topic briefing, or cn_session_find for "which session".`,
      );
    }
    const rows = hits.map((h, i) => {
      const meta = asObject(h.metadata);
      const sid = h.session_id ?? (meta?.src_session as string | undefined);
      return [
        `${i + 1}. [${num(h.similarity, 3)}] ${clip(h.content)}`,
        `   id=${h.id}${sid ? ` session=${sid}` : ""}${h.importance !== undefined ? ` importance=${num(h.importance)}` : ""}`,
      ].join("\n");
    });
    const note = scoped
      ? ""
      : `\n  note: unscoped scan took ${r.ms} ms. Pass session_id/session_ids when you know the session (~1.4 ms).\n` +
        `  note: a hit at similarity ~0 has just been ingested and is not consolidated yet — that is not a miss.`;
    return text(`${hits.length} hit(s) for "${args.query}" (${r.ms} ms)\n\n${rows.join("\n")}${note}`);
  },
);

server.registerTool(
  "cn_prompt_context_capsule",
  {
    title: "Prompt-context capsule (topic briefing)",
    description:
      "Deterministic markdown briefing that clusters past-session atoms by kind and orders " +
      "them Risks → Decisions → Failures → Prompt directives → Verifications → Evidence → Files → Assumptions. " +
      "Use BEFORE planning non-trivial work in an area. IMPORTANT: `query` is a literal " +
      "substring filter over stored prose, NOT semantic search — file paths, route names and " +
      "rare terms return nothing. Use cn_retrieve for meaning-based or identifier recall.",
    inputSchema: {
      query: z.string().optional().describe("Literal lowercase substring filter over atom text."),
      project: z.string().optional().describe("Match project_cwd substring."),
      session_id: z.string().optional(),
      since: z.string().optional().describe("7d | 30d | 90d"),
      min_count: z.number().int().min(1).optional().describe("Atoms seen at least N times (default 2)."),
      kind: z.string().optional().describe("Restrict to one kind (decision_made, failure, risk_flag, …)."),
      max_per_kind: z.number().int().min(1).max(25).optional(),
    },
  },
  async (args) => {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(args)) {
      if (v !== undefined && v !== "") qs.set(k, String(v));
    }
    const path = `/api/v1/prompt-context/capsule${qs.size ? `?${qs}` : ""}`;
    const r = await request<string>(path, { timeoutMs: FAST_TIMEOUT_MS });
    if (!r.ok) return fail(r.message);
    const md = typeof r.data === "string" ? r.data : JSON.stringify(r.data);
    if (!md.trim()) {
      return text(
        `Capsule is empty for ${path} (${r.ms} ms).\n` +
          `  Most likely cause: the filter is a literal substring over prose. Try a broader word, ` +
          `drop min_count, or use cn_retrieve (semantic) instead.`,
      );
    }
    return text(`${md}\n\n— capsule ${path} (${r.ms} ms)`);
  },
);

server.registerTool(
  "cn_session_find",
  {
    title: "Find past sessions by free text",
    description:
      "Search session summaries/intents/goals for a topic and return the sessions that match, " +
      "ranked. Use when you need 'which session worked on X' and no feature name or file path " +
      "is known. (cn_session_by_feature / cn_session_by_file are more precise when you have one.)",
    inputSchema: {
      query: z.string().min(1),
      project: z.string().optional(),
      since: z.string().optional().describe("7d | 30d | 90d"),
      limit: z.number().int().min(1).max(50).default(10),
    },
  },
  async (args) => {
    const r = await request<{
      count: number;
      total_scored?: number;
      truncated?: boolean;
      sessions: Array<{
        session_id: string;
        project_cwd?: string;
        last_ts?: string;
        score?: number;
        match_kind?: string;
        match_text?: string;
      }>;
    }>("/api/v1/sessions/find", {
      method: "POST",
      body: { query: args.query, project: args.project, since: args.since, limit: args.limit ?? 10 },
    });
    if (!r.ok) return fail(r.message);
    const rows = (r.data.sessions ?? []).slice(0, MAX_ROWS).map((s, i) =>
      [
        `${i + 1}. ${s.session_id}${s.score !== undefined ? ` [${num(s.score, 3)}]` : ""} ${s.last_ts ?? ""}`,
        `   ${s.project_cwd ?? ""}`,
        s.match_text ? `   ${s.match_kind ?? "match"}: ${clip(s.match_text, 160)}` : "",
      ].filter(Boolean).join("\n"),
    );
    if (!rows.length) return text(`No session matched "${args.query}" (${r.ms} ms).`);
    const trunc = r.data.truncated ? ` · truncated from ${r.data.total_scored}` : "";
    return text(
      `${r.data.count} session(s) for "${args.query}" (${r.ms} ms${trunc})\n\n${rows.join("\n")}\n\n` +
        `  Drill in: cn_session_summary / cn_session_trajectory with one of the ids above.`,
    );
  },
);

server.registerTool(
  "cn_session_by_file",
  {
    title: "Sessions that touched a file",
    description:
      "Given a file path (absolute or a distinctive suffix like src/api/field.rs), return the " +
      "sessions that touched it. The most precise forensics tool when you have a path.",
    inputSchema: { path: z.string().min(1).describe("File path or distinctive suffix.") },
  },
  async (args) => {
    const r = await request<{
      query: string;
      matches: Array<{ session_id: string; matched_files?: string[]; total_files?: number }>;
    }>(`/api/v1/sessions/by-file?path=${encodeURIComponent(args.path)}`);
    if (!r.ok) return fail(r.message);
    const m = r.data.matches ?? [];
    if (!m.length) return text(`No session touched "${args.path}" (${r.ms} ms).`);
    return text(
      `${m.length} session(s) touched "${args.path}" (${r.ms} ms)\n\n` +
        m.slice(0, MAX_ROWS).map((x, i) =>
          `  ${i + 1}. ${x.session_id} (${x.total_files ?? "?"} file(s); ${(x.matched_files ?? []).length} matched)`,
        ).join("\n"),
    );
  },
);

server.registerTool(
  "cn_session_by_feature",
  {
    title: "Sessions that shipped a feature",
    description:
      "Given a feature/deliverable name (route, endpoint, capability), return the sessions that " +
      "built or deployed it. In testing this beat both the capsule and free-text session search " +
      "for 'which session built X'. HTTP-only upstream — no other client exposes it.",
    inputSchema: { q: z.string().min(1).describe("Feature, route, or deliverable name.") },
  },
  async (args) => {
    const r = await request<unknown>(`/api/v1/sessions/by-feature?q=${encodeURIComponent(args.q)}`);
    if (!r.ok) return fail(r.message);
    const rendered = renderGeneric(`sessions by-feature "${args.q}" (${r.ms} ms)`, r.data);
    if (/^sessions by-feature[^\n]*\n\s*$/.test(rendered)) {
      return text(`No session matched feature "${args.q}" (${r.ms} ms). Try cn_session_find.`);
    }
    return text(rendered);
  },
);

server.registerTool(
  "cn_session_by_intent",
  {
    title: "Sessions by user intent",
    description: "Return sessions grouped/ranked by the user intent they served (e.g. 'fundraising outreach').",
    inputSchema: { q: z.string().min(1) },
  },
  async (args) => {
    const r = await request<unknown>(`/api/v1/sessions/by-intent?q=${encodeURIComponent(args.q)}`);
    if (!r.ok) return fail(r.message);
    return text(renderGeneric(`sessions by-intent "${args.q}" (${r.ms} ms)`, r.data));
  },
);

server.registerTool(
  "cn_session_summary",
  {
    title: "One session's summary",
    description:
      "Hydrate a session id you got from cn_session_find / by-file / by-feature into its summary: " +
      "domain, progress, goal, topics, and the counts that back them.",
    inputSchema: { id: z.string().min(1).describe("Session id (uuid).") },
  },
  async (args) => {
    const r = await request<{ session_id: string; fragment_count?: number; summary?: unknown }>(
      `/api/v1/sessions/${encodeURIComponent(args.id)}/summary`,
    );
    if (!r.ok) return fail(r.message);
    const s = asObject(r.data.summary);
    const body = s
      ? Object.entries(s).map(([k, v]) => `  ${k}: ${clip(v, 220)}`).join("\n")
      : `  ${clip(r.data.summary, 1500)}`;
    return text(
      `session ${r.data.session_id} · ${r.data.fragment_count ?? "?"} fragments (${r.ms} ms)\n${body}`,
    );
  },
);

server.registerTool(
  "cn_session_trajectory",
  {
    title: "One session's trajectory",
    description:
      "Ordered atom timeline for a session: phases (goal + counts per phase) then the atoms in " +
      "sequence. Use when the summary is too coarse — e.g. a session whose goal drifted, or to see " +
      "the order in which decisions/failures actually happened.",
    inputSchema: {
      id: z.string().min(1),
      max_records: z.number().int().min(1).max(200).default(MAX_ROWS).describe("How many atoms to list."),
    },
  },
  async (args) => {
    const r = await request<{
      session_id: string;
      trajectory_count?: number;
      phases?: Array<{ idx: number; goal?: string; counts?: unknown }>;
      records?: Array<{ id: string; kind: string; content: string; ts?: string; phase_idx?: number }>;
    }>(`/api/v1/sessions/${encodeURIComponent(args.id)}/trajectory`);
    if (!r.ok) return fail(r.message);
    const phases = (r.data.phases ?? []).map((p) =>
      `  phase ${p.idx}: ${clip(p.goal, 120)}\n            counts: ${clip(p.counts, 160)}`,
    );
    const records = (r.data.records ?? []).slice(0, args.max_records ?? MAX_ROWS).map((x, i) =>
      `  ${i + 1}. [${x.kind}] ${clip(x.content, 180)}${x.ts ? `  (${x.ts})` : ""}`,
    );
    const more = (r.data.records?.length ?? 0) - records.length;
    return text(
      `session ${r.data.session_id} · ${r.data.trajectory_count ?? r.data.records?.length ?? 0} atoms (${r.ms} ms)\n` +
        (phases.length ? `phases:\n${phases.join("\n")}\n` : "") +
        `timeline:\n${records.join("\n")}${more > 0 ? `\n  … ${more} more` : ""}`,
    );
  },
);

server.registerTool(
  "cn_inbox",
  {
    title: "Actionable memory inbox",
    description:
      "The substrate's attention queue: pending todos and explicit user-actions extracted from " +
      "past sessions' z-insight blocks. Use for 'what needs my attention' / 'what's queued'. " +
      "The raw response is ~230 KB and ~500 items, so this tool renders the head plus the " +
      "untruncated total.",
    inputSchema: {
      limit: z.number().int().min(1).max(200).default(MAX_ROWS),
      kind: z.string().optional().describe("Filter rows by metadata.kind (todo | user_action | …)."),
    },
  },
  async (args) => {
    const r = await request<{
      items: Array<{ id: string; session_id?: string; content: string; importance?: number; metadata?: unknown }>;
    }>("/api/v1/inbox");
    if (!r.ok) return fail(r.message);
    const all = r.data.items ?? [];
    const filtered = args.kind
      ? all.filter((x) => (asObject(x.metadata)?.kind as string | undefined) === args.kind)
      : all;
    const rows = filtered.slice(0, args.limit ?? MAX_ROWS).map((x, i) => {
      const meta = asObject(x.metadata);
      const kind = (meta?.kind as string | undefined) ?? "item";
      const urgency = meta?.urgency ? ` urgency=${meta.urgency}` : "";
      return `  ${i + 1}. [${kind}${urgency}] ${clip(x.content, 180)}\n     id=${x.id}${x.session_id ? ` session=${x.session_id}` : ""}`;
    });
    if (!rows.length) return text(`Inbox empty${args.kind ? ` for kind=${args.kind}` : ""} (${r.ms} ms).`);
    return text(
      `inbox: ${filtered.length} item(s)${args.kind ? ` of kind=${args.kind}` : ""} ` +
        `(raw ${all.length} total, ${r.ms} ms) — showing ${rows.length}\n\n${rows.join("\n")}`,
    );
  },
);

server.registerTool(
  "cn_attention",
  {
    title: "Sessions needing user action",
    description: "Sessions flagged as blocked on the user. Complements cn_inbox (which is atom-level).",
    inputSchema: {},
  },
  async () => {
    const r = await request<unknown>("/api/v1/sessions/attention");
    if (!r.ok) return fail(r.message);
    return text(renderGeneric(`sessions needing attention (${r.ms} ms)`, r.data));
  },
);

server.registerTool(
  "cn_features",
  {
    title: "Features shipped in a window",
    description:
      "What shipped recently, each with a replayable how_to_test recipe. This is the 'what did we " +
      "ship yesterday and how do I re-verify it' query.",
    inputSchema: {
      since: z.string().default("24h").describe("24h | 7d | 30d"),
      layer: z.string().optional().describe("frontend | backend | infra | docs | tests | other"),
    },
  },
  async (args) => {
    const qs = new URLSearchParams({ since: args.since ?? "24h" });
    if (args.layer) qs.set("layer", args.layer);
    const r = await request<{
      count: number;
      features: Array<{
        session_id?: string;
        feature: string;
        ts?: string;
        layer?: string;
        how_to_test?: string;
        refs?: unknown;
        files?: unknown;
      }>;
    }>(`/api/v1/features?${qs}`);
    if (!r.ok) return fail(r.message);
    const f = r.data.features ?? [];
    if (!f.length) return text(`No features recorded in ${args.since} (${r.ms} ms).`);
    const rows = f.slice(0, MAX_ROWS).map((x, i) =>
      [
        `${i + 1}. ${x.feature}${x.layer ? ` [${x.layer}]` : ""}${x.ts ? ` ${x.ts}` : ""}`,
        x.how_to_test ? `   test: ${clip(x.how_to_test, 160)}` : "",
        x.session_id ? `   session=${x.session_id}` : "",
      ].filter(Boolean).join("\n"),
    );
    return text(
      `${f.length} feature(s) since ${args.since} (${r.ms} ms)${f.length > MAX_ROWS ? ` — showing ${MAX_ROWS}` : ""}\n\n${rows.join("\n")}`,
    );
  },
);

server.registerTool(
  "cn_outcome",
  {
    title: "Reinforce or dock recalled atoms",
    description:
      "Close the loop: after acting on atoms recalled via cn_retrieve / cn_prompt_context_capsule, " +
      "tell the substrate whether they helped, so future recall ranks them better. " +
      "IMPORTANT: read `updated` — it is the count of atoms actually written. `delta_applied` " +
      "is returned even when every id was stale and nothing was written. Only report outcomes " +
      "you actually observed; never fabricate feedback.",
    inputSchema: {
      atom_ids: z.array(z.string()).min(1).describe("Atom ids from retrieve/capsule results."),
      outcome: z.enum(["success", "failure"]),
      evidence: z.string().optional().describe("What you observed that justifies the signal."),
    },
  },
  async (args) => {
    const r = await request<{ updated: number; skipped_unknown: number; delta_applied: number }>(
      "/api/v1/agent/outcome",
      {
        method: "POST",
        body: { atom_ids: args.atom_ids, outcome: args.outcome, evidence: args.evidence },
      },
    );
    if (!r.ok) return fail(r.message);
    const { updated, skipped_unknown, delta_applied } = r.data;
    const verdict =
      updated > 0
        ? `reinforced ${updated} atom(s)`
        : `NOTHING was written (updated=0) — ${skipped_unknown} id(s) unknown/stale`;
    return text(
      `${verdict} · delta_applied=${delta_applied} (${r.ms} ms)\n` +
        (updated > 0
          ? `  ${args.outcome === "success" ? "+" : "−"} signal applied to ${updated} atom(s).`
          : `  delta_applied is the per-atom magnitude and is NOT a success indicator — trust \`updated\`.`),
    );
  },
);

// ─── boot ───────────────────────────────────────────────────────────────────

// MCP clients tear down stdio when a session ends, and a launcher that wraps
// this process can close the pipe mid-write. Both surface as EPIPE on stdout;
// neither should produce a stack trace. (Seen in the wild when the server was
// registered through a login shell, which also pollutes stdout with terminal
// escape sequences and breaks the handshake outright — see README.)
process.stdout.on("error", (err: NodeJS.ErrnoException) => {
  if (err.code === "EPIPE") process.exit(0);
  throw err;
});

const transport = new StdioServerTransport();
await server.connect(transport);
console.error(`contextnest-mcp ready — base=${BASE} (scan timeout ${SCAN_TIMEOUT_MS} ms, fast ${FAST_TIMEOUT_MS} ms)`);
