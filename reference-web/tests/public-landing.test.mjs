import { test } from 'node:test';
import assert from 'node:assert/strict';
import { publicLandingHost, publicLandingRequest, localRequest } from '../../web/src/lib/auth.ts';
const saved={...process.env};
function configured(){process.env.SUPA_SITE_MODE='staging';process.env.SUPA_SITE_ORIGIN='https://supa-web-staging.up.railway.app';process.env.SUPA_PUBLIC_LANDING_ENABLED='true';}
function request(path='/',options={}){return new Request('https://supaagency.xyz'+path,{method:options.method??'GET',headers:{host:options.host??'supaagency.xyz',...(options.origin?{origin:options.origin}:{})}});}
function restore(){for(const k of ['SUPA_SITE_MODE','SUPA_SITE_ORIGIN','SUPA_PUBLIC_LANDING_ENABLED'])if(saved[k]===undefined)delete process.env[k];else process.env[k]=saved[k];}
test('public landing is explicitly enabled only for the official host',()=>{try{
 configured();assert.equal(publicLandingHost(request()),true);assert.equal(localRequest(request()),true);
 delete process.env.SUPA_PUBLIC_LANDING_ENABLED;assert.equal(publicLandingRequest(request()),false);assert.equal(localRequest(request()),false);
 configured();for(const host of ['supa-web-staging.up.railway.app','supaagency.xyz.evil.invalid','evil.invalid','supaagency.xyz:444','www.supaagency.xyz'])assert.equal(publicLandingRequest(request('/',{host})),false);
}finally{restore();}});
test('only editorial public views and bounded assets are public; records, controls and writes stay closed',()=>{try{
 configured();for(const path of ['/','/home','/agents','/intelligence','/memory','/economy','/social','/trading','/token-launch','/factory','/about','/robots.txt','/style.css','/app.js','/scene.js','/public/assets/supa-agent-icon.svg','/public/fonts/IBMPlexMono-Regular.ttf'])assert.equal(publicLandingRequest(request(path)),true,path);
 for(const path of ['/my-agents','/my-agents/private','/operator','/wallet','/wallets','/management','/admin','/api/wallet','/agents/894ec18a-bb53-4f82-955f-7eff9f92d8a1','/agents/new','/memory/private','/economy/wallet','/social/drafts','/intelligence/control','/trading/orders','/token-launch/create','/factory/create','/api/status','/api/agents','/api/operator-status','/api/autonomy','/api/console/agents','/experience','/experience/agents','/experience/api/agents','/.env','/public/secrets.env'])assert.equal(publicLandingRequest(request(path)),false,path);
 for(const path of ['/home/','/agents/','/about/private','/AGENTS','/agents%2fprivate'])assert.equal(publicLandingRequest(request(path)),false,path);
 for(const method of ['POST','PUT','PATCH','DELETE','OPTIONS'])assert.equal(publicLandingRequest(request('/',{method})),false,method);
 for(const path of ['/home', '/agents', '/intelligence', '/memory', '/economy', '/social', '/trading', '/token-launch', '/factory', '/about'])for(const method of ['POST','PUT','PATCH','DELETE','OPTIONS'])assert.equal(publicLandingRequest(request(path,{method})),false,`${method} ${path}`);
 assert.equal(publicLandingRequest(request('/',{method:'HEAD'})),true);
}finally{restore();}});
test('origin checks reject cross-origin requests and preserve authenticated staging origin',()=>{try{
 configured();assert.equal(publicLandingRequest(request('/',{origin:'https://supaagency.xyz'})),true);
 assert.equal(publicLandingRequest(request('/',{origin:'https://untrusted.invalid'})),false);
 assert.equal(localRequest(request('/api/operator-status',{origin:'https://untrusted.invalid'})),false);
 assert.equal(localRequest(request('/',{host:'supa-web-staging.up.railway.app',origin:'https://supa-web-staging.up.railway.app'})),true);
 assert.equal(localRequest(request('/',{host:'supa-web-staging.up.railway.app',origin:'https://supaagency.xyz'})),false);
 process.env.SUPA_SITE_MODE='development';assert.equal(publicLandingRequest(request()),false);
}finally{restore();}});
test('navigation loops admit only reviewed public asset names and preserve origin/method boundaries',()=>{try{
 configured();
 for(const section of ['economy','social','trading','factory'])for(const format of ['webm','mp4']){
  const path=`/public/assets/supa-${section}-loop.${format}`;
  assert.equal(publicLandingRequest(request(path)),true,path);
  assert.equal(publicLandingRequest(request(path,{method:'HEAD'})),true,path);
  assert.equal(publicLandingRequest(request(path,{method:'POST'})),false,path);
  assert.equal(publicLandingRequest(request(path,{origin:'https://untrusted.invalid'})),false,path);
  assert.equal(publicLandingRequest(request(path,{host:'supa-web-staging.up.railway.app'})),false,path);
 }
 for(const path of ['/public/assets/private.webm','/public/uploads/avatar.mp4','/public/assets/supa-home-loop.webm','/public/assets/supa-economy-loop.mp4/extra','/public/assets/supa-economy-loop.mp4.secret','/api/private.mp4','/operator'])assert.equal(publicLandingRequest(request(path)),false,path);
 process.env.SUPA_PUBLIC_LANDING_ENABLED='false';assert.equal(publicLandingRequest(request('/public/assets/supa-economy-loop.webm')),false);
}finally{restore();}});
