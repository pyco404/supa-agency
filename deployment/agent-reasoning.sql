-- Explicit local-only upgrade after agent-supervised.sql. Never auto-installed.
BEGIN;
ALTER TABLE agent_supervised.tasks DROP CONSTRAINT tasks_state_check;
ALTER TABLE agent_supervised.tasks ADD CONSTRAINT tasks_state_check CHECK(state IN ('pending','approved','running','paused','awaiting_review','stopped','completed','failed'));
DROP INDEX agent_supervised.one_supervised_active_per_agent;
CREATE UNIQUE INDEX one_supervised_active_per_agent ON agent_supervised.tasks(agent_id) WHERE state IN ('approved','running','paused','awaiting_review');
CREATE TABLE agent_supervised.inferences (
 task_id uuid PRIMARY KEY REFERENCES agent_supervised.tasks, request_id uuid NOT NULL,
 reservation_id uuid NOT NULL UNIQUE, reserved_revision integer NOT NULL, request_body jsonb NOT NULL,
 model text NOT NULL CHECK(model IN ('synthetic-incident-v1','gpt-6-luna')), policy jsonb NOT NULL,
 state text NOT NULL DEFAULT 'reserved' CHECK(state IN ('reserved','settled','discarded','failed')),
 reserved_microusd bigint NOT NULL DEFAULT 0 CHECK(reserved_microusd IN (0,378)),
 actual_microusd bigint DEFAULT 0 CHECK(actual_microusd>=0),
 diagnostic jsonb, usage jsonb, result_sha256 text, outcome_code text,
 started_at timestamptz NOT NULL DEFAULT clock_timestamp(), finished_at timestamptz
);
REVOKE ALL ON agent_supervised.inferences FROM PUBLIC;
-- A budget authorizes one lifetime attempt; uncertain reservations cannot be recycled.
CREATE UNIQUE INDEX one_real_reasoning_budget ON agent_supervised.inferences (((policy->'real'->>'budget_id')::uuid)) WHERE policy ? 'real';
CREATE FUNCTION agent_supervised.reasoning_plan() RETURNS jsonb LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT '{"provider":"mock","model":"synthetic-incident-v1","max_input_tokens":2000,"max_output_tokens":256,"max_requests":1,"max_cost_microusd":0,"external_tools":[]}'::jsonb
$$;
CREATE FUNCTION agent_supervised.reasoning_plan_valid(p jsonb) RETURNS boolean LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog AS $$
BEGIN
 IF p=agent_supervised.reasoning_plan() THEN RETURN true; END IF;
 IF jsonb_typeof(p->'real') IS DISTINCT FROM 'object' THEN RETURN false; END IF;
 IF p->'real'->>'budget_id' IS NULL OR p->'real'->>'project' IS NULL OR (p->'real'->>'budget_id')::uuid='00000000-0000-0000-0000-000000000000'::uuid
 OR NOT (p->'real'->>'project' ~ '^proj_[A-Za-z0-9_-]{1,115}$')
 OR (p->'real'->>'expires_at')::timestamptz IS NULL THEN RETURN false; END IF;
 RETURN p = (agent_supervised.reasoning_plan() || jsonb_build_object('provider','openai','model','gpt-6-luna','max_cost_microusd',378,'real',
  jsonb_build_object('project',p->'real'->>'project','budget_id',p->'real'->>'budget_id','expires_at',p->'real'->>'expires_at')));
EXCEPTION WHEN invalid_text_representation OR invalid_datetime_format OR datetime_field_overflow THEN RETURN false;
END;
$$;
CREATE OR REPLACE FUNCTION agent_supervised.view_task(t agent_supervised.tasks) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT to_jsonb(t)||jsonb_build_object('events',coalesce((SELECT jsonb_agg(to_jsonb(e) ORDER BY revision) FROM agent_supervised.events e WHERE task_id=t.id),'[]'))
 ||CASE WHEN t.input ? 'reasoning' THEN jsonb_build_object('inference',(SELECT to_jsonb(i) FROM agent_supervised.inferences i WHERE task_id=t.id)) ELSE '{}'::jsonb END
