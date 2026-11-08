#!/usr/bin/env node
// Live read-only browser acceptance; no deployment, credential mutation or database writes.

const fs=require('node:fs');
const {createObserver,ProbeFailure}=require('./railway-browser-observation.cjs');
const assert=require('node:assert/strict');
const {chromium}=require(process.argv[2]);
const {execFileSync}=require('node:child_process');
const origin='https://supaagency.xyz';
const report={passed:false,contexts:[],private_api_requests:0,mutations:0,page_errors:[]};
let browser,stage='browser_start',subtest='startup',observedStatus;
const observations=[];
const observer=createObserver({record:row=>{observations.push(row);if(observations.length>300)observations.shift();fs.writeFileSync(process.argv[4]+'/browser-observations.json',JSON.stringify(observations));}});
const roleAuth={};
function loadAuth(){
const query='query($p:String!,$e:String!,$s:String!,$a:String!){web:variables(projectId:$p,environmentId:$e,serviceId:$s) api:variables(projectId:$p,environmentId:$e,serviceId:$a)}';
const payload=JSON.parse(execFileSync('/home/pikoo/.nvm/versions/node/v24.22.1/bin/railway',['api',query,'--compact','--variables','@-'],{input:JSON.stringify({p:'99648d2a-69a1-4d8c-98d0-7f2a26543c9c',e:'ff0b2092-e42b-4a89-ae25-441ab95d92d8',s:'955bb925-6343-433e-b9dd-f7cbdd89114a',a:'9996e351-d7a5-482e-9f14-5f863eb9b45f'}),encoding:'utf8',timeout:30000,stdio:['pipe','pipe','pipe']}));
if(payload.errors)throw Error('private_auth_observation_unavailable');
const accounts=JSON.parse(payload.data.web.SUPA_WEB_OPERATORS),credentials=JSON.parse(payload.data.api.SUPA_OPERATOR_CREDENTIALS);
const deadlines=JSON.parse(payload.data.api.SUPA_OPERATOR_SESSION_DEADLINES);
assert.ok(credentials.every(c=>deadlines[c.id]>Date.now()/1000),'valid_sessions_required');
for(const c of credentials){const account=accounts.find(x=>x.api_token===c.token);assert.ok(account);roleAuth[c.role]='Basic '+Buffer.from(account.username+':'+account.password).toString('base64');}
}
const fixtureAuth=role=>roleAuth[role];
const status=(path,headers={},expected=401,operation='protected_route')=>observer.status(path,headers,expected,operation);
(async()=>{
 try{
  loadAuth();
  assert.equal(await status('/healthz',{},200,'readiness'),200,'health_deadline');
  browser=await chromium.launch({headless:true,executablePath:'/usr/bin/google-chrome'});
  for(const spec of [{name:'desktop',viewport:{width:1440,height:1000}},{name:'mobile',viewport:{width:390,height:844},isMobile:true,hasTouch:true}]){
   stage=spec.name;
   const context=await browser.newContext({...spec,reducedMotion:'reduce',serviceWorkers:'block'});
   await context.route('**/*',route=>{const q=route.request(),u=new URL(q.url());if(!['GET','HEAD'].includes(q.method())){report.mutations++;return route.abort();}if(u.pathname.startsWith('/api/')){report.private_api_requests++;return route.abort();}if(u.origin!==origin)return route.abort();return route.continue();});
   const page=await context.newPage();page.on('pageerror',e=>report.page_errors.push(e.name));
   subtest='public_navigation';
   assert.equal((await page.goto(origin)).status(),200,'public_landing');await page.locator('html[data-ready=true]').waitFor();
   assert.equal(await page.locator('.enter').getAttribute('href'),'/home');await page.locator('.enter').click();await page.waitForURL('**/home');
   assert.match(await page.locator('.buffer').innerText(),/Supa Agent/);
   await page.getByRole('button',{name:'Create Agent →',exact:true}).click();await page.waitForURL('**/factory');
   await page.getByRole('button',{name:'Discover public agents →',exact:true}).click();await page.waitForURL('**/agents');
   await page.getByRole('button',{name:'Create Agent →',exact:true}).click();await page.waitForURL('**/factory');
   assert.equal(await status('/my-agents',{},401,'anonymous_management'),401,'anonymous_management_denied');
   assert.equal(await status('/my-agents',{'x-supa-public-landing':'1','x-supa-experience':'1'},401,'forged_management'),401,'forged_management_denied');
   assert.equal((await page.goto(origin+'/token-launch')).status(),200);await page.waitForURL('**/factory');
   await page.goto(origin+'/home');await page.locator('html[data-ready=true]').waitFor();
   await page.screenshot({path:process.argv[4]+'/'+spec.name+'-home.png'});
   const go=async label=>{if(spec.isMobile)await page.locator('.tabline').click();await page.getByRole('option',{name:label,exact:true}).click();};
   const sections=[['Agents','agents',/No public agent profiles are available yet/],['Intelligence','intelligence',/No creator-approved reasoning reports/],['Memory','memory',/No approved public memory summaries/],['Economy','economy',/No verified public economic reports/],['Social','social',/No approved public activity/],['Trading · planned','trading',/No public trades/],['Factory · planned','factory',/Factory \/ Create Agents/],['About','about',/planned, not live/]];
   for(const [label,path,empty] of sections){
    await go(label);await page.waitForURL('**/'+path);assert.match(await page.locator('.buffer').innerText(),empty);
    if(path!=='about'){
     const controls=page.locator('.buffer button');assert.ok(await controls.count()>0,'visible_action_state');
     for(const control of await controls.all())if(!await control.isDisabled()){
      assert.ok(['Create Agent →','Discover public agents →'].includes(await control.innerText()),'only_read_only_navigation_enabled');
     }
     if(path==='factory'){
      assert.equal(await page.locator('.buffer button:disabled').count(),4,'factory_actions_disabled');
      assert.deepEqual(await page.locator('.terminal-heading').allTextContents(),['Factory / Create Agents','1. Create Agent','2. Configure','3. Purpose Qualification','4. Agent Token / Optional','5. My Agents']);
      assert.match(await page.locator('.buffer').innerText(),/Skip this step and keep your agent token-free/);
      assert.equal(await page.getByRole('link',{name:'My Agents / sign-in required'}).getAttribute('href'),'/my-agents');
     }else if(path!=='agents')for(const control of await controls.all())assert.equal(await control.isDisabled(),true,'unfinished_action_disabled');
    }
    assert.equal(await page.locator('.buffer form,.buffer input,.buffer select').count(),0,'no_management_forms');
    await page.screenshot({path:process.argv[4]+'/'+spec.name+'-'+path+'.png'});
    assert.equal((await page.goto(origin+'/'+path)).status(),200,'public_deep_link');await page.locator('html[data-ready=true]').waitFor();assert.match(await page.locator('.buffer').innerText(),empty);
   }
   assert.equal(await page.getByRole('link',{name:'Official X / @supa_agency'}).getAttribute('href'),'https://x.com/supa_agency');
   subtest='github_alerts';
   for(const button of [page.getByRole('button',{name:'github',exact:true}),page.getByRole('button',{name:'Source / github.com',exact:true})]){
    const before=page.url(),shown=page.waitForEvent('dialog'),clicked=button.click();const dialog=await shown;
    assert.equal(dialog.type(),'alert');assert.equal(dialog.message(),'GitHub account private, will be open sourced soon.');await dialog.accept();await clicked;assert.equal(page.url(),before);
   }
   assert.equal(await page.locator('a[href*="github.com"]').count(),0);
   subtest='caret';
   const caret=await page.locator('.buffer > :last-child').evaluate(el=>{const c=getComputedStyle(el,'::after');return {width:c.width,content:c.content,animation:c.animationName};});
   assert.equal(caret.width,'1px');assert.notEqual(caret.content,'none');assert.equal(caret.animation,'none');
   subtest='history';
   await go('Agents');await page.waitForURL('**/agents');await go('About');await page.waitForURL('**/about');
   await page.goBack();await page.waitForURL('**/agents');await page.goForward();await page.waitForURL('**/about');
   if(spec.isMobile){await page.locator('.tabline').click();assert.equal(await page.locator('.pane').evaluate(e=>e.inert),true);await page.keyboard.press('Escape');assert.equal(await page.locator('.tabline').getAttribute('aria-expanded'),'false');}
   else{await page.locator('.wheel').focus();await page.keyboard.press('ArrowUp');await page.waitForURL('**/factory');await page.keyboard.press('ArrowDown');await page.waitForURL('**/about');}
   await page.emulateMedia({reducedMotion:'no-preference'});await page.waitForFunction(()=>document.documentElement.dataset.motion==='running');await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(()=>document.fonts.ready);
   const layout=await page.evaluate(()=>({width:innerWidth,scrollWidth:document.documentElement.scrollWidth,fonts:document.fonts.status,logo:[...document.querySelectorAll('.brand img')].every(e=>e.complete&&e.naturalWidth>0)}));
   assert.ok(layout.scrollWidth<=layout.width+2&&layout.logo&&layout.fonts==='loaded');assert.equal(await page.locator('.wheel-item').count(),9);
   assert.deepEqual(await page.locator('.wheel-name').allTextContents(),['Home','Agents','Intelligence','Memory','Economy','Social','Trading','Factory','About']);
   // A separate authenticated context loads only the inactive owner-area preview.
   for(const role of ['viewer','operator']){
    subtest='my_agents_'+role;
    const managed=await browser.newContext({...spec,reducedMotion:'reduce',serviceWorkers:'block',extraHTTPHeaders:{authorization:fixtureAuth(role)}});
    await managed.route('**/*',route=>{const q=route.request(),u=new URL(q.url());if(q.method()!=='GET'||u.origin!==origin||u.pathname.startsWith('/api/')){report.mutations+=q.method()!=='GET'?1:0;report.private_api_requests+=u.pathname.startsWith('/api/')?1:0;return route.abort();}return route.continue();});
    const owned=await managed.newPage();owned.on('pageerror',e=>report.page_errors.push(e.name));
    observedStatus=await status('/my-agents',{authorization:fixtureAuth(role)},200,'authenticated_management');assert.equal(observedStatus,200,'authenticated_management_preview');
    assert.equal((await owned.goto(origin+'/my-agents')).status(),200,'authenticated_management_preview');
    assert.match(await owned.locator('main').innerText(),/owner management Coming Soon/);
    assert.equal(await owned.locator('button:disabled').count(),4);assert.equal(await owned.locator('button:not(:disabled),form,input,script').count(),0);
    await owned.evaluate(()=>document.fonts.ready);
    const type=await owned.evaluate(()=>{const body=getComputedStyle(document.querySelector('.terminal-line')),h1=getComputedStyle(document.querySelector('h1')),h2=getComputedStyle(document.querySelector('h2'));return {leading:parseFloat(body.lineHeight)/parseFloat(body.fontSize),step:parseFloat(h1.fontSize)/parseFloat(h2.fontSize),bodyStep:parseFloat(h2.fontSize)/parseFloat(body.fontSize),overflow:document.documentElement.scrollWidth>innerWidth};});
    assert.ok(type.leading>=1.5&&type.step>=1.25&&type.bodyStep>=1.25&&!type.overflow,'management_typography');
    await owned.screenshot({path:process.argv[4]+'/'+spec.name+'-my-agents-'+role+'.png'});await managed.close();
   }
   report.contexts.push({name:spec.name,passed:true,fresh_anonymous_context:true,enter_without_password:true,home_agents_about:true,factory_flow:true,my_agents_authentication:true,all_nine_public_sections:true,github_notice:true,completed_text_caret:true,history_and_keyboard:true,...layout});await context.close();
  }
  stage='protected_routes';subtest='protected_routes';
  report.protected_route_acceptance=await observer.protectedRoutes();
  assert.equal(report.private_api_requests,0);assert.equal(report.mutations,0);assert.deepEqual(report.page_errors,[]);assert.equal(observer.acceptance?.passed,true,'protected_acceptance_required');report.passed=true;
 }catch(e){
  report.failure_stage=stage;report.failure_subtest=subtest;report.error_type=e.name;
  const first=String(e.message||'').split('\n')[0];
  report.assertion_id=['authenticated_management_preview','management_typography','factory_actions_disabled','only_read_only_navigation_enabled','no_management_forms','public_deep_link'].includes(first)?first:'browser_assertion_failed';
  if(e instanceof ProbeFailure){report.observation_failure=observations.at(-1);report.failure_code=e.code;}
  process.exitCode=1;
 }
 finally{await browser?.close().catch(()=>{});console.log(JSON.stringify(report));}
})();
