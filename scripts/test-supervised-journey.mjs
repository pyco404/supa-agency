/** Actual browser -> Next -> Rust -> isolated PG18. Synthetic loopback only. */
import {spawn} from 'node:child_process';
import {randomBytes} from 'node:crypto';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import assert from 'node:assert/strict';
import {chromium} from '../web/node_modules/playwright/index.mjs';
const root=resolve(import.meta.dirname,'..'),origin='http://127.0.0.1:3338',out=resolve(root,'artifacts/supervised');
assert(process.env.SUPA_FACTORY_BROWSER_TOKEN?.startsWith('factory-synthetic-'));assert(process.env.SUPA_FACTORY_OTHER_TOKEN?.startsWith('factory-synthetic-'));
const password=randomBytes(32).toString('hex'),accounts=[{username:'owner',password,api_token:process.env.SUPA_FACTORY_BROWSER_TOKEN},{username:'other',password,api_token:process.env.SUPA_FACTORY_OTHER_TOKEN}];
await mkdir(out,{recursive:true,mode:0o700});
const env={PATH:process.env.PATH,HOME:process.env.HOME,NODE_ENV:'development',SUPA_SITE_MODE:'staging',SUPA_SYNTHETIC_FACTORY_ENABLED:'true',SUPA_SUPERVISED_TASKS_ENABLED:'true',SUPA_SUPERVISED_REASONING_ENABLED:'true',SUPA_PUBLIC_LANDING_ENABLED:'true',SUPA_SITE_ORIGIN:origin,SUPA_BACKEND_URL:'http://127.0.0.1:3352',SUPA_WEB_OPERATORS:JSON.stringify(accounts)};
const child=spawn(process.execPath,['node_modules/next/dist/bin/next','dev','--webpack','--hostname','127.0.0.1','--port','3338'],{cwd:resolve(root,'web'),env,stdio:'ignore',detached:true});
let browser;const report={passed:false,contexts:[],external_requests:0,synthetic_only:true};let operation='startup';
try{
 const deadline=Date.now()+90000;let ready=false;while(Date.now()<deadline){try{const r=await fetch(origin+'/healthz',{signal:AbortSignal.timeout(1000)});await r.arrayBuffer();if(r.ok){ready=true;break;}}catch{}await new Promise(r=>setTimeout(r,400));}assert(ready);
 browser=await chromium.launch({executablePath:'/usr/bin/google-chrome',headless:true});
 for(const [i,viewport]of [[0,{width:1440,height:1000}],[1,{width:390,height:844}]]){
  const context=await browser.newContext({viewport,isMobile:!!i,hasTouch:!!i,serviceWorkers:'block',reducedMotion:'reduce'});await context.route('**/*',route=>{if(new URL(route.request().url()).origin!==origin){report.external_requests++;return route.abort();}return route.continue();});const page=await context.newPage();page.setDefaultTimeout(30000);
  assert.equal((await context.request.get(origin+'/api/factory/tasks/11111111-1111-4111-8111-111111111111')).status(),401);
  operation='signin_'+i;await page.goto(origin+'/sign-in');await page.getByLabel('Synthetic account').fill(accounts[i].username);await page.getByLabel('Password',{exact:true}).fill(password);await page.getByRole('button',{name:'Sign In',exact:true}).click();await page.waitForURL(origin+'/factory/create');
  const name='Synthetic supervised '+(i?'mobile':'desktop');await page.getByLabel('Name',{exact:true}).fill(name);await page.getByLabel('Description',{exact:true}).fill('Disposable supervised browser task');await page.getByLabel('Purpose',{exact:true}).fill('Summarize synthetic reports');await page.getByLabel('Personality / voice').fill('Curious and concise');await page.getByLabel('Goals / one per line, up to eight').fill('Review one synthetic report');await page.getByLabel('Read own memory').check();await page.getByLabel('Retain lessons',{exact:true}).check();await page.getByLabel('I consent to privately storing').check();await page.getByRole('button',{name:'Review configuration',exact:true}).click();await page.getByRole('button',{name:'Create private paused agent',exact:true}).click();await page.waitForURL(origin+'/my-agents');
  const own=page.locator('article').filter({has:page.getByRole('heading',{name,exact:true})});await own.getByRole('link',{name:'View details / configuration',exact:true}).click();await page.waitForURL(/\/my-agents\/[0-9a-f-]{36}$/);const agent=page.url().split('/').pop();
  operation='submit_task_'+i;
  const taskPath=origin+'/api/factory/agents/'+agent+'/tasks',submissions=[];let loseSubmission=true;
  await page.route(taskPath,async route=>{if(route.request().method()!=='POST')return route.continue();submissions.push(route.request().postDataJSON());if(loseSubmission){loseSubmission=false;const response=await route.fetch();assert.equal(response.status(),200);await response.dispose();return route.abort('failed');}return route.continue();});
  await page.getByLabel('Task',{exact:true}).fill('Review synthetic task evidence');await page.getByLabel('Desired evidence / exact phrase').fill('reviewed');await page.getByLabel('Synthetic material',{exact:true}).fill('Synthetic report reviewed. Two sample checks passed.\n<script>window.__supaTaskInjected=true</script>');await page.getByLabel('This is disposable synthetic data').check();await page.getByLabel('Retain an evidence-linked private task memory').check();await page.getByRole('button',{name:'Submit task for approval',exact:true}).click();
  await page.getByRole('button',{name:'Retry exact task submission',exact:true}).waitFor();
  assert.equal(await page.getByLabel('Task',{exact:true}).isDisabled(),true);assert.equal(await page.locator('#task-form').getAttribute('data-submission-state'),'unconfirmed');
  await page.screenshot({path:resolve(out,(i?'mobile':'desktop')+'-unconfirmed-submission.png'),fullPage:true});
  // Even a programmatic field change must not alter an unconfirmed request.
  await page.locator('#task-objective').evaluate(input=>{input.value='Changed after ambiguous transport';});
  await page.getByRole('button',{name:'Retry exact task submission',exact:true}).click();
  let task=page.locator('#task-list article');await task.getByRole('heading',{name:'Review synthetic task evidence',exact:true}).waitFor();
  assert.equal(submissions.length,2);assert.deepEqual(submissions[0],submissions[1]);assert.equal(await page.getByLabel('Task',{exact:true}).isDisabled(),false);await page.unroute(taskPath);
  // A confirmed write followed by a failed observation is saved, not unknown.
  let loseRefresh=true;await page.route(taskPath,route=>{if(route.request().method()==='GET'&&loseRefresh){loseRefresh=false;return route.abort('failed');}return route.continue();});
  await task.getByLabel('I approve this exact synthetic task').check();await task.getByRole('button',{name:'Approve execution',exact:true}).click();
  await page.getByText('Task action saved, but the refreshed state is unavailable.',{exact:false}).waitFor();
  assert.equal(await task.getByRole('button',{name:'Approve execution',exact:true}).isDisabled(),true);
  await page.unroute(taskPath);await page.getByRole('button',{name:'Refresh tasks',exact:true}).click();await task.getByText('Status: approved',{exact:false}).waitFor();
  let releaseOld;const oldReady=new Promise(resolve=>{releaseOld=resolve;});let releaseObservation;const observationGate=new Promise(resolve=>{releaseObservation=resolve;});let holdObservation=true;
  await page.route(taskPath,async route=>{if(route.request().method()==='GET'&&holdObservation){holdObservation=false;const old=await route.fetch();releaseOld();await observationGate;return route.fulfill({response:old});}return route.continue();});
  await page.getByRole('button',{name:'Refresh tasks',exact:true}).click();await oldReady;
  await task.getByLabel('I approve this exact synthetic task').check();await task.getByRole('button',{name:'Start approved task',exact:true}).click();await task.getByText('Status: running',{exact:false}).waitFor();
  const oldDelivered=page.waitForResponse(taskPath);releaseObservation();await (await oldDelivered).finished();await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  assert((await task.innerText()).includes('Status: running'));await page.unroute(taskPath);
  await task.getByRole('button',{name:'Run next step',exact:true}).click();await task.getByText(/step 1 of 3/).waitFor();await task.getByRole('button',{name:'Pause task',exact:true}).click();await task.getByText('Status: paused',{exact:false}).waitFor();assert.equal(await task.getByRole('button',{name:'Run next step',exact:true}).count(),0);
  await page.reload();task=page.locator('#task-list article');await task.getByText('Status: paused',{exact:false}).waitFor();await task.getByLabel('I approve this exact synthetic task').check();await task.getByRole('button',{name:'Resume task',exact:true}).click();await task.getByText('Status: running',{exact:false}).waitFor();
  for(const step of [2,3]){await task.getByRole('button',{name:'Run next step',exact:true}).click();await task.getByText(new RegExp('step '+step+' of 3')).waitFor();}
  operation='result_'+i;await task.getByText('Status: completed',{exact:false}).waitFor();assert.equal(await task.getByRole('button').count(),0);assert((await task.innerText()).includes('Hypothesis /'));assert((await task.innerText()).includes('Synthetic report reviewed. Two sample checks passed.'));assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);assert.equal(await page.evaluate(()=>window.__supaTaskInjected),undefined);await page.screenshot({path:resolve(out,(i?'mobile':'desktop')+'-completed.png'),fullPage:true});await task.screenshot({path:resolve(out,(i?'mobile':'desktop')+'-task-result.png')});
  const list=await(await context.request.get(origin+'/api/factory/agents/'+agent+'/tasks')).json();assert.equal(list.tasks.length,1);assert.equal(list.tasks[0].state,'completed');assert.equal(list.tasks[0].payloads[2].spending_microusd,0);
  assert.equal((await context.request.post(origin+'/api/factory/tasks/'+list.tasks[0].id+'/command',{headers:{Origin:'https://untrusted.invalid','Content-Type':'application/json'},data:{}})).status(),403);
  operation='reasoning_task_'+i;
  await page.getByLabel('Task mode',{exact:true}).selectOption('reasoning');
  await page.getByLabel('Task',{exact:true}).fill('Review synthetic incident');await page.getByLabel('Desired evidence / exact phrase').fill('recovered');await page.getByLabel('Synthetic material',{exact:true}).fill('12:00 Synthetic requests timed out.\n12:05 Synthetic service recovered after restart.');await page.getByLabel('This is disposable synthetic data').check();await page.getByLabel('Retain an evidence-linked private task memory').check();await page.getByRole('button',{name:'Submit task for approval',exact:true}).click();
  const reasoning=page.locator('#task-list article').filter({has:page.getByRole('heading',{name:'Review synthetic incident',exact:true})});await reasoning.getByRole('heading',{name:'Bounded reasoning plan / mock model',exact:true}).waitFor();assert((await reasoning.innerText()).includes('synthetic-incident-v1'));
  for(const action of ['Approve execution','Start approved task']){await reasoning.getByLabel('I approve this exact synthetic task').check();await reasoning.getByRole('button',{name:action,exact:true}).click();await reasoning.getByText(action==='Approve execution'?'Status: approved':'Status: running',{exact:false}).waitFor();}
  await reasoning.getByRole('button',{name:'Run next step',exact:true}).click();await reasoning.getByText(/step 1 of 3/).waitFor();await reasoning.getByRole('button',{name:'Run next step',exact:true}).click();await reasoning.getByText('Status: awaiting_review',{exact:false}).waitFor();
  assert.equal(await reasoning.getByRole('button',{name:'Run next step',exact:true}).count(),0);assert.equal(await reasoning.getByRole('heading',{name:'Retained private memory',exact:true}).count(),0);assert((await reasoning.innerText()).includes('causation remains uncertain'));
  await page.reload();await reasoning.getByText('Status: awaiting_review',{exact:false}).waitFor();assert.equal(await reasoning.getByRole('button',{name:'Accept reviewed result and complete',exact:true}).isDisabled(),true);
  await reasoning.screenshot({path:resolve(out,(i?'mobile':'desktop')+'-reasoning-review.png')});
  await reasoning.getByLabel('I reviewed this model result and its uncertainty.').check();await reasoning.getByRole('button',{name:'Accept reviewed result and complete',exact:true}).click();await reasoning.getByText('Status: completed',{exact:false}).waitFor();await page.reload();await reasoning.getByText('Status: completed',{exact:false}).waitFor();assert((await reasoning.innerText()).includes('Hypothesis /'));assert((await reasoning.innerText()).includes('Owner accepted this proposal.'));assert(!(await reasoning.innerText()).includes('not yet owner-reviewed'));
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await reasoning.screenshot({path:resolve(out,(i?'mobile':'desktop')+'-reasoning-result.png')});
  const reasoningList=await(await context.request.get(origin+'/api/factory/agents/'+agent+'/tasks')).json(),reasoned=reasoningList.tasks.find(t=>t.input.reasoning);assert.equal(reasoned.state,'completed');assert.equal(reasoned.inference.state,'settled');assert.equal(reasoned.inference.actual_microusd,0);assert(reasoned.inference.usage.input_tokens<=2000&&reasoned.inference.usage.output_tokens<=256);assert.equal(reasoned.payloads[2].memory.source_result_sha256,reasoned.inference.result_sha256);
  operation='transport_stop_'+i;await page.getByLabel('Task mode',{exact:true}).selectOption('reasoning');await page.getByLabel('Task',{exact:true}).fill('Cancel unconfirmed reasoning request');await page.getByLabel('Desired evidence / exact phrase').fill('recovered');await page.getByLabel('Synthetic material',{exact:true}).fill('Synthetic service recovered.');await page.getByLabel('This is disposable synthetic data').check();await page.getByRole('button',{name:'Submit task for approval',exact:true}).click();
  const cancelled=page.locator('#task-list article').filter({has:page.getByRole('heading',{name:'Cancel unconfirmed reasoning request',exact:true})});await cancelled.getByText('Status: pending',{exact:false}).waitFor();for(const action of ['Approve execution','Start approved task']){await cancelled.getByLabel('I approve this exact synthetic task').check();await cancelled.getByRole('button',{name:action,exact:true}).click();await cancelled.getByText(action==='Approve execution'?'Status: approved':'Status: running',{exact:false}).waitFor();}await cancelled.getByRole('button',{name:'Run next step',exact:true}).click();await cancelled.getByText(/step 1 of 3/).waitFor();
  let loseAdvance=true,stopObserved=false;const commandRoute='**/api/factory/tasks/*/command';await page.route(commandRoute,async route=>{const data=route.request().postDataJSON();if(data.operation==='advance'&&loseAdvance){loseAdvance=false;return route.abort('failed');}if(data.operation==='stop')stopObserved=true;return route.continue();});
  await cancelled.getByRole('button',{name:'Run next step',exact:true}).click();await page.getByText(/Refresh tasks before another action/).waitFor();await page.getByRole('button',{name:'Refresh tasks',exact:true}).click();await cancelled.getByText('Status: running',{exact:false}).waitFor();await cancelled.getByRole('button',{name:'Stop task',exact:true}).click();await cancelled.getByText('Status: stopped',{exact:false}).waitFor();assert(stopObserved);await page.unroute(commandRoute);
  const cancelledRows=await(await context.request.get(origin+'/api/factory/agents/'+agent+'/tasks')).json(),cancelledTask=cancelledRows.tasks.find(t=>t.input.task==='Cancel unconfirmed reasoning request');assert.equal(cancelledTask.state,'stopped');assert.equal(cancelledTask.inference,null,'Stop must never replay an unconfirmed model request');assert.equal(cancelledTask.payloads.length,1);
  await page.getByRole('button',{name:'Sign out',exact:true}).click();await page.waitForURL(origin+'/sign-in');assert.equal((await context.request.get(origin+'/api/factory/agents/'+agent+'/tasks')).status(),401);report.contexts.push({viewport:i?'mobile':'desktop',created_agent:true,approved_task:true,pause_resume_refresh:true,completed:true,private_memory:true,zero_spend:true,reasoning_owner_review:true,reasoning_persisted:true,stops_after_transport_error:true});await context.close();
 }
 // Inspect durable simulated real-provider receipts through the actual private UI.
 // These records were produced by Rust Probe fixtures, never a paid model call.
 for(const [label,viewport]of [['desktop',{width:1440,height:1000}],['mobile',{width:390,height:844}]]){
  operation='real_receipt_ui_'+label;
  const context=await browser.newContext({viewport,serviceWorkers:'block',reducedMotion:'reduce'});
  await context.route('**/*',route=>{if(new URL(route.request().url()).origin!==origin){report.external_requests++;return route.abort();}return route.continue();});
  const page=await context.newPage();await page.goto(origin+'/sign-in');await page.getByLabel('Synthetic account').fill('owner');await page.getByLabel('Password',{exact:true}).fill(password);await page.getByRole('button',{name:'Sign In',exact:true}).click();await page.waitForURL(origin+'/factory/create');
  const agents=await(await context.request.get(origin+'/api/factory/agents')).json();
  const agent=agents.agents.find(a=>a.configuration.request_id==='11111111-1111-4111-8000-000000000fa0');assert(agent);
  await page.goto(origin+'/my-agents/'+agent.agent_id);
  const completed=page.locator('#task-list article').filter({has:page.getByText('Status: completed',{exact:false})});
  await completed.getByRole('heading',{name:'Bounded reasoning plan / real AI model',exact:true}).waitFor();
  assert((await completed.innerText()).includes('gpt-6-luna'));assert((await completed.innerText()).includes('$0.000378'));
  assert((await completed.innerText()).includes('$0.000080 observed cost'));assert((await completed.innerText()).includes('Received a real AI proposal'));
  assert((await page.locator('#task-list').innerText()).includes('cost unknown; full reservation held'));
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await completed.screenshot({path:resolve(out,label+'-simulated-real-receipt.png')});
  report.contexts.push({viewport:label,simulated_real_receipt_ui:true,unknown_cost_visible:true,paid_requests:0});await context.close();
 }
 assert.equal(report.external_requests,0);report.passed=true;
}catch(e){report.error_type=e.name;report.operation=operation;report.assertion=String(e.message).split('\n')[0].slice(0,180);}
finally{await browser?.close();try{process.kill(-child.pid,'SIGTERM');}catch{}await writeFile(resolve(out,'browser-result.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify(report));if(!report.passed)process.exitCode=1;}
