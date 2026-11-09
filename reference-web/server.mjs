import { createServer } from "node:http";
import { readFile, realpath, stat } from "node:fs/promises";
import { dirname, extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { loadLocalEnvironment, readResource, runAutonomy, isEconomicPath, operatorAuthorized, localRequest } from './backend.mjs';
import { navigation } from './platform.js';
await loadLocalEnvironment();

const root = dirname(fileURLToPath(import.meta.url));
const port = Number(process.env.PORT ?? 3200);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Invalid local port");
const appFiles = new Set(["index.html", "style.css", "app.js", "scene.js", "wheel.js", "platform.js"]);
const media = new Map([
  [".html", "text/html; charset=utf-8"], [".css", "text/css; charset=utf-8"], [".txt", "text/plain; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"], [".json", "application/json; charset=utf-8"], [".woff2", "font/woff2"],
  [".woff", "font/woff"], [".ttf", "font/ttf"], [".png", "image/png"],
  [".jpg", "image/jpeg"], [".jpeg", "image/jpeg"], [".webp", "image/webp"],
  [".webm", "video/webm"], [".mp4", "video/mp4"], [".avif", "image/avif"], [".svg", "image/svg+xml"], [".ico", "image/x-icon"],
]);
const publicExtensions = new Set([".woff2", ".woff", ".ttf", ".png", ".jpg", ".jpeg", ".webp", ".avif", ".svg", ".ico"]);

function finish(response, status, message) {
  response.writeHead(status, { "Content-Type": "text/plain; charset=utf-8" });
  response.end(message);
}

createServer(async (request, response) => {
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("Referrer-Policy", "no-referrer");
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; media-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'");
  if (request.method !== "GET" && request.method !== "HEAD" && !(request.method === 'POST' && (isEconomicPath(request.url)||['/api/autonomy','/api/launch'].includes(request.url)||/^\/api\/agents\/[0-9a-f-]{36}\/(?:schedule|live-work|live(?:\/[0-9a-f-]{36}\/approval)?)$/i.test(request.url)))) {
    response.setHeader("Allow", "GET, HEAD");
    return finish(response, 405, "Operation not permitted");
  }
  let path;
  try { path = decodeURIComponent((request.url ?? "/").split("?", 1)[0]); }
  catch { return finish(response, 400, "Invalid path"); }
  if (path.includes("\0") || path.includes("\\") || path.split("/").some(part => part === "." || part === "..")) {
    return finish(response, 400, "Invalid path");
  }
  if (!localRequest(new Request('http://127.0.0.1', { headers:request.headers }))) return finish(response,403,'Local access only');
  if(request.method==='POST'){
    if(!operatorAuthorized(request.headers.authorization))return finish(response,401,'Operator authentication required');
    const chunks=[];let size=0;
    try{for await(const chunk of request){size+=chunk.length;if(size>(isEconomicPath(request.url)?12000:4096))return finish(response,413,'Request too large');chunks.push(chunk);}}
    catch{return finish(response,400,'Invalid request');}
    const result=await runAutonomy(request.url,request.headers,Buffer.concat(chunks).toString('utf8'));
    response.writeHead(result.status,{'Content-Type':'application/json; charset=utf-8'});return response.end(JSON.stringify(result.body));
  }
  if (path === '/operator') {
    if (!operatorAuthorized(request.headers.authorization)) {
      response.setHeader('WWW-Authenticate','Basic realm="Supa local operator", charset="UTF-8"');
      return finish(response,401,'Operator authentication required');
    }
    response.writeHead(302,{Location:'/agents'});return response.end();
  }
  if (path === '/my-agents') {
    if (!operatorAuthorized(request.headers.authorization)) return finish(response,401,'Authenticated management access required');
    response.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});
    return response.end(request.method==='HEAD'?undefined:await readFile(resolve(root,'my-agents.html')));
  }
  if (path === '/token-launch') {
    response.writeHead(308,{Location:'/factory'+new URL(request.url,'http://localhost').search});return response.end();
  }
  if (path.startsWith('/api/')) {
    const result = await readResource(request.url, request.headers);
    response.writeHead(result.status, {'Content-Type':'application/json; charset=utf-8'});
    return response.end(request.method === 'HEAD' ? undefined : JSON.stringify(result.body));
  }
  if (path === "/healthz") {
    response.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
    return response.end(request.method === "HEAD" ? undefined : JSON.stringify({ status: "ok", mode: "supa-bounded-offline", host: "127.0.0.1" }));
  }
  const canonical = path === '/home' || path === '/home/' ? '/' : navigation.find(item=>item.path!=='/' && path===item.path+'/')?.path;
  if(canonical){response.writeHead(308,{Location:canonical+new URL(request.url,'http://localhost').search});return response.end();}
  const file = navigation.some(item=>item.path===path) || path === "/" || path === "/site" || path === "/site/" ? "index.html" : path.slice(1);
  const isPublic = file.startsWith("public/") && file.split("/").every(part => part && !part.startsWith(".")) && (publicExtensions.has(extname(file)) || /^public\/assets\/supa-((economy|social|trading|factory)-loop|factory-artwork-loop)\.(webm|mp4)$/.test(file) || file === "public/fonts/OFL.txt");
  if (!appFiles.has(file) && !isPublic) {
    response.writeHead(404,{'Content-Type':'text/html; charset=utf-8'});
    return response.end(request.method==='HEAD'?undefined:await readFile(resolve(root,'not-found.html')));
  }
  try {
    const target = await realpath(resolve(root, file));
    const safeRoot = isPublic ? resolve(root, "public") : root;
    if (!target.startsWith(`${safeRoot}${sep}`) || !(await stat(target)).isFile()) return finish(response, 404, "Not found");
    const content = await readFile(target);
    response.writeHead(200, { "Content-Type": media.get(extname(file)), "Content-Length": content.byteLength });
    response.end(request.method === "HEAD" ? undefined : content);
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ENOTDIR" || error.code === "EACCES") return finish(response, 404, "Not found");
    finish(response, 500, "Local asset unavailable");
  }
}).listen(port, "127.0.0.1", () => console.log(`Supa Agency local: http://127.0.0.1:${port}`));
