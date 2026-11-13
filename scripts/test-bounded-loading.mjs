/** Focused DOM hydration regression with synthetic API fixtures, not a backend certification. */
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {chromium} from '../web/node_modules/playwright-core/index.mjs';
const root=resolve(import.meta.dirname,'..'),out=resolve(root,'artifacts/phase5-bounded-autonomy/final-local/loading');
await mkdir(out,{recursive:true,mode:0o700});
const id='11111111-1111-4111-8111-111111111111';
const html=(await readFile(resolve(root,'reference-web/factory-create.html'),'utf8')).replace('<html lang="en">','<html lang="en" data-bounded-enabled="true">');
const css=await readFile(resolve(root,'reference-web/style.css'));
const server=createServer((req,res)=>{if(req.url==='/my-agents/'+id){res.writeHead(200,{'Content-Type':'text/html'});res.end(html);}else if(req.url==='/style.css'){res.writeHead(200,{'Content-Type':'text/css'});res.end(css);}else {res.writeHead(404,{'Content-Type':'text/plain'});res.end('Fixture asset unavailable');}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const origin='http://127.0.0.1:'+server.address().port;
const agent={agent_id:id,configuration:{name:'Synthetic loading fixture',description:'No real records',purpose:'Review synthetic notes',personality:{voice:'Precise',traits:[]},goals:['Review synthetic notes'],requested_capabilities:['read_memory','remember','evaluate_goal']},status:'paused'};
const goal={id,description:'Review synthetic notes',completed:false};
const run={state:'paused',revision:3,cycles_started:1,actions_reserved:3,approved_until:'2026-11-13T20:00:00Z',scope_sha256:'a'.repeat(64),scope:{max_cycles:3,max_actions:9,max_spending_microusd:0},cycles:[],events:[]};
const report={passed:false,cases:[],external_requests:0,writes:0,page_errors:[],automatic_retries:0};let browser;
try{
 browser=await chromium.launch({executablePath:'/usr/bin/google-chrome',headless:true});
 for(const [device,viewport] of [['desktop',{width:1440,height:1000}],['mobile',{width:390,height:844}]])for(const state of ['saved','empty','error']){
  const context=await browser.newContext({viewport,isMobile:device==='mobile',hasTouch:device==='mobile',serviceWorkers:'block'});
  try{
   let release;const gate=new Promise(r=>{release=r;});let gets=0,recover=false;
   await context.route('**/*',async route=>{const req=route.request(),url=new URL(req.url());if(url.origin!==origin){report.external_requests++;return route.abort();}
    if(req.method()!=='GET'){report.writes++;return route.abort();}
    if(url.pathname==='/api/factory/agents/'+id+'/autonomy'){gets++;await gate;if(state==='error'&&!recover)return route.fulfill({status:503,contentType:'application/json',body:'{"error":"synthetic_unavailable"}'});return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({run:state==='empty'?null:run,goals:[goal]})});}
    if(url.pathname==='/api/factory/agents/'+id)return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(agent)});
    if(url.pathname.startsWith('/public/'))return route.fulfill({status:200,contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg"/>'});return route.continue();
   });
   const page=await context.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>report.page_errors.push(e.name));
   assert((await page.goto(origin+'/my-agents/'+id,{waitUntil:'domcontentloaded'})).ok());
   assert.equal(await page.locator('#bounded-status').textContent(),'Loading saved scope…');release();
   const expected=state==='empty'?'Choose an existing goal and prepare its bounded scope.':state==='error'?'Schedule unavailable. Your agent remains unchanged. Refresh to inspect saved state.':'Saved schedule loaded. Execution state: paused.';
   await page.waitForFunction(text=>document.querySelector('#bounded-status').textContent===text,expected);
   assert.equal(gets,1,'hydration_must_not_retry');assert(!(await page.locator('#bounded-status').textContent()).includes('Loading'));
   if(state==='empty')assert(await page.locator('#bounded-form').isVisible());
   if(state==='saved')assert(await page.getByRole('heading',{name:'Schedule / paused',exact:true}).isVisible());
   if(state==='error'){
    recover=true;await page.getByRole('button',{name:'Refresh schedule',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#bounded-status').textContent==='Saved schedule loaded. Execution state: paused.');assert.equal(gets,2,'only_explicit_refresh_may_repeat_read');
   }
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
   if(state==='saved')await page.locator('#bounded-autonomy').screenshot({path:resolve(out,device+'.png')});
   report.cases.push({device,state,passed:true,reads:gets,automatic_retry:false});
  }finally{await context.close();}
 }
 assert.equal(report.writes,0);assert.equal(report.external_requests,0);assert.deepEqual(report.page_errors,[]);report.passed=true;
}finally{await browser?.close();server.closeAllConnections();await new Promise(r=>server.close(r));await writeFile(resolve(out,'result.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify(report));}
