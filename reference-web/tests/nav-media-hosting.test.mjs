/** Real local static-hosting path; no live credentials or backend requests. */
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {once} from 'node:events';

test('reviewed clips serve correct MIME while private routes and writes stay denied',async()=>{
 const root=resolve(import.meta.dirname,'..');
 const child=spawn(process.execPath,['server.mjs'],{cwd:root,stdio:'ignore',env:{PATH:process.env.PATH,HOME:process.env.HOME,PORT:'3297',SUPA_SITE_ENV_FILE:'',SUPA_BACKEND_URL:'',SUPA_API_TOKEN:'',SUPA_WEB_USERNAME:'',SUPA_WEB_PASSWORD:''}});
 const base='http://127.0.0.1:3297';
 const get=(path,options)=>fetch(base+path,{...options,signal:AbortSignal.timeout(2000)});
 try{
  let ready=false;const deadline=Date.now()+10000;
  while(Date.now()<deadline){if(child.exitCode!==null)throw Error('fixture_server_exited');try{if((await get('/healthz')).ok){ready=true;break;}}catch{}await new Promise(r=>setTimeout(r,100));}assert(ready);
  for(const format of ['webm','mp4']){
   const bytes=await readFile(resolve(root,'public/assets/supa-economy-loop.'+format));
   const response=await get('/public/assets/supa-economy-loop.'+format);assert.equal(response.status,200);assert.equal(response.headers.get('content-type'),'video/'+format);assert.deepEqual(Buffer.from(await response.arrayBuffer()),bytes);
   assert.match(response.headers.get('content-security-policy'),/media-src 'self'/);
   assert.equal((await get('/public/assets/supa-economy-loop.'+format,{method:'HEAD'})).status,200);
   assert.equal((await get('/public/assets/supa-economy-loop.'+format,{method:'POST'})).status,405);
  }
  for(const path of ['/operator','/my-agents','/api/agents/11111111-1111-4111-8111-111111111111/memory'])assert.equal((await get(path)).status,401);
  assert.equal((await get('/public/assets/unreviewed.mp4')).status,404);
  assert.equal((await get('/public/assets/%2e%2e%2fprivate.mp4')).status,400);
 }finally{
  const exited=once(child,'close');child.kill('SIGTERM');await exited;
 }
});
