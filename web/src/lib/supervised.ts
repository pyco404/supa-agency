/** Owner-only supervised task adapter; no caller-supplied results or identity. */
import { z } from 'zod';
import { factoryAuthorization, factorySameOrigin } from './factory-session';
import { localRequest, operatorToken } from './auth';
export const supervisedEnabled=()=>process.env.SUPA_SYNTHETIC_FACTORY_ENABLED==='true'&&process.env.SUPA_SUPERVISED_TASKS_ENABLED==='true';
const mockPlan=z.object({provider:z.literal('mock'),model:z.literal('synthetic-incident-v1'),max_input_tokens:z.literal(2000),max_output_tokens:z.literal(256),max_requests:z.literal(1),max_cost_microusd:z.literal(0),external_tools:z.array(z.never()).max(0)}).strict();
const realPlan=z.object({provider:z.literal('openai'),model:z.literal('gpt-6-luna'),max_input_tokens:z.literal(2000),max_output_tokens:z.literal(256),max_requests:z.literal(1),max_cost_microusd:z.literal(378),external_tools:z.array(z.never()).max(0),real:z.object({project:z.string().regex(/^proj_[A-Za-z0-9_-]{1,115}$/),budget_id:z.uuid(),expires_at:z.iso.datetime()}).strict()}).strict();
const plan=z.union([mockPlan,realPlan]);
const input=z.object({task:z.string().trim().min(1).max(600),desired_outcome:z.string().trim().min(1).max(120),material:z.string().trim().min(1).max(4000),synthetic_only:z.literal(true),retain_memory:z.boolean(),reasoning:plan.optional()}).strict();
const command=z.object({request_id:z.uuid(),operation:z.enum(['approve','start','advance','pause','resume','stop','review']),expected_revision:z.number().int().min(0).max(33),input_sha256:z.string().regex(/^[0-9a-f]{64}$/),confirmed:z.boolean(),result_sha256:z.string().regex(/^[0-9a-f]{64}$/).optional()}).strict();
const usage=z.object({input_tokens:z.number().int().nonnegative(),output_tokens:z.number().int().nonnegative(),cached_input_tokens:z.number().int().nonnegative(),cache_write_tokens:z.number().int().nonnegative()}).strict();
const inference=z.object({task_id:z.uuid(),request_id:z.uuid(),reservation_id:z.uuid(),reserved_revision:z.number().int(),request_body:command,model:z.enum(['synthetic-incident-v1','gpt-6-luna']),policy:plan,state:z.enum(['reserved','settled','discarded','failed']),reserved_microusd:z.union([z.literal(0),z.literal(378)]),actual_microusd:z.number().int().nonnegative().nullable(),diagnostic:z.unknown().optional(),usage:usage.nullable(),result_sha256:z.string().nullable(),outcome_code:z.string().nullable(),started_at:z.string(),finished_at:z.string().nullable()}).strict();
const row=z.object({id:z.uuid(),agent_id:z.uuid(),creator_id:z.uuid(),request_id:z.uuid(),input,input_sha256:z.string().regex(/^[0-9a-f]{64}$/),state:z.enum(['pending','approved','running','paused','awaiting_review','stopped','completed','failed']),revision:z.number().int().min(0).max(34),step:z.number().int().min(0).max(3),created_at:z.string(),approved_until:z.string().nullable(),started_at:z.string().nullable(),deadline:z.string().nullable(),finished_at:z.string().nullable(),payloads:z.array(z.unknown()).max(3),error_code:z.string().nullable(),events:z.array(z.unknown()).max(34),inference:inference.nullable().optional()}).strict();
const headers={'Cache-Control':'private, no-store','X-Content-Type-Options':'nosniff',Vary:'Cookie, Authorization'};
const fail=(status:number)=>Response.json({error:'supervised_task_denied_or_unavailable'}, {status,headers});
async function bounded(request:Request|Response,limit:number){const reader=request.body?.getReader();if(!reader)throw Error('body_required');let size=0;const chunks:Uint8Array[]=[];try{while(true){const c=await reader.read();if(c.done)break;size+=c.value.length;if(size>limit){await reader.cancel();throw Error('body_limit');}chunks.push(c.value);}}finally{reader.releaseLock();}return JSON.parse(Buffer.concat(chunks).toString('utf8'));}
export async function supervisedRoute(request:Request,path:string):Promise<Response>{
 if(!supervisedEnabled()||!localRequest(request)||new URL(request.url).search)return fail(403);
 const token=operatorToken(factoryAuthorization(request));if(!token)return fail(401);
 const list=/^\/api\/factory\/agents\/([0-9a-f-]{36})\/tasks$/.exec(path),task=/^\/api\/factory\/tasks\/([0-9a-f-]{36})(\/command)?$/.exec(path);
 if(!list&&!task)return fail(404);
 if(!['GET','POST'].includes(request.method)||(request.method==='GET'&&task?.[2])||(request.method==='POST'&&task&&!task[2]))return fail(405);
 if(request.method==='POST'&&(!factorySameOrigin(request)||request.headers.get('content-type')!=='application/json'))return fail(403);
 try{
  z.uuid().parse((list||task)![1]);
  let body;
  if(request.method==='POST')body=(list?z.object({request_id:z.uuid(),input}).strict():command).parse(await bounded(request,12000));
  if(list && request.method==='POST' && body && 'input' in body && body.input.reasoning && process.env.SUPA_SUPERVISED_REASONING_ENABLED!=='true')return fail(403);
  const base=new URL(process.env.SUPA_BACKEND_URL??'');
  if(base.protocol!=='http:'||!(['127.0.0.1','[::1]'].includes(base.hostname)||(process.env.SUPA_SITE_MODE==='staging'&&/^[a-z0-9][a-z0-9-]*\.railway\.internal$/.test(base.hostname)))||base.username||base.password||base.pathname!=='/'||base.search||base.hash)return fail(503);
  const response=await fetch(new URL(path.slice(4),base),{method:request.method,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json'},body:body?JSON.stringify(body):undefined,redirect:'error',cache:'no-store',signal:AbortSignal.timeout(35000)});
  if(!response.ok){await response.body?.cancel();return fail([400,401,403,404,409,413,429].includes(response.status)?response.status:503);}
  if(!response.headers.get('content-type')?.includes('application/json'))return fail(502);
  const result=(list&&request.method==='GET'?z.object({tasks:z.array(row).max(20)}).strict():row).safeParse(await bounded(response,1500000));if(!result.success)return fail(502);return Response.json(result.data,{headers});
 }catch(error){return fail(error instanceof z.ZodError||error instanceof SyntaxError?400:503);}
}