$$;
CREATE OR REPLACE FUNCTION agent_supervised.command(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE t agent_supervised.tasks%ROWTYPE; old agent_supervised.commands%ROWTYPE;
 aid uuid; request uuid; f agent_factory.submissions%ROWTYPE; event_body jsonb; previous text;
 now_at timestamptz:=clock_timestamp(); content jsonb; action text; inf agent_supervised.inferences%ROWTYPE; permitted boolean;
BEGIN
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid
 OR EXISTS(SELECT 1 FROM public.application_epoch) OR jsonb_typeof(b)<>'object' THEN
  RAISE EXCEPTION 'supervised_scope_denied' USING ERRCODE='42501'; END IF;
 IF op IN ('list','create') THEN aid=(b->>'agent_id')::uuid;
 ELSE SELECT agent_id INTO aid FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor; END IF;
 -- Accounting settlement survives revocation; no result or memory is released
 -- to the former owner. The HTTP command schema never accepts this operation.
 IF op='settle_inference' THEN
  PERFORM 1 FROM public.agent_access WHERE agent_id=aid AND principal_id=actor AND role='owner' FOR SHARE;
  permitted=FOUND;
  SELECT * INTO t FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'settlement_scope' USING ERRCODE='42501'; END IF;
  SELECT * INTO inf FROM agent_supervised.inferences WHERE task_id=t.id FOR UPDATE;
  IF NOT FOUND OR inf.reservation_id IS DISTINCT FROM (b->>'reservation_id')::uuid OR inf.state<>'reserved' THEN
   RAISE EXCEPTION 'inference_settlement_conflict' USING ERRCODE='40001'; END IF;
  IF b->'usage'<>'null'::jsonb AND (jsonb_typeof(b->'usage')<>'object'
   OR NOT b->'usage' ?& ARRAY['input_tokens','output_tokens','cached_input_tokens','cache_write_tokens']
   OR EXISTS(SELECT 1 FROM jsonb_each(b->'usage') e WHERE e.key NOT IN ('input_tokens','output_tokens','cached_input_tokens','cache_write_tokens') OR jsonb_typeof(e.value)<>'number' OR e.value::text !~ '^[0-9]{1,9}$')) THEN
   RAISE EXCEPTION 'invalid_usage_receipt' USING ERRCODE='22023'; END IF;
  permitted=permitted AND t.state='running' AND t.step=1 AND t.revision=inf.reserved_revision
   AND t.approved_until>now_at AND t.deadline>now_at;
  inf.usage=b->'usage';inf.finished_at=now_at;inf.diagnostic=CASE WHEN jsonb_typeof(b->'diagnostic')='object' AND octet_length((b->'diagnostic')::text)<=4096 THEN b->'diagnostic' ELSE NULL END;
  IF inf.policy ? 'real' THEN
   -- Unknown usage remains NULL with the full reservation held, never free/$0.
   inf.actual_microusd=NULL;
   IF jsonb_typeof(inf.usage)='object' AND (inf.usage->>'input_tokens')::bigint>=(inf.usage->>'cached_input_tokens')::bigint+(inf.usage->>'cache_write_tokens')::bigint THEN
    inf.actual_microusd=ceil((((inf.usage->>'input_tokens')::numeric-(inf.usage->>'cached_input_tokens')::numeric-(inf.usage->>'cache_write_tokens')::numeric)*100000+(inf.usage->>'cached_input_tokens')::numeric*10000+(inf.usage->>'cache_write_tokens')::numeric*125000+(inf.usage->>'output_tokens')::numeric*500000)/1000000)::bigint;
   END IF;
   permitted=permitted AND (inf.policy->'real'->>'expires_at')::timestamptz>now_at;
  END IF;
  IF NOT permitted THEN inf.state='discarded';inf.outcome_code='cancelled_revoked_or_expired';
  ELSIF b->>'outcome'='accepted' AND jsonb_typeof(b->'payload')='object'
   AND b->'payload'->>'action'='reasoning_result' AND octet_length((b->'payload')::text)<=16000
   AND jsonb_typeof(b->'payload'->'report')='object'
   AND b->'payload'->>'model'=inf.model
   AND ((NOT inf.policy ? 'real' AND b->'payload'->>'provider'='mock' AND b->'payload'->'mock_model'='true'::jsonb AND b->'payload'->'spending_microusd'='0'::jsonb AND b->'payload'->'external_requests'='0'::jsonb)
    OR (inf.policy ? 'real' AND b->'payload'->>'provider'='openai' AND b->'payload'->'mock_model'='false'::jsonb AND b->'payload'->'external_requests'='3'::jsonb AND inf.actual_microusd BETWEEN 1 AND inf.reserved_microusd AND b->'payload'->'spending_microusd'=to_jsonb(inf.actual_microusd)))
   AND (b->'usage'->>'input_tokens')::bigint BETWEEN 1 AND 2000
   AND (b->'usage'->>'output_tokens')::bigint BETWEEN 1 AND 256
   AND ((inf.policy ? 'real' AND (b->'usage'->>'cached_input_tokens')::bigint+(b->'usage'->>'cache_write_tokens')::bigint<=(b->'usage'->>'input_tokens')::bigint)
    OR (b->'usage'->>'cached_input_tokens'='0' AND b->'usage'->>'cache_write_tokens'='0')) THEN
    inf.state='settled';inf.outcome_code='owner_review_required';
    inf.result_sha256=encode(sha256(convert_to((b->'payload'->'report')::text,'UTF8')),'hex');
    t.payloads=t.payloads||jsonb_build_array((b->'payload')||jsonb_build_object('result_sha256',inf.result_sha256));
    t.step=2;t.state='awaiting_review';
  ELSE inf.state='failed';inf.outcome_code=CASE WHEN inf.policy ? 'real' AND inf.usage='null'::jsonb THEN 'model_usage_unknown' ELSE 'model_response_rejected' END;t.state='failed';t.error_code=inf.outcome_code;t.finished_at=now_at;
  END IF;
  UPDATE agent_supervised.inferences SET actual_microusd=inf.actual_microusd,diagnostic=inf.diagnostic,state=inf.state,usage=inf.usage,finished_at=inf.finished_at,outcome_code=inf.outcome_code,result_sha256=inf.result_sha256 WHERE task_id=t.id;
  t.revision=t.revision+1;
  UPDATE agent_supervised.tasks SET revision=t.revision,state=t.state,step=t.step,payloads=t.payloads,error_code=t.error_code,finished_at=t.finished_at WHERE id=t.id;
  SELECT sha256 INTO previous FROM agent_supervised.events WHERE task_id=t.id ORDER BY revision DESC LIMIT 1;
  previous=coalesce(previous,repeat('0',64));
  event_body=jsonb_build_object('actor',actor,'operation',op,'state',t.state,'step',t.step,'recorded_at',now_at,'input_sha256',t.input_sha256,'payloads_sha256',encode(sha256(convert_to(t.payloads::text,'UTF8')),'hex'),'inference_sha256',encode(sha256(convert_to(to_jsonb(inf)::text,'UTF8')),'hex'));
  INSERT INTO agent_supervised.events(task_id,revision,body,previous_sha256,sha256) VALUES(t.id,t.revision,event_body,previous,encode(sha256(convert_to(previous||event_body::text,'UTF8')),'hex'));
  IF NOT EXISTS(SELECT 1 FROM public.agent_access WHERE agent_id=aid AND principal_id=actor AND role='owner') THEN RETURN 'null'::jsonb; END IF;
  RETURN agent_supervised.view_task(t);
 END IF;
 -- Serialize revocation with all owner operations, including result commits.
 PERFORM 1 FROM public.agent_access WHERE agent_id=aid AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'supervised_owner_unavailable' USING ERRCODE='42501'; END IF;
 SELECT * INTO f FROM agent_factory.submissions WHERE agent_id=aid AND creator_id=actor;
 IF NOT FOUND THEN RAISE EXCEPTION 'supervised_factory_agent_required' USING ERRCODE='42501'; END IF;
 IF op='list' THEN RETURN jsonb_build_object('tasks',coalesce((SELECT jsonb_agg(agent_supervised.view_task(q) ORDER BY q.created_at) FROM agent_supervised.tasks q WHERE q.agent_id=aid AND q.creator_id=actor),'[]')); END IF;
 IF op='get' THEN SELECT * INTO t FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor; RETURN agent_supervised.view_task(t); END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('supervised:'||actor::text,0));
 request=(b->>'request_id')::uuid;
 IF request IS NULL OR request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION 'request_required' USING ERRCODE='22023'; END IF;
 IF op='create' THEN
  content=b->'input';
  IF jsonb_typeof(content)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(content))<>(CASE WHEN content ? 'reasoning' THEN 6 ELSE 5 END)
  OR (content ? 'reasoning' AND NOT coalesce(agent_supervised.reasoning_plan_valid(content->'reasoning'),false))
  OR NOT content ?& ARRAY['task','desired_outcome','material','synthetic_only','retain_memory']
  OR content->'synthetic_only'<>'true'::jsonb OR jsonb_typeof(content->'retain_memory')<>'boolean'
  OR jsonb_typeof(content->'task')<>'string' OR length(content->>'task') NOT BETWEEN 1 AND 600
  OR jsonb_typeof(content->'desired_outcome')<>'string' OR length(content->>'desired_outcome') NOT BETWEEN 1 AND 120
  OR jsonb_typeof(content->'material')<>'string' OR octet_length(content->>'material') NOT BETWEEN 1 AND 4000
  OR (content->'retain_memory'='true'::jsonb AND NOT coalesce(f.configuration->'requested_capabilities' ? 'remember',false)) THEN
   RAISE EXCEPTION 'supervised_input_or_permission_invalid' USING ERRCODE='42501'; END IF;
  IF content->'reasoning' ? 'real' AND ((content->'reasoning'->'real'->>'expires_at')::timestamptz<=now_at OR (content->'reasoning'->'real'->>'expires_at')::timestamptz>now_at+interval '10 minutes') THEN RAISE EXCEPTION 'model_consent_expired' USING ERRCODE='42501'; END IF;
  SELECT * INTO t FROM agent_supervised.tasks WHERE agent_id=aid AND creator_id=actor AND request_id=request;
  IF FOUND THEN IF t.input<>content THEN RAISE EXCEPTION 'request_conflict' USING ERRCODE='40001'; END IF; RETURN agent_supervised.view_task(t); END IF;
  IF (SELECT count(*) FROM agent_supervised.tasks WHERE creator_id=actor)>=100 OR (SELECT count(*) FROM agent_supervised.tasks WHERE agent_id=aid)>=20 THEN RAISE EXCEPTION 'task_quota' USING ERRCODE='54000'; END IF;
  INSERT INTO agent_supervised.tasks(agent_id,creator_id,request_id,input,input_sha256) VALUES(aid,actor,request,content,encode(sha256(convert_to(content::text,'UTF8')),'hex')) RETURNING * INTO t;
 ELSE
  SELECT * INTO t FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor FOR UPDATE;
  SELECT * INTO old FROM agent_supervised.commands WHERE creator_id=actor AND request_id=request;
  IF FOUND THEN IF old.task_id<>t.id OR old.body<>(b-'payload')||jsonb_build_object('operation',op) THEN RAISE EXCEPTION 'command_conflict' USING ERRCODE='40001'; END IF; RETURN agent_supervised.view_task(t); END IF;
  IF t.revision IS DISTINCT FROM (b->>'expected_revision')::integer OR t.input_sha256 IS DISTINCT FROM b->>'input_sha256' THEN RAISE EXCEPTION 'stale_review' USING ERRCODE='40001'; END IF;
  IF t.state IN ('completed','stopped','failed') OR (t.revision>=32 AND op<>'stop') THEN RAISE EXCEPTION 'terminal_or_command_limit' USING ERRCODE='42501'; END IF;
  IF op='reserve_inference' AND t.state='running' AND t.step=1 AND t.input ? 'reasoning'
   AND t.approved_until>now_at AND t.deadline>now_at THEN
   IF t.input->'reasoning' ? 'real' AND (t.input->'reasoning'->'real'->>'expires_at')::timestamptz<=now_at THEN RAISE EXCEPTION 'model_consent_expired' USING ERRCODE='42501'; END IF;
   INSERT INTO agent_supervised.inferences(task_id,request_id,reservation_id,reserved_revision,request_body,model,policy,reserved_microusd,actual_microusd)
    VALUES(t.id,request,(b->>'reservation_id')::uuid,t.revision+1,b-'reservation_id'-'task_id',t.input->'reasoning'->>'model',t.input->'reasoning',CASE WHEN t.input->'reasoning' ? 'real' THEN 378 ELSE 0 END,CASE WHEN t.input->'reasoning' ? 'real' THEN NULL ELSE 0 END);
  ELSIF op='review' AND t.state='awaiting_review' AND b->'confirmed'='true'::jsonb AND t.approved_until>now_at AND (NOT t.input->'reasoning' ? 'real' OR (t.input->'reasoning'->'real'->>'expires_at')::timestamptz>now_at)
   AND b->>'result_sha256' IS NOT NULL AND b->>'result_sha256'=t.payloads->1->>'result_sha256' THEN
   IF b->'payload'->>'action' IS DISTINCT FROM 'retain_and_assess' OR octet_length((b->'payload')::text)>16000 THEN RAISE EXCEPTION 'review_result_invalid' USING ERRCODE='22023'; END IF;
   t.payloads=t.payloads||jsonb_build_array(b->'payload');t.step=3;t.state='completed';t.finished_at=now_at;
  ELSIF op='stop' THEN t.state='stopped';t.finished_at=now_at;
  ELSIF op='pause' AND t.state='running' THEN t.state='paused';
  ELSIF op='approve' AND t.state='pending' AND b->'confirmed'='true'::jsonb AND now_at<t.created_at+interval '1 hour' THEN t.state='approved';t.approved_until=now_at+interval '10 minutes';
  ELSIF op IN ('start','resume') AND b->'confirmed'='true'::jsonb AND t.approved_until>now_at AND ((op='start' AND t.state='approved') OR (op='resume' AND t.state='paused')) THEN
   t.state='running';t.started_at=coalesce(t.started_at,now_at);t.deadline=coalesce(t.deadline,least(now_at+interval '120 seconds',t.created_at+interval '1 hour'));
   IF t.deadline<=now_at THEN RAISE EXCEPTION 'execution_expired' USING ERRCODE='42501'; END IF;
  ELSIF op='advance' AND t.state='running' AND NOT(t.input ? 'reasoning' AND t.step>0) THEN
   IF t.deadline<=now_at OR t.approved_until<=now_at THEN t.state='failed';t.error_code='execution_deadline_exceeded';t.finished_at=now_at;
   ELSE
    action=CASE t.step WHEN 0 THEN 'inspect_input' WHEN 1 THEN 'extract_evidence' WHEN 2 THEN 'retain_and_assess' END;
    IF jsonb_typeof(b->'payload')<>'object' OR b->'payload'->>'action' IS DISTINCT FROM action OR octet_length((b->'payload')::text)>16000 THEN RAISE EXCEPTION 'invalid_runtime_result' USING ERRCODE='22023'; END IF;
    t.payloads=t.payloads||jsonb_build_array(b->'payload');t.step=t.step+1;
    IF t.step=3 THEN t.state='completed';t.finished_at=now_at;END IF;
   END IF;
  ELSE RAISE EXCEPTION 'transition_not_authorized' USING ERRCODE='42501'; END IF;
  t.revision=t.revision+1;
  UPDATE agent_supervised.tasks SET state=t.state,revision=t.revision,step=t.step,approved_until=t.approved_until,started_at=t.started_at,deadline=t.deadline,finished_at=t.finished_at,payloads=t.payloads,error_code=t.error_code WHERE id=t.id;
 END IF;
 IF op<>'reserve_inference' THEN
 INSERT INTO agent_supervised.commands VALUES(actor,request,t.id,(b-'payload')||jsonb_build_object('operation',op));
 END IF;
 SELECT sha256 INTO previous FROM agent_supervised.events WHERE task_id=t.id ORDER BY revision DESC LIMIT 1;previous=coalesce(previous,repeat('0',64));
 event_body=jsonb_build_object('actor',actor,'operation',op,'state',t.state,'step',t.step,'recorded_at',now_at,'input_sha256',t.input_sha256,'payloads_sha256',encode(sha256(convert_to(t.payloads::text,'UTF8')),'hex'),'error_code',t.error_code,'inference_sha256',(SELECT encode(sha256(convert_to(to_jsonb(i)::text,'UTF8')),'hex') FROM agent_supervised.inferences i WHERE task_id=t.id));
 INSERT INTO agent_supervised.events(task_id,revision,body,previous_sha256,sha256) VALUES(t.id,t.revision,event_body,previous,encode(sha256(convert_to(previous||event_body::text,'UTF8')),'hex'));
 RETURN agent_supervised.view_task(t);
END;
$$;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA agent_supervised FROM PUBLIC;
-- Preserve the existing sole command-function grant; never grant table writes.
COMMIT;
