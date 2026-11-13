import { NextRequest, NextResponse } from "next/server";
import { localRequest, operatorAuthorized, stagingAuthorized, publicLandingRequest, officialSiteOrigin } from "./lib/auth";

import { factoryAuthorization, factoryEnabled, factorySameOrigin } from "./lib/factory-session";

export function proxy(request: NextRequest) {
  if (process.env.SUPA_SITE_MODE === "staging" && process.env.SUPA_PUBLIC_LANDING_ENABLED === "true"
      && request.headers.get("host") === "www.supaagency.xyz"
      && (!request.headers.get("origin") || request.headers.get("origin") === "https://www.supaagency.xyz")
      && ["GET", "HEAD"].includes(request.method)) {
    return NextResponse.redirect(new URL(request.nextUrl.pathname + request.nextUrl.search, officialSiteOrigin), 308);
  }
  if (process.env.SUPA_SITE_MODE === "staging" && request.nextUrl.pathname === "/healthz") {
    const headers = new Headers(request.headers); headers.set("x-supa-experience", "1"); headers.delete("x-supa-public-landing");
    return NextResponse.next({ request: { headers } });
  }
  if (!localRequest(request)) return new NextResponse("Local access only", { status: 403 });
  if (process.env.SUPA_SITE_MODE === "staging") {
    const headers = new Headers(request.headers);
    headers.set("x-supa-experience", "1"); headers.delete("x-supa-public-landing");
    if (request.nextUrl.pathname === "/healthz") return NextResponse.next({ request: { headers } });
    const authorization = request.headers.get("authorization");
    const factoryPath = ["/factory/create", "/my-agents", "/api/factory/agents"].includes(request.nextUrl.pathname)
      || /^\/(?:my-agents|api\/factory\/agents)\/[0-9a-f-]{36}$/.test(request.nextUrl.pathname)
      || (process.env.SUPA_AGENT_AVATARS_ENABLED === "true" && /^\/api\/factory\/avatar(?:s|-[a-z]+)(?:\/|$)/.test(request.nextUrl.pathname));
    const supervisedPath = process.env.SUPA_SUPERVISED_TASKS_ENABLED === "true" && /^\/api\/factory\/(?:agents\/[0-9a-f-]{36}\/tasks|tasks\/[0-9a-f-]{36}(?:\/command)?)$/.test(request.nextUrl.pathname);
    const boundedPath = process.env.SUPA_BOUNDED_AUTONOMY_ENABLED === 'true' && process.env.SUPA_SUPERVISED_TASKS_ENABLED === 'true' && /^\/api\/factory\/agents\/[0-9a-f-]{36}\/autonomy$/.test(request.nextUrl.pathname);
    const factoryAuth = (factoryPath || supervisedPath || boundedPath) && !!factoryAuthorization(request);
    const signInAsset = request.nextUrl.pathname === "/style.css" || /^\/public\/(assets\/[a-z0-9-]+\.svg|fonts\/[a-zA-Z0-9._-]+\.(woff2|ttf|txt))$/.test(request.nextUrl.pathname);
    const factorySignIn = factoryEnabled() && (((request.nextUrl.pathname === "/sign-in" || signInAsset) && request.method === "GET") || (request.nextUrl.pathname === "/api/factory/session" && ["POST", "DELETE"].includes(request.method) && factorySameOrigin(request)));
    const authenticated = operatorAuthorized(authorization) || stagingAuthorized(authorization) || factoryAuth || factorySignIn;
    const publicRead = !authenticated && publicLandingRequest(request);
    if (publicRead) headers.set("x-supa-public-landing", "1");
    if (!authenticated && !publicRead) return new NextResponse("Private staging authentication required", {
      status: 401, headers: { "WWW-Authenticate": 'Basic realm="Supa staging", charset="UTF-8"', "Cache-Control": "no-store" },
    });
    const response = NextResponse.next({ request: { headers } });
    response.headers.set("Cache-Control", "private, no-store");
    if (publicRead) response.headers.set("X-Robots-Tag", request.nextUrl.pathname === "/" ? "index, follow" : "noindex, nofollow");
    return response;
  }
  // The public disconnected shell contains no operator data. Configuration activates authentication.
  if (!process.env.SUPA_BACKEND_URL && !process.env.SUPA_API_TOKEN) return NextResponse.next();
  if (!process.env.SUPA_WEB_USERNAME || (process.env.SUPA_WEB_PASSWORD?.length ?? 0) < 24) {
    return new NextResponse("Operator authentication is not configured", { status: 503 });
  }
  if (!operatorAuthorized(request.headers.get("authorization"))) {
    return new NextResponse("Operator authentication required", {
      status: 401, headers: { "WWW-Authenticate": 'Basic realm="Supa local operator", charset="UTF-8"', "Cache-Control": "no-store" },
    });
  }
  const response = NextResponse.next();
  response.headers.set("Cache-Control", "no-store");
  return response;
}
export const config = { matcher: ["/((?!_next/static|_next/image|favicon.ico).*)"] };
