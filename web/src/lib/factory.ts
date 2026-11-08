import { z } from "zod";
import { supervisedRoute } from "./supervised";
import { factoryAuthorization, factoryCookie, factoryEnabled, factorySameOrigin, issueFactorySession } from "./factory-session";
import { localRequest, operatorToken } from "./auth";
import { avatarRoute, avatarsEnabled } from "./avatars";

const text = (max: number) => z.string().min(1).max(max).refine(v => v.trim() === v && !/[\u0000-\u001f\u007f]/.test(v));
const configuration = z.object({
  request_id: z.uuid(), name: text(80), description: text(600), purpose: text(600),
  personality: z.object({ voice: text(1000), traits: z.array(text(80)).max(12) }).strict(),
  goals: z.array(text(240)).min(1).max(8),
  requested_capabilities: z.array(z.enum(["read_memory", "remember", "evaluate_goal", "draft_tweet", "conserve"])).max(5),
  autonomy: z.literal("disabled"), spending_limit_microusd: z.literal(0), private_consent: z.literal(true), avatar_upload_id: z.uuid().optional(),
}).strict();
const agent = z.object({ agent_id: z.uuid(), configuration, status: z.literal("paused"), qualification: z.literal("pending_review"), public: z.literal(false), enabled_capabilities: z.array(z.never()), execution_enabled: z.literal(false), token_mint: z.null() }).strict();
const headers = { "Cache-Control": "private, no-store", "X-Content-Type-Options": "nosniff" };
const fail = (error: string, status: number) => Response.json({ error }, { status, headers });
async function bounded(request: Request | Response, maximum: number) {
  const reader = request.body?.getReader(); if (!reader) throw new Error("body_required");
  const chunks: Uint8Array[] = []; let size = 0;
  try { while (true) { const { done, value } = await reader.read(); if (done) break; size += value.length; if (size > maximum) { await reader.cancel(); throw new Error("body_limit"); } chunks.push(value); } }
  finally { reader.releaseLock(); }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}
export async function factoryRoute(request: Request, path: string): Promise<Response> {
  if (/^\/api\/factory\/(?:agents\/[0-9a-f-]{36}\/tasks|tasks\/)/.test(path)) return supervisedRoute(request,path);
  if (/^\/api\/factory\/avatar/.test(path)) return avatarRoute(request, path);
  if (!localRequest(request)) return fail("origin_not_permitted", 403);
  if (!factoryEnabled()) return fail("synthetic_factory_unavailable", 403);
  if (!["GET", "POST", "DELETE"].includes(request.method) || new URL(request.url).search) return fail("request_not_permitted", 400);
  if (request.method !== "GET" && (!factorySameOrigin(request) || (request.method === "POST" && request.headers.get("content-type") !== "application/json"))) return fail("same_origin_json_required", 403);
  if (path === "/api/factory/session" && request.method === "DELETE") return Response.json({ signed_out: true }, { headers: { ...headers, "Set-Cookie": `${factoryCookie}=; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=0` } });
  if (path === "/api/factory/session" && request.method === "POST") {
    try {
      const credentials = z.object({ username: text(80), password: z.string().min(24).max(512) }).strict().parse(await bounded(request, 2048));
      const session = issueFactorySession(credentials.username, credentials.password);
      if (!session) return fail("sign_in_failed", 401);
      // Test live API credential/expiry/role before issuing an authenticated session.
      const probe = await backend(request, "GET", `Basic ${Buffer.from(`${credentials.username}:${credentials.password}`).toString("base64")}`);
      if (probe.status !== 200) return probe;
      return Response.json({ signed_in: true, expires_in_seconds: 900 }, { headers: { ...headers, "Set-Cookie": `${factoryCookie}=${session}; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=900` } });
    } catch { return fail("invalid_sign_in", 400); }
  }
  const detail = /^\/api\/factory\/agents\/([0-9a-f-]{36})$/.exec(path)?.[1];
  if (detail && (!z.uuid().safeParse(detail).success || request.method !== "GET")) return fail("not_found", 404);
  if (!detail && (path !== "/api/factory/agents" || !["GET", "POST"].includes(request.method))) return fail("not_found", 404);
  const authorization = factoryAuthorization(request);
  if (!authorization) return fail("sign_in_required", 401);
  return backend(request, request.method as "GET" | "POST", authorization, detail);
}
async function backend(request: Request, method: "GET" | "POST", authorization: string, detail?: string): Promise<Response> {
  const token = operatorToken(authorization);
  if (!token) return fail("sign_in_required", 401);
  let body;
  if (method === "POST") {
    try {
      body = configuration.parse(await bounded(request, 12000));
      if (body.avatar_upload_id && !avatarsEnabled()) return fail("avatars_unavailable",403);
    }
    catch { return fail("invalid_agent_configuration", 400); }
  }
  try {
    const base = new URL(process.env.SUPA_BACKEND_URL ?? "");
    const privateHost = process.env.SUPA_SITE_MODE === "staging" && /^[a-z0-9][a-z0-9-]*\.railway\.internal$/.test(base.hostname);
    if (base.protocol !== "http:" || !(["127.0.0.1", "[::1]"].includes(base.hostname) || privateHost) || base.username || base.password || base.pathname !== "/" || base.search || base.hash) return fail("backend_configuration_invalid", 503);
    const response = await fetch(new URL("/factory/agents" + (detail ? `/${detail}` : ""), base), { method, headers: { Authorization: `Bearer ${token}`, Accept: "application/json", "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined, cache: "no-store", redirect: "error", signal: AbortSignal.timeout(7000) });
    if (!response.ok) { await response.body?.cancel(); return fail(response.status === 409 ? "submission_conflict" : "creation_not_authorized_or_unavailable", [400, 401, 403, 404, 409].includes(response.status) ? response.status : 503); }
    if (!response.headers.get("content-type")?.includes("application/json")) return fail("backend_contract_invalid", 502);
    const data = (method === "GET" && !detail ? z.object({ agents: z.array(agent).max(100) }).strict() : agent).safeParse(await bounded(response, 1_500_000));
    return data.success ? Response.json(data.data, { headers }) : fail("backend_contract_invalid", 502);
  } catch { return fail("backend_unavailable_no_automatic_retry", 503); }
}
