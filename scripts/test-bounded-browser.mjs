/** Real browser -> Next -> Rust -> disposable PG18; no external or model requests. */
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {randomBytes} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createServer} from 'node:net';
import {chromium} from '../web/node_modules/playwright-core/index.mjs';
const root=resolve(import.meta.dirname,'..'),out=resolve(root,'artifacts/phase5-bounded-autonomy/system');
const api=new URL(process.env.SUPA_TEST_API_ORIGIN);assert.equal(api.hostname,'127.0.0.1');assert.equal(api.protocol,'http:');
const accounts=JSON.parse(process.env.SUPA_TEST_ACCOUNTS);assert.equal(accounts.length,2);
for(const a of accounts){assert(a.api_token.length>=32);a.password=randomBytes(32).toString('hex');}
const port=await new Promise(resolve=>{const s=createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p));});});
const origin='http://127.0.0.1:'+port;
await mkdir(out,{recursive:true,mode:0o700});
const env={PATH:process.env.PATH,HOME:process.env.HOME,NODE_ENV:'development',SUPA_SITE_MODE:'staging',SUPA_SITE_ORIGIN:origin,SUPA_BACKEND_URL:api.origin,SUPA_PUBLIC_LANDING_ENABLED:'true',SUPA_SYNTHETIC_FACTORY_ENABLED:'true',SUPA_SUPERVISED_TASKS_ENABLED:'true',SUPA_BOUNDED_AUTONOMY_ENABLED:'true',SUPA_WEB_OPERATORS:JSON.stringify(accounts)};
const nextEnv=resolve(root,'web/next-env.d.ts'),originalTypes=await readFile(nextEnv);
const child=spawn(process.execPath,['node_modules/next/dist/bin/next','dev','--webpack','--hostname','127.0.0.1','--port',String(port)],{cwd:resolve(root,'web'),env,stdio:['ignore','pipe','pipe'],detached:true});
const log=[];const redact=chunk=>{let s=chunk.toString();for(const a of accounts)for(const x of [a.api_token,a.password,Buffer.from(a.username+':'+a.password).toString('base64')])s=s.split(x).join('[redacted]');log.push(s.replaceAll(root,'[workspace]').slice(-4000));if(log.length>40)log.shift();};child.stdout.on('data',redact);child.stderr.on('data',redact);
const report={passed:false,contexts:[],external_requests:0,page_errors:[],retries:0,paid_requests:0};let browser,phase='startup';const started=Date.now();
async function read(page,path){return page.evaluate(async path=>{const r=await fetch(path,{credentials:'same-origin',cache:'no-store'});return {status:r.status,value:r.ok?await r.json():null};},path);}
try{
 const deadline=Date.now()+90000;let ready=false;
 while(Date.now()<deadline){if(child.exitCode!==null)throw Error('next_process_exited');try{const r=await fetch(origin+'/healthz',{signal:AbortSignal.timeout(Math.max(1,deadline-Date.now()))});assert.equal(r.status,200);assert.equal((await r.json()).status,'ok');ready=true;break;}catch(e){if(e.code!=='ECONNREFUSED'&&e.cause?.code!=='ECONNREFUSED')throw e;await new Promise(r=>setTimeout(r,200));}}
 assert(ready,'next_readiness_deadline');report.startup_ms=Date.now()-started;
 browser=await chromium.launch({executablePath:'/usr/bin/google-chrome',headless:true});
 let previousAgent;
 for(const [i,viewport] of [[0,{width:1440,height:1000}],[1,{width:390,height:844}]]){
  const context=await browser.newContext({viewport,isMobile:!!i,hasTouch:!!i,serviceWorkers:'block',reducedMotion:'reduce'});
  try{
   await context.route('**/*',route=>{if(new URL(route.request().url()).origin!==origin){report.external_requests++;return route.abort();}return route.continue();});
   const page=await context.newPage();page.setDefaultTimeout(30000);page.on('pageerror',e=>report.page_errors.push(e.name));
   assert.equal((await context.request.get(origin+'/api/factory/agents/11111111-1111-4111-8111-111111111111/autonomy')).status(),401);
   phase='signin_'+i;const navigation=await page.goto(origin+'/sign-in',{waitUntil:'domcontentloaded'});assert(navigation.ok());
   await page.getByLabel('Synthetic account',{exact:true}).fill(accounts[i].username);await page.getByLabel('Password',{exact:true}).fill(accounts[i].password);await page.getByRole('button',{name:'Sign In',exact:true}).click();await page.waitForURL(origin+'/factory/create');
   if(previousAgent)assert.equal((await read(page,'/api/factory/agents/'+previousAgent+'/autonomy')).status,403);
   phase='create_'+i;const name='Bounded browser '+(i?'mobile':'desktop');
   await page.getByLabel('Name',{exact:true}).fill(name);await page.getByLabel('Description',{exact:true}).fill('Disposable bounded execution demonstration');await page.getByLabel('Purpose',{exact:true}).fill('Review synthetic incident notes');await page.getByLabel('Personality / voice',{exact:true}).fill('Curious and precise');await page.getByLabel('Goals / one per line, up to eight',{exact:true}).fill('Review synthetic incident notes');
   for(const capability of ['read_memory','remember','evaluate_goal'])await page.locator('input[name=capability][value='+capability+']').check();await page.locator('#consent').check();await page.getByRole('button',{name:'Review configuration',exact:true}).click();await page.getByRole('button',{name:'Create private paused agent',exact:true}).click();await page.waitForURL(origin+'/my-agents');
   await page.locator('#agents article').filter({has:page.getByRole('heading',{name,exact:true})}).getByRole('link',{name:'View details / configuration',exact:true}).click();await page.waitForURL(/\/my-agents\/[0-9a-f-]{36}$/);
   const agent=new URL(page.url()).pathname.split('/').at(-1),endpoint='/api/factory/agents/'+agent+'/autonomy';previousAgent=agent;
   const panel=page.locator('#bounded-autonomy');await panel.waitFor({state:'visible'});await page.waitForFunction(()=>document.querySelector('#bounded-goal')?.options.length>0);
   phase='scope_'+i;await page.locator('#bounded-outcome').fill('Synthetic');await page.locator('#bounded-material').fill('Synthetic latency rose after a test change. Synthetic service recovered after restart. Causation is unknown.');await page.locator('#bounded-interval').fill('30');await page.locator('#bounded-synthetic').check();await panel.getByRole('button',{name:'Prepare scope for review',exact:true}).click();await panel.getByRole('heading',{name:'Schedule / pending',exact:true}).waitFor();
   let r=(await read(page,endpoint)).value.run;assert.equal(r.cycles_started,0);assert.equal(r.scope.max_spending_microusd,0);assert.equal(r.scope.input.retain_memory,false);
   await page.locator('#bounded-consent').check();await panel.getByRole('button',{name:'Approve exact scope',exact:true}).click();await panel.getByRole('heading',{name:'Schedule / approved',exact:true}).waitFor();
   await page.locator('#bounded-consent').check();await panel.getByRole('button',{name:'Start bounded schedule',exact:true}).click();await panel.getByRole('heading',{name:'Schedule / active',exact:true}).waitFor();await page.locator('#title').click();
   phase='first_cycle_'+i;await panel.getByRole('heading',{name:'Cycle 1 / awaiting_review',exact:true}).waitFor();
   await panel.getByRole('button',{name:'Pause schedule',exact:true}).click();await panel.getByRole('heading',{name:'Schedule / paused',exact:true}).waitFor();
   await page.reload({waitUntil:'domcontentloaded'});await panel.getByRole('heading',{name:'Schedule / paused',exact:true}).waitFor();
   phase='memory_review_'+i;await panel.getByRole('button',{name:'Inspect cycle 1 evidence',exact:true}).click();await panel.getByRole('heading',{name:'Genuine deterministic result',exact:true}).waitFor();await panel.getByLabel(/Retain this hypothesis privately when accepting/).check();await panel.getByRole('button',{name:'Accept reviewed cycle',exact:true}).click();await panel.getByRole('heading',{name:'Approved private memory',exact:true}).waitFor();
   r=(await read(page,endpoint)).value.run;const memory=r.cycles[0].approved_memory;assert(memory);assert.equal(r.cycles[0].state,'accepted');
   phase='resume_'+i;await page.locator('#bounded-consent').check();await panel.getByRole('button',{name:'Resume within original limits',exact:true}).click();await panel.getByRole('heading',{name:'Schedule / active',exact:true}).waitFor();await page.locator('#title').click();
   await panel.getByRole('heading',{name:'Cycle 2 / awaiting_review',exact:true}).waitFor({timeout:45000});
   await panel.getByRole('button',{name:'Stop schedule',exact:true}).click();await panel.getByRole('heading',{name:'Schedule / stopped',exact:true}).waitFor();
   r=(await read(page,endpoint)).value.run;assert.equal(r.cycles_started,2);assert.equal(r.actions_reserved,6);assert.equal(r.spending_microusd,0);
   const task=(await read(page,'/api/factory/tasks/'+r.cycles[1].task_id)).value;assert.equal(task.state,'completed');assert(task.payloads[0].retrieved_task_memories.some(m=>m.id===memory.id),'approved_memory_not_retrieved');assert.equal(task.payloads[2].memory,null);
   await page.reload({waitUntil:'domcontentloaded'});await panel.getByRole('heading',{name:'Schedule / stopped',exact:true}).waitFor();await panel.getByRole('heading',{name:'Approved private memory',exact:true}).waitFor();
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'horizontal_overflow');
   await panel.screenshot({path:resolve(out,(i?'mobile':'desktop')+'.png')});
   report.contexts.push({viewport,agent_id:agent,state:'stopped',cycles_started:r.cycles_started,actions_reserved:r.actions_reserved,scope_sha256:r.scope_sha256,approved_memory_id:memory.id,task_id:r.cycles[1].task_id,approved_memory_retrieved:true,refresh_persistence:true,overflow:false});
  }finally{await context.close();}
 }
 assert.equal(report.external_requests,0);assert.deepEqual(report.page_errors,[]);report.passed=true;
}catch(e){report.failure={phase,category:e.name,message:e.message.replace(/[0-9a-f]{64}/g,'[hash]').slice(0,250)};throw e;}
finally{
 await browser?.close();const closed=new Promise(r=>child.once('close',r));if(child.exitCode===null){try{process.kill(-child.pid,'SIGTERM');}catch{}}let timer;await Promise.race([closed,new Promise(r=>{timer=setTimeout(r,5000);})]);clearTimeout(timer);if(child.exitCode===null){try{process.kill(-child.pid,'SIGKILL');}catch{}await closed;}
 await writeFile(nextEnv,originalTypes);report.elapsed_ms=Date.now()-started;report.fixture_closed=true;await writeFile(resolve(out,'browser-result.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});await writeFile(resolve(out,'next.log'),log.join('\n'),{mode:0o600});console.log(JSON.stringify({passed:report.passed,contexts:report.contexts.length,elapsed_ms:report.elapsed_ms,external_requests:report.external_requests,failure:report.failure}));
}
