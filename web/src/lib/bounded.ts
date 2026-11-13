/** Private deterministic goal scopes. Reuses Factory sessions; never forwards lease tokens. */
import { z } from 'zod';
import { factoryAuthorization, factorySameOrigin } from './factory-session';
import { localRequest, operatorToken } from './auth';
export const boundedEnabled = () => process.env.SUPA_SYNTHETIC_FACTORY_ENABLED === 'true' && process.env.SUPA_SUPERVISED_TASKS_ENABLED === 'true' && process.env.SUPA_BOUNDED_AUTONOMY_ENABLED === 'true';
const hash = z.string().regex(/^[0-9a-f]{64}$/);
const text = (max: number) => z.string().min(1).max(max);
const input = z.object({task:text(600),desired_outcome:text(120),material:text(4000),synthetic_only:z.literal(true),retain_memory:z.literal(false)}).strict();
export const boundedScope = z.object({goal_id:z.uuid(),priority:z.number().int().min(0).max(5),input,max_cycles:z.number().int().min(1).max(10),max_actions:z.number().int().min(3).max(30),max_spending_microusd:z.literal(0),interval_seconds:z.number().int().min(5).max(86400),valid_for_seconds:z.number().int().min(60).max(3600),mode:z.literal('deterministic')}).strict().refine(s=>s.max_actions>=s.max_cycles*3);
const baseCommand = z.object({request_id:z.uuid(),operation:z.enum(['approve','start','pause','resume','stop','review']),expected_revision:z.number().int().nonnegative().max(150),scope_sha256:hash,confirmed:z.boolean(),task_id:z.uuid().optional(),result_sha256:hash.optional(),accept:z.boolean().optional(),retain_memory:z.boolean().optional()}).strict();
const command = baseCommand.refine(c => {
 if (['approve','start','resume','review'].includes(c.operation) && !c.confirmed) return false;
 if (c.operation==='review') return !!c.task_id && !!c.result_sha256 && c.accept!==undefined && c.retain_memory!==undefined && (!c.retain_memory || c.accept);
 return c.task_id===undefined && c.result_sha256===undefined && c.accept===undefined && c.retain_memory===undefined;
});
const memory = z.object({id:z.uuid(),created_at:text(80),source_event_id:z.uuid(),kind:z.literal('hypothesis'),lesson:text(4000),source_task_id:z.uuid(),source_input_sha256:hash,owner_review_required:z.literal(true)}).strict();
const cycle = z.object({task_id:z.uuid(),run_id:z.uuid(),ordinal:z.number().int().min(1).max(10),state:z.enum(['running','awaiting_review','accepted','rejected','interrupted']),result_sha256:hash.nullable(),proposed_memory:memory.nullable(),approved_memory:memory.nullable()}).strict();
const run = z.object({id:z.uuid(),creator_id:z.uuid(),agent_id:z.uuid(),request_id:z.uuid(),scope:boundedScope,scope_sha256:hash,state:z.enum(['pending','approved','active','paused','stopped','exhausted','quarantined']),revision:z.number().int().min(0).max(150),cycles_started:z.number().int().min(0).max(10),actions_reserved:z.number().int().min(0).max(30),approved_until:text(80).nullable(),next_cycle_at:text(80),lease_until:text(80).nullable(),task_id:z.uuid().nullable(),created_at:text(80),mode:z.literal('deterministic'),spending_microusd:z.literal(0),cycles:z.array(cycle).max(10),events:z.array(z.unknown()).max(160)}).strict();
const goal = z.object({id:z.uuid(),description:text(600),completed:z.boolean(),priority:z.number().int().optional(),suggested_progress_bps:z.number().int().optional()}).strict();
const headers={'Cache-Control':'private, no-store','X-Content-Type-Options':'nosniff',Vary:'Cookie, Authorization'};
const fail=(status:number)=>Response.json({error:'bounded_schedule_denied_or_unavailable'},{status,headers});
async function json(request:Request|Response,limit:number) {
 const reader=request.body?.getReader();if(!reader)throw Error('body_required');let size=0;const chunks:Uint8Array[]=[];
 try {while(true){const c=await reader.read();if(c.done)break;size+=c.value.length;if(size>limit){await reader.cancel();throw Error('body_limit');}chunks.push(c.value);}}
 finally{reader.releaseLock();}return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}
export async function boundedRoute(request:Request,path:string):Promise<Response> {
 if(!boundedEnabled()||!localRequest(request)||new URL(request.url).search)return fail(403);
 const match=/^\/api\/factory\/agents\/([0-9a-f-]{36})\/autonomy$/.exec(path);
 if(!match||!z.uuid().safeParse(match[1]).success)return fail(404);
 const token=operatorToken(factoryAuthorization(request));if(!token)return fail(401);
 if(!['GET','POST'].includes(request.method))return fail(405);
 if(request.method==='POST'&&(!factorySameOrigin(request)||request.headers.get('content-type')!=='application/json'))return fail(403);
 try {
  const body=request.method==='POST'?z.union([z.object({request_id:z.uuid(),scope:boundedScope}).strict(),command]).parse(await json(request,10000)):undefined;
  const base=new URL(process.env.SUPA_BACKEND_URL??'');
  if(base.protocol!=='http:'||!(['127.0.0.1','[::1]'].includes(base.hostname)||(process.env.SUPA_SITE_MODE==='staging'&&/^[a-z0-9][a-z0-9-]*\.railway\.internal$/.test(base.hostname)))||base.username||base.password||base.pathname!=='/'||base.search||base.hash)return fail(503);
  const r=await fetch(new URL(path.slice(4),base),{method:request.method,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json'},body:body?JSON.stringify(body):undefined,redirect:'error',cache:'no-store',signal:AbortSignal.timeout(5000)});
  if(!r.ok){await r.body?.cancel();return fail([400,401,403,404,409,413,429].includes(r.status)?r.status:503);}
  if(!r.headers.get('content-type')?.includes('application/json')){await r.body?.cancel();return fail(502);}
  const result=z.object({run:run.nullable(),goals:z.array(goal).max(8).optional()}).strict().safeParse(await json(r,300000));
  if(!result.success)return fail(502);return Response.json(result.data,{headers});
 }catch(e){return fail(e instanceof z.ZodError||e instanceof SyntaxError?400:503);}
}
