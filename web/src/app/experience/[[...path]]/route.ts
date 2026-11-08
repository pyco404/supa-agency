/** Next.js hosts the approved experience unchanged; secrets stay in the server adapter. */
import { readFile, realpath } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { readResource, runAutonomy, isEconomicPath } from "../../../../../reference-web/backend.mjs";
import { operatorAuthorized, publicLandingRequest, publicSectionPaths } from "@/lib/auth";

import { avatarRoute, avatarsEnabled } from "@/lib/avatars";
import { supervisedEnabled } from "@/lib/supervised";
import { factoryRoute } from "@/lib/factory";
import { factoryAuthorization, factoryEnabled } from "@/lib/factory-session";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";
export async function POST(request: Request, context: { params: Promise<{ path?: string[] }> }) {
  if(process.env.SUPA_SITE_MODE!=="staging")return new Response("Not found",{status:404});
  const path='/'+((await context.params).path??[]).join('/');
  if (path.startsWith('/api/factory/')) return factoryRoute(request, path);
  if((!isEconomicPath(path)&&path!=="/api/autonomy"&&!/^\/api\/agents\/[0-9a-f-]{36}\/(?:schedule|live-work|live(?:\/[0-9a-f-]{36}\/approval)?)$/i.test(path))||new URL(request.url).search)return new Response("Method not allowed",{status:405});
  if(!operatorAuthorized(request.headers.get("authorization")))return Response.json({error:"operator_authentication_required"},{status:401});
  const reader=request.body?.getReader();let size=0;const chunks:Uint8Array[]=[];
  if(!reader)return new Response("Invalid request",{status:400});
  try{while(true){const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>(isEconomicPath(path)?12000:4096)){await reader.cancel();return new Response("Request too large",{status:413});}chunks.push(value);}}finally{reader.releaseLock();}
  const result=await runAutonomy(path,request.headers,Buffer.concat(chunks).toString('utf8'));
  return Response.json(result.body,{status:result.status,headers:{"Cache-Control":"private, no-store"}});
}
const files = new Set(["index.html", "style.css", "app.js", "scene.js", "wheel.js", "platform.js"]);
const mime: Record<string, string> = { ".html":"text/html; charset=utf-8", ".css":"text/css", ".js":"text/javascript", ".svg":"image/svg+xml", ".webp":"image/webp", ".avif":"image/avif", ".woff2":"font/woff2", ".ttf":"font/ttf", ".txt":"text/plain" };
const headers = { "Cache-Control":"private, no-store", "X-Content-Type-Options":"nosniff" };
export async function DELETE(request: Request, context: { params: Promise<{ path?: string[] }> }) {
  const path = "/" + ((await context.params).path ?? []).join("/");
  return factoryRoute(request, path);
}
export async function GET(request: Request, context: { params: Promise<{ path?: string[] }> }) {
  if (process.env.SUPA_SITE_MODE !== "staging") return new Response("Not found", {status:404, headers});
  const parts = (await context.params).path ?? [];
  if (parts.some(part=>part.includes("\\") || part.includes("\0") || part === "." || part === "..")) return new Response("Invalid path", {status:400, headers});
  const path = "/"+parts.join("/");
  if (path.startsWith("/api/public/")) return avatarRoute(request,path);
  if (path.startsWith("/api/factory/")) return factoryRoute(request, path);
  if (factoryEnabled() && (["/sign-in", "/factory/create", "/my-agents"].includes(path) || /^\/my-agents\/[0-9a-f-]{36}$/.test(path))) {
    if (path !== "/sign-in" && !factoryAuthorization(request)) return new Response(null, {status:303,headers:{...headers,Location:"/sign-in"}});
    return new Response((await readFile(resolve(process.cwd(), "../reference-web/factory-create.html"),"utf8")).replace('<html lang="en">','<html lang="en"'+(avatarsEnabled()?' data-avatars-enabled="true"':'')+(supervisedEnabled()?' data-supervised-enabled="true"':'')+'>'),{headers:{...headers,"Content-Type":"text/html; charset=utf-8"}});
  }
  const publicLanding = request.headers.get("x-supa-public-landing") === "1"
    && publicLandingRequest(new Request(new URL(path, request.url), {method:request.method, headers:request.headers}));
  if (publicLanding && avatarsEnabled() && /^\/agents\/[0-9a-f-]{36}$/.test(path)) {
    const check=await avatarRoute(new Request(new URL("/api/public/agent-profiles/"+parts[1],request.url),{headers:request.headers}),"/api/public/agent-profiles/"+parts[1]);
    if(check.status!==200)return new Response("No approved public profile",{status:check.status,headers});
    return new Response(await readFile(resolve(process.cwd(),"../reference-web/public-agent-profile.html")),{headers:{...headers,"Content-Type":"text/html; charset=utf-8"}});
  }
  if (publicLanding && path === "/robots.txt") return new Response(`User-agent: *\nAllow: /$\n${publicSectionPaths.map(path=>`Allow: ${path}$\n`).join("")}Disallow: /api/\nDisallow: /operator\n`, {headers:{...headers,"Content-Type":"text/plain; charset=utf-8"}});
  if (publicLanding && ["/", ...publicSectionPaths].includes(path)) {
    const html = await readFile(resolve(process.cwd(), "../reference-web/index.html"), "utf8");
    return new Response(html.replace('data-motion="running"', 'data-motion="running" data-public-landing="true"')
      .replace('content="noindex, nofollow"', 'content="index, follow"')
      .replace('class="enter" href="/"', 'class="enter" href="/home"')
      .replace('data-view="intro"', avatarsEnabled() ? 'data-view="intro" data-agent-avatars="true"' : 'data-view="intro"')
      .replace('data-public-landing="true"', factoryEnabled() ? 'data-public-landing="true" data-synthetic-factory="true"' : 'data-public-landing="true"'), {headers:{...headers,"Content-Type":"text/html; charset=utf-8"}});
  }
  if (path === "/healthz") return Response.json({status:"ok",mode:"private-staging",signing_enabled:false},{headers});
  if (path === "/my-agents") {
    if (!operatorAuthorized(request.headers.get("authorization"))) return new Response("Authenticated management access required", {status:401,headers});
    return new Response(await readFile(resolve(process.cwd(), "../reference-web/my-agents.html")),
      {headers:{...headers,"Content-Type":"text/html; charset=utf-8"}});
  }
  if (path === "/operator") {
    if (!operatorAuthorized(request.headers.get("authorization"))) return new Response("Operator authentication required", {status:401,headers:{...headers,"WWW-Authenticate":'Basic realm="Supa operator", charset="UTF-8"'}});
    return new Response(null, {status:302, headers:{...headers, Location:"/agents"}});
  }
  if (path.startsWith("/api/")) {
    const result = await readResource(path+new URL(request.url).search, request.headers);
    return Response.json(result.body, {status:result.status,headers});
  }
  const sections = ["/", "/agents", "/intelligence", "/memory", "/economy", "/social", "/trading", "/token-launch", "/factory", "/about"];
  if(path==="/home")return new Response(null,{status:308,headers:{...headers,Location:"/"+new URL(request.url).search}});
  const file = [...sections, "/site", "/site/"].includes(path) ? "index.html" : parts.join("/");
  const asset = /^public\/(assets\/[a-z0-9-]+\.svg|fonts\/[a-zA-Z0-9._-]+\.(woff2|ttf|txt))$/.test(file)
    || /^public\/assets\/supa-(home|agents|intelligence|memory|economy|social|trading|factory|about)-(?:(thumb|480|960)\.webp|960\.avif)$/.test(file);
  if (!files.has(file) && !asset) return new Response(await readFile(resolve(process.cwd(),"../reference-web/not-found.html")), {status:404, headers:{...headers,"Content-Type":"text/html; charset=utf-8"}});
  try {
    const root = await realpath(resolve(process.cwd(), "../reference-web"));
    const target = await realpath(resolve(root,file));
    if (!target.startsWith(root+sep)) return new Response("Not found", {status:404,headers});
    return new Response(await readFile(target), {headers:{...headers,"Content-Type":mime[extname(file)] ?? "application/octet-stream"}});
  } catch {return new Response("Asset unavailable", {status:503,headers});}
}
