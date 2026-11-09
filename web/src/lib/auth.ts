import { createHash, timingSafeEqual } from "node:crypto";

// Each Basic account maps to its own server-side Rust credential. No identity headers.
export function operatorToken(header:string|null):string|null {
  if (!header?.startsWith("Basic ") || header.length>2048) return null;
  const accounts: {username:string;password:string;api_token:string}[]=[];
  const staging = process.env.SUPA_SITE_MODE === "staging";
  if(!staging && process.env.SUPA_WEB_USERNAME && process.env.SUPA_WEB_PASSWORD && process.env.SUPA_API_TOKEN)
    accounts.push({username:process.env.SUPA_WEB_USERNAME,password:process.env.SUPA_WEB_PASSWORD,api_token:process.env.SUPA_API_TOKEN});
  try {
    const configured:unknown=JSON.parse(process.env.SUPA_WEB_OPERATORS??"[]");
    if(!Array.isArray(configured)||configured.length>100) return null;
    for(const account of configured) {
      if(!account || typeof account.username!=="string" || typeof account.password!=="string" || typeof account.api_token!=="string") return null;
      accounts.push(account);
    }
  } catch { return null; }
  if(new Set(accounts.map(a=>a.username)).size!==accounts.length || new Set(accounts.map(a=>a.api_token)).size!==accounts.length) return null;
  const hash=(value:string)=>createHash("sha256").update(value).digest();
  for(const account of accounts) {
    if(staging && (account.api_token===process.env.SUPA_API_TOKEN || account.username===process.env.SUPA_STAGING_USERNAME || account.password===process.env.SUPA_STAGING_PASSWORD)) continue;
    if(account.password.length<24 || account.api_token.length<32 || account.api_token.length>256 || !/^[\x21-\x7e]+$/.test(account.api_token)) continue;
    if(timingSafeEqual(hash(header),hash(`Basic ${Buffer.from(`${account.username}:${account.password}`).toString("base64")}`))) return account.api_token;
  }
  return null;
}
export function operatorAuthorized(header: string | null): boolean { return operatorToken(header)!==null; }

export const officialSiteOrigin = "https://supaagency.xyz";
export function publicLandingHost(request: Request): boolean {
  return process.env.SUPA_SITE_MODE === "staging"
    && process.env.SUPA_PUBLIC_LANDING_ENABLED === "true"
    && request.headers.get("host") === "supaagency.xyz"
    && (!request.headers.get("origin") || request.headers.get("origin") === officialSiteOrigin);
}
export const publicSectionPaths = ['/home', '/agents', '/intelligence', '/memory', '/economy', '/social', '/trading', '/token-launch', '/factory', '/about'] as const;
export function publicLandingRequest(request: Request): boolean {
  if (!publicLandingHost(request) || !["GET", "HEAD"].includes(request.method)) return false;
  const path = new URL(request.url).pathname;
  // Only editorial public views. Agent details, APIs and controls are never admitted.
  return (process.env.SUPA_SYNTHETIC_FACTORY_ENABLED === "true" && process.env.SUPA_AGENT_AVATARS_ENABLED === "true" &&
      (/^\/api\/public\/agent-profiles(?:\/[0-9a-f-]{36})?$/.test(path) || /^\/api\/public\/agent-avatars\/[0-9a-f-]{36}\/[1-9][0-9]?$/.test(path) || /^\/agents\/[0-9a-f-]{36}$/.test(path)))
    || ["/", ...publicSectionPaths, "/robots.txt"].includes(path)
    || ["/style.css", "/app.js", "/scene.js", "/wheel.js", "/platform.js"].includes(path)
    || /^\/public\/(assets\/[a-z0-9-]+\.svg|fonts\/[a-zA-Z0-9._-]+\.(woff2|ttf|txt))$/.test(path)
    || /^\/public\/assets\/supa-(home|agents|intelligence|memory|economy|social|trading|factory|about)-(?:(thumb|480|960)\.webp|960\.avif)$/.test(path)
    || /^\/public\/assets\/supa-((economy|social|trading|factory)-loop|factory-artwork-loop)\.(webm|mp4)$/.test(path);
}

export function localRequest(request: Request): boolean {
  if (process.env.SUPA_SITE_MODE === "staging") {
    try {
      const configured = new URL(process.env.SUPA_SITE_ORIGIN ?? "");
      const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(configured.hostname);
      if ((configured.protocol !== "https:" && !(loopback && configured.protocol === "http:")) || configured.username || configured.password || configured.pathname !== "/" || configured.search || configured.hash) return false;
      const origin = request.headers.get("origin");
      return publicLandingHost(request) || (request.headers.get("host") === configured.host && (!origin || origin === configured.origin));
    } catch { return false; }
  }
  const allowed = (value: string) => {
    try {
      const url = new URL(value);
      return url.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname) && !url.username && !url.password;
    } catch { return false; }
  };
  const host = request.headers.get("host");
  if (!host || !allowed(`http://${host}`)) return false;
  const origin = request.headers.get("origin");
  return !origin || (allowed(origin) && new URL(origin).host === host);
}

/** Separate staging visitors can browse public identities without reading operator records. */
export function stagingAuthorized(header: string | null): boolean {
  const username = process.env.SUPA_STAGING_USERNAME, password = process.env.SUPA_STAGING_PASSWORD;
  if (!username || !password || password.length < 24 || username === process.env.SUPA_WEB_USERNAME || !header?.startsWith("Basic ") || header.length > 2048) return false;
  const digest = (value: string) => createHash("sha256").update(value).digest();
  return timingSafeEqual(digest(header), digest(`Basic ${Buffer.from(`${username}:${password}`).toString("base64")}`));
}
