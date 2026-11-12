import {test} from 'node:test';
import assert from 'node:assert/strict';
import {randomBytes} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import ts from '../web/node_modules/typescript/lib/typescript.js';
const auth=new URL('../web/src/lib/auth.ts',import.meta.url).href;
const source=await readFile(new URL('../web/src/lib/factory-session.ts',import.meta.url),'utf8');
const moduleUrl='data:text/javascript,'+encodeURIComponent(ts.transpileModule(source.replace('"./auth"',JSON.stringify(auth)),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText);
const session=await import(moduleUrl);
const taskSource=await readFile(new URL('../web/src/lib/supervised.ts',import.meta.url),'utf8');
const taskZod=new URL('../web/node_modules/zod/index.js',import.meta.url).href;
const taskModuleUrl='data:text/javascript,'+encodeURIComponent(ts.transpileModule(taskSource.replace("'zod'",JSON.stringify(taskZod)).replace("'./factory-session'",JSON.stringify(moduleUrl)).replace("'./auth'",JSON.stringify(auth)),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText);

const nextServer=new URL('../web/node_modules/next/server.js',import.meta.url).href;
const {NextRequest}=await import(nextServer);
const proxySource=await readFile(new URL('../web/src/proxy.ts',import.meta.url),'utf8');
const {proxy}=await import('data:text/javascript,'+encodeURIComponent(ts.transpileModule(proxySource.replace('"next/server"',JSON.stringify(nextServer)).replace('"./lib/auth"',JSON.stringify(auth)).replace('"./lib/factory-session"',JSON.stringify(moduleUrl)),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText));
const password=randomBytes(32).toString('hex'),api_token=randomBytes(32).toString('hex');
Object.assign(process.env,{SUPA_SITE_MODE:'staging',SUPA_SYNTHETIC_FACTORY_ENABLED:'true',SUPA_SITE_ORIGIN:'http://127.0.0.1:3339',SUPA_PUBLIC_LANDING_ENABLED:'true',SUPA_WEB_OPERATORS:JSON.stringify([{username:'synthetic',password,api_token}])});
const now=1000000;
function req(value,headers={}){return new Request('http://127.0.0.1:3339/api/factory/agents',{headers:{cookie:`supa_factory_session=${value}`,...headers}});}
test('signed bounded session does not contain account credentials and expires exclusively',()=>{
 const c=session.issueFactorySession('synthetic',password,now);assert(c&&!c.includes(password)&&!c.includes(api_token));
 assert(session.factoryAuthorization(req(c),now));assert.equal(session.factoryAuthorization(req(c),now+900000),null);
 assert.equal(session.factoryAuthorization(req(c),now-1000),null);assert.equal(session.issueFactorySession('synthetic','wrong',now),null);
});
test('forged, duplicate and malformed cookies never confer authorization',()=>{
 const c=session.issueFactorySession('synthetic',password,now);
 for(const value of ['invalid',c+'x',c.replace(/^./,c[0]==='a'?'b':'a'),'a.b'])assert.equal(session.factoryAuthorization(req(value),now),null);
 assert.equal(session.factoryAuthorization(req(c,{cookie:`supa_factory_session=${c}; supa_factory_session=${c}`}),now),null);
});
test('credential revocation and default-off flag invalidate sessions',()=>{
 const c=session.issueFactorySession('synthetic',password,now);process.env.SUPA_WEB_OPERATORS='[]';assert.equal(session.factoryAuthorization(req(c),now),null);
 process.env.SUPA_WEB_OPERATORS=JSON.stringify([{username:'synthetic',password,api_token}]);process.env.SUPA_SYNTHETIC_FACTORY_ENABLED='false';assert.equal(session.factoryAuthorization(req(c),now),null);assert.equal(session.issueFactorySession('synthetic',password,now),null);process.env.SUPA_SYNTHETIC_FACTORY_ENABLED='true';
});
test('same-origin writes require exact origin and host; public origin is not arbitrary',()=>{
 for(const [host,origin,yes]of [['127.0.0.1:3339','http://127.0.0.1:3339',true],['supaagency.xyz','https://supaagency.xyz',true],['127.0.0.1:3339','https://untrusted.invalid',false],['untrusted.invalid','https://untrusted.invalid',false],['supaagency.xyz','http://supaagency.xyz',false]])assert.equal(session.factorySameOrigin(new Request('http://127.0.0.1/',{headers:{host,origin}})),yes);
 assert.equal(session.factorySameOrigin(new Request('http://127.0.0.1/')),false);
});
test('sign-in assets are readable without exposing data, and cookie scope excludes operator APIs',()=>{
 const cookie=session.issueFactorySession('synthetic',password);
 const check=(path,headers={},method='GET')=>proxy(new NextRequest('http://127.0.0.1:3339'+path,{method,headers:{host:'127.0.0.1:3339',...headers}})).status;
 for(const path of ['/sign-in','/style.css','/public/fonts/IBMPlexMono-Regular.ttf','/public/assets/supa-agent-code.svg'])assert.equal(check(path),200,path);
 for(const path of ['/api/agents','/operator','/my-agents','/factory/create','/api/factory/agents','/public/../private','/style.css/private'])assert.equal(check(path,{'x-supa-public-landing':'1'}),401,path);
 assert.equal(check('/my-agents',{cookie:`supa_factory_session=${cookie}`}),200);
 const ownId='11111111-1111-4111-8111-111111111111';
 for(const path of [`/my-agents/${ownId}`,`/api/factory/agents/${ownId}`]){
  assert.equal(check(path),401,'details require authentication even without avatars');
  assert.equal(check(path,{cookie:`supa_factory_session=${cookie}`}),200,'avatar feature is not a detail-access dependency');
 }
 assert.equal(check('/operator',{cookie:`supa_factory_session=${cookie}`}),401);
 assert.equal(check('/api/agents',{cookie:`supa_factory_session=${cookie}`}),401);
 assert.equal(check('/api/factory/session',{origin:'https://untrusted.invalid'},'POST'),403);
 assert.equal(check('/style.css',{},'POST'),401);
 process.env.SUPA_SYNTHETIC_FACTORY_ENABLED='false';assert.equal(check('/sign-in'),401);process.env.SUPA_SYNTHETIC_FACTORY_ENABLED='true';
});
test('avatar routes keep authentication, host, origin and disabled-feature boundaries',()=>{
 process.env.SUPA_AGENT_AVATARS_ENABLED='true';
 const cookie=session.issueFactorySession('synthetic',password),id='11111111-1111-4111-8111-111111111111';
 const check=(path,host='127.0.0.1:3339',headers={},method='GET')=>proxy(new NextRequest('http://127.0.0.1:3339'+path,{method,headers:{host,...headers}})).status;
 for(const path of [`/my-agents/${id}`,`/api/factory/avatar-assets/${id}`,`/api/factory/avatar-view/${id}`]){assert.equal(check(path),401);assert.equal(check(path,undefined,{cookie:`supa_factory_session=${cookie}`}),200);}
 assert.equal(check(`/api/public/agent-avatars/${id}/1`,'supaagency.xyz'),200);
 assert.equal(check(`/api/public/agent-avatars/${id}/1`),401,'Railway staging remains authenticated');
 assert.equal(check('/api/factory/avatar-uploads',undefined,{cookie:`supa_factory_session=${cookie}`,origin:'https://untrusted.invalid'},'POST'),403);
 assert.equal(check(`/api/factory/avatar-assets/../../credentials`),401);
 process.env.SUPA_AGENT_AVATARS_ENABLED='false';assert.equal(check(`/api/public/agent-avatars/${id}/1`,'supaagency.xyz'),401);
});

test('avatar-free detail adapter forwards one validated ID and rejects writes or malformed contracts',async()=>{
 const stub='data:text/javascript,'+encodeURIComponent('export const avatarsEnabled=()=>false; export const avatarRoute=()=>{throw Error("unexpected_avatar_route")};');
 const src=await readFile(new URL('../web/src/lib/factory.ts',import.meta.url),'utf8');
 const zod=new URL('../web/node_modules/zod/index.js',import.meta.url).href;
 const {factoryRoute}=await import('data:text/javascript,'+encodeURIComponent(ts.transpileModule(src.replace('"./supervised"',JSON.stringify(taskModuleUrl)).replace('"zod"',JSON.stringify(zod)).replace('"./factory-session"',JSON.stringify(moduleUrl)).replace('"./auth"',JSON.stringify(auth)).replace('"./avatars"',JSON.stringify(stub)),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText));
 const id='11111111-1111-4111-8111-111111111111',path='/api/factory/agents/'+id;
 const configuration={request_id:id,name:'Synthetic assistant',description:'Disposable fixture',purpose:'Review synthetic reports',personality:{voice:'Curious',traits:[]},goals:['Review a synthetic report'],requested_capabilities:[],autonomy:'disabled',spending_limit_microusd:0,private_consent:true};
 const agent={agent_id:id,configuration,status:'paused',qualification:'pending_review',public:false,enabled_capabilities:[],execution_enabled:false,token_mint:null};
 const originalFetch=globalThis.fetch,originalBackend=process.env.SUPA_BACKEND_URL;
 const calls=[];process.env.SUPA_BACKEND_URL='http://127.0.0.1:3351';
 const cookie=session.issueFactorySession('synthetic',password);
 const request=(method='GET')=>new Request('http://127.0.0.1:3339'+path,{method,headers:{host:'127.0.0.1:3339',origin:'http://127.0.0.1:3339','content-type':'application/json',cookie:`supa_factory_session=${cookie}`}});
 try{
  globalThis.fetch=async(url,options)=>{calls.push({path:new URL(url).pathname,method:options.method});return Response.json(agent);};
  assert.deepEqual(await (await factoryRoute(request(),path)).json(),agent);
  assert.deepEqual(calls,[{path:'/factory/agents/'+id,method:'GET'}]);
  assert.equal((await factoryRoute(request('POST'),path)).status,404);assert.equal(calls.length,1);
  assert.equal((await factoryRoute(request(),'/api/factory/agents/not-a-uuid')).status,404);assert.equal(calls.length,1);
  globalThis.fetch=async()=>Response.json({private_record:'must_not_escape'},{status:404});
  const unavailable=await factoryRoute(request(),path);assert.equal(unavailable.status,404);assert(!(await unavailable.text()).includes('must_not_escape'));
  globalThis.fetch=async()=>Response.json({...agent,execution_enabled:true});
  assert.equal((await factoryRoute(request(),path)).status,502,'invalid backend state fails closed');
 }finally{globalThis.fetch=originalFetch;if(originalBackend===undefined)delete process.env.SUPA_BACKEND_URL;else process.env.SUPA_BACKEND_URL=originalBackend;}
});

test('supervised adapter enforces feature, session, origin, exact command and backend contracts',async()=>{
 const {supervisedRoute}=await import(taskModuleUrl),id='11111111-1111-4111-8111-111111111111',path='/api/factory/tasks/'+id+'/command';
 const cookie=session.issueFactorySession('synthetic',password),oldFetch=globalThis.fetch,oldUrl=process.env.SUPA_BACKEND_URL;
 let calls=0;globalThis.fetch=async()=>{calls++;return Response.json({invalid_private_backend_shape:true});};process.env.SUPA_BACKEND_URL='http://127.0.0.1:3351';
 const command={request_id:id,operation:'start',expected_revision:0,input_sha256:'0'.repeat(64),confirmed:true};
 const make=(body=command,origin='http://127.0.0.1:3339',auth=true)=>new Request('http://127.0.0.1:3339'+path,{method:'POST',headers:{host:'127.0.0.1:3339',origin,'Content-Type':'application/json',...(auth?{cookie:'supa_factory_session='+cookie}:{})},body:JSON.stringify(body)});
 try{
  delete process.env.SUPA_SUPERVISED_TASKS_ENABLED;assert.equal((await supervisedRoute(make(),path)).status,403);
  process.env.SUPA_SUPERVISED_TASKS_ENABLED='true';assert.equal((await supervisedRoute(make(command,undefined,false),path)).status,401);
  assert.equal((await supervisedRoute(make(command,'https://untrusted.invalid'),path)).status,403);
  for(const extra of [{creator_id:id},{payload:{result:'forged'}},{operation:'trade'}])assert.equal((await supervisedRoute(make({...command,...extra}),path)).status,400);
  assert.equal(calls,0);assert.equal((await supervisedRoute(make(),path)).status,502);assert.equal(calls,1);
  assert.equal(proxy(new NextRequest('http://127.0.0.1:3339'+path,{headers:{host:'127.0.0.1:3339',cookie:'supa_factory_session='+cookie}})).status,200);
  assert.equal(proxy(new NextRequest('http://127.0.0.1:3339'+path,{headers:{host:'127.0.0.1:3339'}})).status,401);
 }finally{delete process.env.SUPA_SUPERVISED_REASONING_ENABLED;delete process.env.SUPA_SUPERVISED_TASKS_ENABLED;globalThis.fetch=oldFetch;if(oldUrl===undefined)delete process.env.SUPA_BACKEND_URL;else process.env.SUPA_BACKEND_URL=oldUrl;}
});

test('reasoning adapter permits only exact bounded model plans and never caller-supplied inference receipts',async()=>{
 process.env.SUPA_SUPERVISED_REASONING_ENABLED='true';
 const {supervisedRoute}=await import(taskModuleUrl),id='11111111-1111-4111-8111-111111111111';
 const cookie=session.issueFactorySession('synthetic',password),oldFetch=globalThis.fetch,oldUrl=process.env.SUPA_BACKEND_URL;
 const plan={provider:'mock',model:'synthetic-incident-v1',max_input_tokens:2000,max_output_tokens:256,max_requests:1,max_cost_microusd:0,external_tools:[]};
 const input={task:'Review synthetic incident',desired_outcome:'recovered',material:'Synthetic service recovered.',synthetic_only:true,retain_memory:false,reasoning:plan};
 const row={id,agent_id:id,creator_id:id,request_id:id,input,input_sha256:'a'.repeat(64),state:'pending',revision:0,step:0,created_at:new Date().toISOString(),approved_until:null,started_at:null,deadline:null,finished_at:null,payloads:[],error_code:null,events:[],inference:null};
 const path='/api/factory/agents/'+id+'/tasks',commandPath='/api/factory/tasks/'+id+'/command';let calls=0;
 const make=(body,p=path)=>new Request('http://127.0.0.1:3339'+p,{method:'POST',headers:{host:'127.0.0.1:3339',origin:'http://127.0.0.1:3339','Content-Type':'application/json',cookie:'supa_factory_session='+cookie},body:JSON.stringify(body)});
 try{
  process.env.SUPA_SUPERVISED_TASKS_ENABLED='true';process.env.SUPA_BACKEND_URL='http://127.0.0.1:3351';globalThis.fetch=async()=>{calls++;return Response.json(row);};
  for(const patch of [{provider:'live'},{model:'other'},{max_input_tokens:2001},{max_output_tokens:257},{max_requests:2},{max_cost_microusd:1},{external_tools:['web']}])assert.equal((await supervisedRoute(make({request_id:id,input:{...input,reasoning:{...plan,...patch}}}),path)).status,400);
  for(const operation of ['reserve_inference','settle_inference'])assert.equal((await supervisedRoute(make({request_id:id,operation,expected_revision:0,input_sha256:'a'.repeat(64),confirmed:true},commandPath),commandPath)).status,400);
  assert.equal((await supervisedRoute(make({request_id:id,input,payload:{invented:true}}),path)).status,400);assert.equal(calls,0);
  process.env.SUPA_SUPERVISED_REASONING_ENABLED='false';assert.equal((await supervisedRoute(make({request_id:id,input}),path)).status,403);assert.equal(calls,0);process.env.SUPA_SUPERVISED_REASONING_ENABLED='true';
  const response=await supervisedRoute(make({request_id:id,input}),path);assert.equal(response.status,200);assert.deepEqual((await response.json()).input.reasoning,plan);assert.equal(calls,1);
  const real={...plan,provider:'openai',model:'gpt-6-luna',max_cost_microusd:378,real:{project:'proj_fixture',budget_id:id,expires_at:new Date(Date.now()+300000).toISOString()}};
  for(const patch of [{max_cost_microusd:377},{model:'gpt-6-luna-latest'},{max_requests:2},{real:{...real.real,project:'untrusted'}}])assert.equal((await supervisedRoute(make({request_id:id,input:{...input,reasoning:{...real,...patch}}}),path)).status,400);
  globalThis.fetch=async()=>Response.json({...row,input:{...input,reasoning:real}});
  assert.equal((await supervisedRoute(make({request_id:id,input:{...input,reasoning:real}}),path)).status,200);
  globalThis.fetch=async()=>Response.json({...row,inference:{private_unsupported_field:'must_not_escape'}});
  const invalid=await supervisedRoute(make({request_id:id,input}),path);assert.equal(invalid.status,502);assert(!(await invalid.text()).includes('must_not_escape'));
 }finally{delete process.env.SUPA_SUPERVISED_REASONING_ENABLED;delete process.env.SUPA_SUPERVISED_TASKS_ENABLED;globalThis.fetch=oldFetch;if(oldUrl===undefined)delete process.env.SUPA_BACKEND_URL;else process.env.SUPA_BACKEND_URL=oldUrl;}
});
